//! The viewer's files, taken from the interface embedded in the executable
//! (docs/design/lan-share.md, "Viewer assets from the executable").
//!
//! Tauri's own resolver is never used for the share: it percent-decodes paths and falls back to
//! `index.html` for anything it does not know. Instead `lib.rs` shares the embedded assets
//! ([`share_context_assets`]) and the share reads, by exact key, only the files Vite's build
//! manifest lists for `viewer.html`.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ardoise_share::{Asset, Assets};
use serde::Deserialize;
use tauri::utils::assets::{AssetKey, AssetsIter, CspHash};
use tauri::{Context, Runtime};

/// The page of the viewer (a Vite entry next to `index.html`).
const VIEWER_ENTRY: &str = "viewer.html";
/// Vite's build manifest (`build.manifest: true`).
const MANIFEST: &str = ".vite/manifest.json";

/// Reads one embedded file by its path in the built interface (`viewer.html`,
/// `assets/viewer-x.js`); `None` when it is not there.
pub type AssetSource = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

/// Why the viewer cannot be served.
#[derive(Debug, thiserror::Error)]
pub enum ViewerAssetsError {
    /// A development build (the interface comes from the dev server) or a build without the
    /// viewer.
    #[error("the viewer is not embedded in this build of Ardoise ({0} missing)")]
    Missing(String),
    #[error("the build manifest of the interface cannot be read: {0}")]
    Manifest(String),
}

/// One entry of Vite's manifest.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Chunk {
    file: String,
    #[serde(default)]
    css: Vec<String>,
    #[serde(default)]
    assets: Vec<String>,
    #[serde(default)]
    imports: Vec<String>,
    #[serde(default)]
    dynamic_imports: Vec<String>,
}

/// `Content-Type` of a file of the viewer, by extension.
pub fn content_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    match extension.to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

/// A file named by the manifest: under `assets/`, made of plain characters only (Vite's
/// `name-hash.ext`). Anything else is refused, whatever the manifest says.
fn plain_asset_path(path: &str) -> bool {
    path.strip_prefix("assets/").is_some_and(|name| {
        !name.is_empty()
            && !name.starts_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    })
}

/// The viewer's files: `viewer.html` under `/`, and the files the manifest lists for it
/// (its script, the shared chunks it imports, its style sheets, its assets such as the
/// WebAssembly module), each under `/assets/<file>`.
pub fn viewer_assets(get: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Assets, ViewerAssetsError> {
    let page = get(VIEWER_ENTRY).ok_or_else(|| ViewerAssetsError::Missing(VIEWER_ENTRY.into()))?;
    let manifest = get(MANIFEST).ok_or_else(|| ViewerAssetsError::Missing(MANIFEST.into()))?;
    let manifest: HashMap<String, Chunk> = serde_json::from_slice(&manifest)
        .map_err(|error| ViewerAssetsError::Manifest(error.to_string()))?;
    let entry = manifest
        .get(VIEWER_ENTRY)
        .ok_or_else(|| ViewerAssetsError::Manifest(format!("no entry for {VIEWER_ENTRY}")))?;

    let mut files: Vec<String> = Vec::new();
    let mut seen_chunks: HashSet<&str> = HashSet::new();
    let mut pending: Vec<&Chunk> = vec![entry];
    while let Some(chunk) = pending.pop() {
        for file in std::iter::once(&chunk.file)
            .chain(&chunk.css)
            .chain(&chunk.assets)
        {
            if !files.contains(file) {
                files.push(file.clone());
            }
        }
        for import in chunk.imports.iter().chain(&chunk.dynamic_imports) {
            if seen_chunks.insert(import.as_str()) {
                let next = manifest.get(import).ok_or_else(|| {
                    ViewerAssetsError::Manifest(format!("unknown chunk {import}"))
                })?;
                pending.push(next);
            }
        }
    }

    // Without its geometry module the viewer would draw no stroke: a build where Vite stopped
    // listing it is refused rather than served half-working.
    if !files.iter().any(|file| file.ends_with(".wasm")) {
        return Err(ViewerAssetsError::Manifest(
            "no WebAssembly module for viewer.html".into(),
        ));
    }

    let mut assets = Assets::new();
    assets.insert(
        "/".to_owned(),
        Asset {
            content_type: content_type(VIEWER_ENTRY).to_owned(),
            bytes: Arc::from(page),
        },
    );
    for file in files {
        if !plain_asset_path(&file) {
            return Err(ViewerAssetsError::Manifest(format!(
                "unexpected file name {file}"
            )));
        }
        let bytes = get(&file).ok_or_else(|| ViewerAssetsError::Missing(file.clone()))?;
        assets.insert(
            format!("/{file}"),
            Asset {
                content_type: content_type(&file).to_owned(),
                bytes: Arc::from(bytes),
            },
        );
    }
    Ok(assets)
}

/// The embedded interface, served to the windows by Tauri as before, and readable by the share.
struct SharedAssets<R: Runtime>(Arc<dyn tauri::Assets<R>>);

impl<R: Runtime> tauri::Assets<R> for SharedAssets<R> {
    fn setup(&self, app: &tauri::App<R>) {
        self.0.setup(app);
    }

    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        self.0.get(key)
    }

    fn iter(&self) -> Box<AssetsIter<'_>> {
        self.0.iter()
    }

    fn csp_hashes(&self, html_path: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        self.0.csp_hashes(html_path)
    }
}

/// Stands in for a moment while the real assets are moved.
struct NoAssets;

impl<R: Runtime> tauri::Assets<R> for NoAssets {
    fn get(&self, _key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        None
    }

    fn iter(&self) -> Box<AssetsIter<'_>> {
        Box::new(std::iter::empty())
    }

    fn csp_hashes(&self, _html_path: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

/// Shares the assets embedded in `context` (Tauri keeps serving them to the windows) and returns
/// a reader of them by exact path, for the share.
pub fn share_context_assets<R: Runtime>(context: &mut Context<R>) -> AssetSource {
    let embedded: Arc<dyn tauri::Assets<R>> = Arc::from(context.set_assets(Box::new(NoAssets)));
    context.set_assets(Box::new(SharedAssets(embedded.clone())));
    Arc::new(move |path: &str| {
        embedded
            .get(&AssetKey::from(path))
            .map(|bytes| bytes.into_owned())
    })
}
