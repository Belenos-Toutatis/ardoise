//! The « Guide d'utilisation » button and the F1 key: opens the user guide in the default browser.
//!
//! The distribution archive puts the guides in a `guide` folder next to `ardoise.exe`
//! (`guide.<lang>.html`, `assistant.<lang>.html`; `cargo xtask dist`). The page in the language of the
//! interface is opened from there, offline. When the file is missing (someone kept only the exe), the same
//! guide on GitHub is opened instead, and the answer says so, because Ardoise otherwise never goes online:
//! the interface tells the user that a web address was opened at their request.
//!
//! What is opened never comes from the interface. The interface only chooses between two pages and two
//! languages (enums that serde refuses to read any other way); the file is found from the executable's own
//! folder, must be a regular file directly inside `guide` (a link leading elsewhere is no page), and the web
//! address is built here from constants. The system opens a local `.html` with the browser, never runs it.
//!
//! For the journeys and the self-test, `ARDOISE_OPEN_LOG=<file>` makes the command write what it would open
//! (one line per page) to that file instead of starting a browser; the answer is the same.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::commands::{CommandError, CommandResult};

/// The folder of the guides, next to the executable.
const GUIDE_FOLDER: &str = "guide";
/// Where the guides are on the web (the Markdown sources, which GitHub shows as pages).
const WEB_GUIDE: &str = "https://github.com/Belenos-Toutatis/ardoise/blob/main/docs/guide/";
/// Names a file that receives what would be opened, instead of opening it (tests and journeys).
pub const OPEN_LOG_ENV: &str = "ARDOISE_OPEN_LOG";
/// Error code when the system could not open the page.
pub const GUIDE_UNAVAILABLE: &str = "guideUnavailable";

/// `GuidePage` in `guide.ts`: the user guide, or the guide of the assistant (MCP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuidePage {
    Guide,
    Assistant,
}

impl GuidePage {
    fn stem(self) -> &'static str {
        match self {
            GuidePage::Guide => "guide",
            GuidePage::Assistant => "assistant",
        }
    }
}

/// The language of the page: the one the interface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuideLanguage {
    Fr,
    En,
}

impl GuideLanguage {
    fn code(self) -> &'static str {
        match self {
            GuideLanguage::Fr => "fr",
            GuideLanguage::En => "en",
        }
    }
}

/// `GuideOpened` in `guide.ts`: where the page came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GuideOpened {
    /// The page of the `guide` folder next to the executable (offline).
    Local,
    /// The page on GitHub: the folder, or the page in it, is missing.
    Web,
}

/// `guide.fr.html`, `assistant.en.md`…
fn page_file(page: GuidePage, language: GuideLanguage, extension: &str) -> String {
    format!("{}.{}.{extension}", page.stem(), language.code())
}

/// The address of the page on GitHub.
fn web_address(page: GuidePage, language: GuideLanguage) -> String {
    format!("{WEB_GUIDE}{}", page_file(page, language, "md"))
}

/// The page `file` of the `guide` folder in `exe_dir`, when it is there: a regular file whose real place is
/// directly inside the real `guide` folder (a link to a file elsewhere, or a `guide` that is a link elsewhere,
/// does not count). The path returned is the one under `exe_dir`, as the user knows it.
fn local_page(exe_dir: &Path, file: &str) -> Option<PathBuf> {
    let folder = exe_dir.join(GUIDE_FOLDER);
    let page = folder.join(file);
    let real_folder = folder.canonicalize().ok()?;
    let real_page = page.canonicalize().ok()?;
    (real_page.is_file() && real_page.parent() == Some(real_folder.as_path())).then_some(page)
}

/// What starts the browser, or stands for it.
trait Opener {
    /// Opens a file path or a web address; the error is the system's wording, for the log.
    fn open(&self, target: &str) -> Result<(), String>;
}

/// Writes what it is asked to open to a file (`ARDOISE_OPEN_LOG`).
struct LogOpener(PathBuf);

impl Opener for LogOpener {
    fn open(&self, target: &str) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.0)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{target}").map_err(|error| error.to_string())
    }
}

/// The system's default handler.
struct SystemOpener;

impl Opener for SystemOpener {
    fn open(&self, target: &str) -> Result<(), String> {
        system::open(target)
    }
}

/// The folder of the running executable, without the `\\?\` prefix the system may give it.
fn exe_folder() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let folder = exe.parent()?;
    let text = folder.to_string_lossy();
    Some(match text.strip_prefix(r"\\?\") {
        Some(plain) if !plain.starts_with("UNC") => PathBuf::from(plain),
        _ => folder.to_path_buf(),
    })
}

/// Opens `page` in `language`: from `exe_dir` when it is there, else from the web. Only an opener that fails is an error.
fn open_guide_in(
    exe_dir: Option<&Path>,
    page: GuidePage,
    language: GuideLanguage,
    opener: &dyn Opener,
) -> CommandResult<GuideOpened> {
    let local = exe_dir.and_then(|dir| local_page(dir, &page_file(page, language, "html")));
    let (target, opened) = match local {
        Some(path) => (path.to_string_lossy().into_owned(), GuideOpened::Local),
        None => (web_address(page, language), GuideOpened::Web),
    };
    opener
        .open(&target)
        .map_err(|error| CommandError::new(GUIDE_UNAVAILABLE, error))?;
    Ok(opened)
}

/// Opens the user guide, or the assistant's, in the default browser (`api.openGuide`). Says which source it used.
#[tauri::command]
pub async fn open_guide(page: GuidePage, language: GuideLanguage) -> CommandResult<GuideOpened> {
    // Starting the browser can take a moment: not on the thread of the window.
    tauri::async_runtime::spawn_blocking(move || {
        let exe_dir = exe_folder();
        match std::env::var_os(OPEN_LOG_ENV).filter(|path| !path.is_empty()) {
            Some(log) => open_guide_in(
                exe_dir.as_deref(),
                page,
                language,
                &LogOpener(PathBuf::from(log)),
            ),
            None => open_guide_in(exe_dir.as_deref(), page, language, &SystemOpener),
        }
    })
    .await
    .map_err(|error| CommandError::new(GUIDE_UNAVAILABLE, error))?
}

#[cfg(windows)]
mod system {
    //! `ShellExecuteW`, with the verb `open`: the file's or the address's default handler.

    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};

    /// What `ShellExecuteW` answers above is a success (below, an error code).
    const SUCCESS_ABOVE: usize = 32;

    pub(super) fn open(target: &str) -> Result<(), String> {
        let target = HSTRING::from(target);
        // The shell asks for an initialised COM on the calling thread (some handlers need it).
        // SAFETY: plain COM initialisation of this thread, undone below when it succeeded.
        let com =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        // SAFETY: the strings live for the call; no window, no parameters, no folder.
        let result = unsafe { ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL) };
        if com.is_ok() {
            // SAFETY: paired with the successful `CoInitializeEx` above.
            unsafe { CoUninitialize() };
        }
        let code = result.0 as usize;
        if code > SUCCESS_ABOVE {
            Ok(())
        } else {
            Err(format!("ShellExecute error {code}"))
        }
    }
}

#[cfg(not(windows))]
mod system {
    //! The desktop's opener (`xdg-open`), given the target as one argument: no shell involved.

    use std::process::{Command, Stdio};

    pub(super) fn open(target: &str) -> Result<(), String> {
        Command::new("xdg-open")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;

    use super::*;

    /// Remembers what it was asked to open.
    #[derive(Default)]
    struct Recorder {
        opened: RefCell<Vec<String>>,
        fail: bool,
    }

    impl Opener for Recorder {
        fn open(&self, target: &str) -> Result<(), String> {
            if self.fail {
                return Err("no handler".into());
            }
            self.opened.borrow_mut().push(target.to_owned());
            Ok(())
        }
    }

    /// A folder with a `guide` folder holding the four pages.
    fn distribution() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let guide = dir.path().join(GUIDE_FOLDER);
        fs::create_dir(&guide).unwrap();
        for stem in ["guide", "assistant"] {
            for code in ["fr", "en"] {
                fs::write(guide.join(format!("{stem}.{code}.html")), "<html></html>").unwrap();
            }
        }
        dir
    }

    fn open(
        dir: &Path,
        page: GuidePage,
        language: GuideLanguage,
    ) -> (CommandResult<GuideOpened>, Vec<String>) {
        let recorder = Recorder::default();
        let result = open_guide_in(Some(dir), page, language, &recorder);
        (result, recorder.opened.into_inner())
    }

    #[test]
    fn the_page_of_the_language_is_opened_from_the_guide_folder() {
        let dir = distribution();
        for (page, language, name) in [
            (GuidePage::Guide, GuideLanguage::Fr, "guide.fr.html"),
            (GuidePage::Guide, GuideLanguage::En, "guide.en.html"),
            (GuidePage::Assistant, GuideLanguage::Fr, "assistant.fr.html"),
            (GuidePage::Assistant, GuideLanguage::En, "assistant.en.html"),
        ] {
            let (result, opened) = open(dir.path(), page, language);
            assert_eq!(result.unwrap(), GuideOpened::Local);
            assert_eq!(
                opened,
                vec![dir.path().join("guide").join(name).to_string_lossy()]
            );
        }
    }

    #[test]
    fn a_missing_folder_opens_the_guide_on_github_in_the_right_language() {
        let dir = tempfile::tempdir().unwrap();
        for (page, language, address) in [
            (
                GuidePage::Guide,
                GuideLanguage::Fr,
                "https://github.com/Belenos-Toutatis/ardoise/blob/main/docs/guide/guide.fr.md",
            ),
            (
                GuidePage::Guide,
                GuideLanguage::En,
                "https://github.com/Belenos-Toutatis/ardoise/blob/main/docs/guide/guide.en.md",
            ),
            (
                GuidePage::Assistant,
                GuideLanguage::Fr,
                "https://github.com/Belenos-Toutatis/ardoise/blob/main/docs/guide/assistant.fr.md",
            ),
            (
                GuidePage::Assistant,
                GuideLanguage::En,
                "https://github.com/Belenos-Toutatis/ardoise/blob/main/docs/guide/assistant.en.md",
            ),
        ] {
            let (result, opened) = open(dir.path(), page, language);
            assert_eq!(result.unwrap(), GuideOpened::Web);
            assert_eq!(opened, vec![address]);
        }
    }

    #[test]
    fn an_unknown_executable_folder_opens_the_web_guide() {
        let recorder = Recorder::default();
        let result = open_guide_in(None, GuidePage::Guide, GuideLanguage::En, &recorder);
        assert_eq!(result.unwrap(), GuideOpened::Web);
        assert!(recorder.opened.borrow()[0].starts_with("https://github.com/"));
    }

    #[test]
    fn only_the_missing_page_falls_back_to_the_web() {
        let dir = distribution();
        fs::remove_file(dir.path().join("guide").join("guide.en.html")).unwrap();
        let (english, _) = open(dir.path(), GuidePage::Guide, GuideLanguage::En);
        assert_eq!(english.unwrap(), GuideOpened::Web);
        let (french, _) = open(dir.path(), GuidePage::Guide, GuideLanguage::Fr);
        assert_eq!(french.unwrap(), GuideOpened::Local);
    }

    #[test]
    fn a_page_that_is_a_folder_is_not_a_page() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("guide").join("guide.fr.html")).unwrap();
        let (result, opened) = open(dir.path(), GuidePage::Guide, GuideLanguage::Fr);
        assert_eq!(result.unwrap(), GuideOpened::Web);
        assert!(opened[0].ends_with("guide.fr.md"));
    }

    #[test]
    fn a_guide_that_is_a_file_is_not_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("guide"), "not a folder").unwrap();
        let (result, _) = open(dir.path(), GuidePage::Guide, GuideLanguage::Fr);
        assert_eq!(result.unwrap(), GuideOpened::Web);
    }

    /// Links need a privilege on Windows: the test is skipped where it is not granted.
    fn link_file(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(target, link);
        #[cfg(not(windows))]
        let made = std::os::unix::fs::symlink(target, link);
        made.is_ok()
    }

    #[test]
    fn a_link_to_a_file_outside_the_guide_folder_is_not_a_page() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("elsewhere.html");
        fs::write(&outside, "<html></html>").unwrap();
        fs::create_dir(dir.path().join("guide")).unwrap();
        if !link_file(&outside, &dir.path().join("guide").join("guide.fr.html")) {
            return;
        }
        let (result, opened) = open(dir.path(), GuidePage::Guide, GuideLanguage::Fr);
        assert_eq!(result.unwrap(), GuideOpened::Web);
        assert!(opened[0].starts_with("https://github.com/"));
    }

    #[test]
    fn a_failing_system_is_an_error_not_a_silent_web_page() {
        let dir = distribution();
        let recorder = Recorder {
            fail: true,
            ..Recorder::default()
        };
        let error = open_guide_in(
            Some(dir.path()),
            GuidePage::Guide,
            GuideLanguage::Fr,
            &recorder,
        )
        .unwrap_err();
        assert!(error.is_code(GUIDE_UNAVAILABLE));
        assert!(recorder.opened.borrow().is_empty());
    }

    #[test]
    fn the_interface_can_only_ask_for_the_known_pages_and_languages() {
        let page: GuidePage = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(page, GuidePage::Assistant);
        let language: GuideLanguage = serde_json::from_str("\"en\"").unwrap();
        assert_eq!(language, GuideLanguage::En);
        for text in [
            "\"..\\\\guide\"",
            "\"C:\\\\Windows\\\\notepad.exe\"",
            "\"auto\"",
            "\"\"",
        ] {
            assert!(serde_json::from_str::<GuidePage>(text).is_err(), "{text}");
            assert!(
                serde_json::from_str::<GuideLanguage>(text).is_err(),
                "{text}"
            );
        }
        assert_eq!(serde_json::to_string(&GuideOpened::Web).unwrap(), "\"web\"");
        assert_eq!(
            serde_json::to_string(&GuideOpened::Local).unwrap(),
            "\"local\""
        );
    }

    #[test]
    fn the_log_hook_records_what_would_be_opened() {
        let dir = distribution();
        let log = dir.path().join("opened.log");
        let opener = LogOpener(log.clone());
        open_guide_in(
            Some(dir.path()),
            GuidePage::Guide,
            GuideLanguage::Fr,
            &opener,
        )
        .unwrap();
        open_guide_in(None, GuidePage::Assistant, GuideLanguage::En, &opener).unwrap();
        let text = fs::read_to_string(log).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("guide.fr.html") && lines[0].contains(GUIDE_FOLDER));
        assert!(lines[1].ends_with("assistant.en.md") && lines[1].starts_with("https://"));
    }

    #[test]
    fn the_executable_folder_is_known_and_plain() {
        let folder = exe_folder().unwrap();
        assert!(folder.is_dir());
        assert!(!folder.to_string_lossy().starts_with(r"\\?\"));
    }
}
