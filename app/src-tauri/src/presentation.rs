//! The windows full screen on the projector (SPEC §4.2, classroom; §4.4, the editor's projection):
//! live's presentation window, showing the live session, and the editor's projection window
//! (`projection.rs`), never both at once; the screens and the choice of one, shared by both.
//!
//! Contract: docs/design/tablet-live-classroom.md (work package B), "Presentation window";
//! docs/design/paper-projection-narration.md §2.3 (M17, the projection window).
//! Screens are placed in physical pixels: with screens of different scale factors, logical
//! positions would be relative to the primary screen's scale (docs/research/windows-platform.md
//! §3). The windows are created from async commands (SPEC §6.2: a window created from a
//! synchronous command can deadlock on Windows).

use serde::Serialize;
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

use crate::AppContext;
use crate::commands::{CommandError, CommandResult, store, update};
use crate::live::{Activity, LiveRuntime};
use crate::projection::{self, ProjectionRuntime};

/// Label of the presentation window.
pub const PRESENTATION: &str = "presentation";
/// Label of the editor's projection window.
pub const PROJECTION: &str = "projection";
/// Label of the main (control) window, made in `lib.rs`.
const MAIN: &str = "main";

/// A window full screen on the projector. Each one's opening destroys the other
/// (docs/design/paper-projection-narration.md, R1): they share the projector, the screen saved for
/// this PC and the flag that holds the handwriting indexer (`Activity`, one token at a time).
/// `capabilities/presentation.json` names both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenWindow {
    /// Live's presentation window (`Presentation.svelte`).
    Presentation,
    /// The editor's projection window (`EditorPresentation.svelte`, `projection.rs`).
    Projection,
}

impl ScreenWindow {
    pub const ALL: [ScreenWindow; 2] = [ScreenWindow::Presentation, ScreenWindow::Projection];

    pub fn label(self) -> &'static str {
        match self {
            ScreenWindow::Presentation => PRESENTATION,
            ScreenWindow::Projection => PROJECTION,
        }
    }

    /// What the window loads (`main.ts` mounts its root for the `view`).
    pub fn url(self) -> &'static str {
        match self {
            ScreenWindow::Presentation => "index.html?view=presentation",
            ScreenWindow::Projection => "index.html?view=projection",
        }
    }
}

/// A screen, as the interface lists them (`MonitorInfo` in `live.ts`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MonitorInfo {
    /// `name@x,y`: stays the same when other screens are unplugged.
    pub key: String,
    pub name: String,
    /// Physical pixels.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub primary: bool,
    /// The choice saved for this PC.
    pub chosen: bool,
}

/// The key of a screen: its name and the physical position of its top-left corner.
pub fn monitor_key(name: &str, x: i32, y: i32) -> String {
    format!("{name}@{x},{y}")
}

impl MonitorInfo {
    /// A screen; `chosen` when its key is `saved`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        scale: f64,
        primary: bool,
        saved: Option<&str>,
    ) -> MonitorInfo {
        let key = monitor_key(name, x, y);
        MonitorInfo {
            chosen: saved == Some(key.as_str()),
            key,
            name: name.to_owned(),
            x,
            y,
            width,
            height,
            scale,
            primary,
        }
    }
}

/// The screen of the presentation: the `requested` one if it is present, else the `saved`
/// one if present, else the first screen that is not the primary one, else the primary one;
/// `None` without any screen.
pub fn choose_monitor<'a>(
    monitors: &'a [MonitorInfo],
    requested: Option<&str>,
    saved: Option<&str>,
) -> Option<&'a MonitorInfo> {
    let by_key = |key: &str| monitors.iter().find(|m| m.key == key);
    requested
        .and_then(by_key)
        .or_else(|| saved.and_then(by_key))
        .or_else(|| monitors.iter().find(|m| !m.primary))
        .or_else(|| monitors.iter().find(|m| m.primary))
}

/// The screen saved for this PC (`[machines."PC"] projector`), if the settings can be read.
fn saved_projector(context: &AppContext) -> Option<String> {
    let store = store(context).ok()?;
    let settings = store.load().ok()?;
    settings.machines.get(store.machine())?.projector.clone()
}

/// Saves the choice of screen for this PC.
fn save_projector(context: &AppContext, key: &str) -> CommandResult<()> {
    let machine = store(context)?.machine().to_owned();
    update(context, |settings| {
        settings.machines.entry(machine).or_default().projector = Some(key.to_owned());
    })?;
    Ok(())
}

fn same_monitor(a: &Monitor, b: &Monitor) -> bool {
    a.name() == b.name() && a.position() == b.position() && a.size() == b.size()
}

/// The screens connected now.
fn monitors(app: &AppHandle, saved: Option<&str>) -> CommandResult<Vec<MonitorInfo>> {
    let all = app
        .available_monitors()
        .map_err(|error| CommandError::new("noMonitor", error))?;
    let primary = app.primary_monitor().ok().flatten();
    Ok(all
        .iter()
        .map(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            MonitorInfo::new(
                monitor.name().map_or("", String::as_str),
                position.x,
                position.y,
                size.width,
                size.height,
                monitor.scale_factor(),
                primary.as_ref().is_some_and(|p| same_monitor(p, monitor)),
                saved,
            )
        })
        .collect())
}

fn unexpected(error: tauri::Error) -> CommandError {
    CommandError::new("unexpected", error)
}

/// The screen a window full screen goes on: `requested` if it is connected, else the one saved
/// for this PC, else a secondary one, else the primary one (marked `chosen`); `noMonitor` without
/// any screen.
pub fn screen_for(
    app: &AppHandle,
    context: &AppContext,
    requested: Option<&str>,
) -> CommandResult<MonitorInfo> {
    let saved = saved_projector(context);
    let monitors = monitors(app, saved.as_deref())?;
    let chosen = choose_monitor(&monitors, requested, saved.as_deref())
        .ok_or_else(|| CommandError::new("noMonitor", "no screen is connected"))?;
    Ok(MonitorInfo {
        chosen: true,
        ..chosen.clone()
    })
}

/// Saves `key` as this PC's projector; the window is shown even if the settings cannot be
/// written.
pub fn remember_screen(context: &AppContext, key: &str) {
    if let Err(error) = save_projector(context, key) {
        eprintln!("presentation: cannot save the screen chosen: {error:?}");
    }
}

/// Destroys `which` if it is open. Its destruction is reported later, on the main thread
/// (`WindowEvent::Destroyed`).
pub fn destroy(app: &AppHandle, which: ScreenWindow) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(which.label()) {
        window.destroy()?;
    }
    Ok(())
}

/// What a window's owner hears when that very window is destroyed, however that happens.
pub type OnDestroyed = Box<dyn Fn() + Send + 'static>;

/// Creates the window `which` on `monitor`, or moves it there, full screen. `on_destroyed` is
/// called when the window shown this way is destroyed (also when it was moved: an older call's
/// is called too, and its owner tells them apart). A window created here that cannot be placed is
/// destroyed (it would stay hidden, and the indexing would wait for it).
pub fn show_on(
    app: &AppHandle,
    which: ScreenWindow,
    monitor: &MonitorInfo,
    activity: &Activity,
    on_destroyed: Option<OnDestroyed>,
) -> tauri::Result<()> {
    let window = match app.get_webview_window(which.label()) {
        Some(window) => {
            window.set_fullscreen(false)?;
            place(&window, monitor)?;
            if let Some(on_destroyed) = on_destroyed {
                window.on_window_event(move |event| {
                    if matches!(event, WindowEvent::Destroyed) {
                        on_destroyed();
                    }
                });
            }
            return Ok(());
        }
        None => {
            let mut builder =
                WebviewWindowBuilder::new(app, which.label(), WebviewUrl::App(which.url().into()))
                    .title("Ardoise")
                    .decorations(false)
                    .skip_taskbar(true)
                    // On Windows 11 the shadow of an undecorated window draws a 1 px border.
                    .shadow(false)
                    // The teacher keeps working in the control window.
                    .focused(false)
                    // Shown once in place, without a flash on the laptop's screen.
                    .visible(false);
            // Same WebView2 data folder as the main window (one WebView2 session).
            if let Some(dir) = crate::webview_data_dir() {
                builder = builder.data_directory(dir);
            }
            // Open from now until this very window is destroyed, however that happens.
            let token = activity.presentation_opened();
            let window = builder
                .build()
                .inspect_err(|_| activity.presentation_closed(token))?;
            let closed = activity.clone();
            window.on_window_event(move |event| {
                if matches!(event, WindowEvent::Destroyed) {
                    closed.presentation_closed(token);
                    if let Some(on_destroyed) = &on_destroyed {
                        on_destroyed();
                    }
                }
            });
            window
        }
    };
    place(&window, monitor).inspect_err(|_| {
        let _ = window.destroy();
    })
}

/// Puts `window` full screen on `monitor` and shows it.
fn place(window: &WebviewWindow, monitor: &MonitorInfo) -> tauri::Result<()> {
    window.set_position(PhysicalPosition::new(monitor.x, monitor.y))?;
    window.set_size(PhysicalSize::new(monitor.width, monitor.height))?;
    // Full screen on the monitor holding that physical point (not "the current one", which a
    // window straddling two screens of different scales could get wrong).
    window.set_fullscreen_on_monitor(PhysicalPosition::new(
        f64::from(monitor.x),
        f64::from(monitor.y),
    ))?;
    window.show()?;
    Ok(())
}

/// The windows' events that matter here (registered in `lib.rs`): when the presentation
/// window goes away (Esc, Alt+F4, `close_presentation`) every window learns it; when the main
/// window goes away the live session is written and both windows full screen go too, so that the
/// app ends. The projection window's destruction is told by its own handler (`show_on`'s
/// `on_destroyed`, numbered by `projection.rs`: a late report of an older window is ignored).
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if !matches!(event, WindowEvent::Destroyed) {
        return;
    }
    match window.label() {
        PRESENTATION => {
            if let Some(runtime) = window.try_state::<LiveRuntime>() {
                runtime.hub().presentation_closed(PRESENTATION);
            }
        }
        MAIN => {
            crate::live::main_window_closed(window.app_handle());
            for which in ScreenWindow::ALL {
                let _ = destroy(window.app_handle(), which);
            }
        }
        _ => {}
    }
}

#[tauri::command]
pub async fn list_monitors(
    app: AppHandle,
    context: State<'_, AppContext>,
) -> CommandResult<Vec<MonitorInfo>> {
    monitors(&app, saved_projector(&context).as_deref())
}

/// Opens the presentation window on `monitor` (else the saved screen, else a secondary one) and,
/// unless `remember` is `false`, saves that choice for this PC. The assistant's bridge passes
/// `false`: no bridge request changes a setting (docs/design/mcp.md). The editor's projection
/// window is closed first (one window full screen at a time), and the editor's watch told.
#[tauri::command]
pub async fn open_presentation(
    app: AppHandle,
    context: State<'_, AppContext>,
    runtime: State<'_, LiveRuntime>,
    projection: State<'_, ProjectionRuntime>,
    monitor: Option<String>,
    remember: Option<bool>,
) -> CommandResult<MonitorInfo> {
    let chosen = screen_for(&app, &context, monitor.as_deref())?;
    projection::close(&app, &projection)?;
    show_on(
        &app,
        ScreenWindow::Presentation,
        &chosen,
        &runtime.hub().activity(),
        None,
    )
    .map_err(unexpected)?;
    if remember != Some(false) {
        remember_screen(&context, &chosen.key);
    }
    runtime.hub().set_presentation(Some(chosen.clone()));
    Ok(chosen)
}

#[tauri::command]
pub async fn close_presentation(
    app: AppHandle,
    runtime: State<'_, LiveRuntime>,
) -> CommandResult<()> {
    destroy(&app, ScreenWindow::Presentation).map_err(unexpected)?;
    runtime.hub().presentation_closed(PRESENTATION);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::context;

    fn screens(saved: Option<&str>) -> Vec<MonitorInfo> {
        vec![
            MonitorInfo::new(r"\\.\DISPLAY1", 0, 0, 2880, 1800, 2.0, true, saved),
            MonitorInfo::new(r"\\.\DISPLAY2", 2880, 0, 1920, 1080, 1.0, false, saved),
            MonitorInfo::new(r"\\.\DISPLAY3", -1280, 200, 1280, 1024, 1.25, false, saved),
        ]
    }

    fn key(monitor: Option<&MonitorInfo>) -> Option<&str> {
        monitor.map(|m| m.key.as_str())
    }

    #[test]
    fn keys_are_the_name_and_the_physical_position() {
        assert_eq!(monitor_key("DELL P2419H", -1920, 0), "DELL P2419H@-1920,0");
        let all = screens(Some(r"\\.\DISPLAY2@2880,0"));
        assert_eq!(all[1].key, r"\\.\DISPLAY2@2880,0");
        assert!(all[1].chosen);
        assert!(!all[0].chosen && !all[2].chosen);
    }

    #[test]
    fn the_screen_is_the_requested_then_the_saved_then_a_secondary_then_the_primary() {
        let all = screens(None);
        let projector = r"\\.\DISPLAY3@-1280,200";
        let second = r"\\.\DISPLAY2@2880,0";
        assert_eq!(
            key(choose_monitor(&all, Some(projector), Some(second))),
            Some(projector)
        );
        // A requested screen that was unplugged: the saved one.
        assert_eq!(
            key(choose_monitor(&all, Some("gone@0,0"), Some(projector))),
            Some(projector)
        );
        // The saved one unplugged too: the first screen that is not the primary one.
        assert_eq!(
            key(choose_monitor(&all, None, Some("gone@0,0"))),
            Some(second)
        );
        assert_eq!(key(choose_monitor(&all, None, None)), Some(second));
        // The laptop alone: its screen.
        let alone = &all[..1];
        assert_eq!(
            key(choose_monitor(alone, None, Some(second))),
            Some(r"\\.\DISPLAY1@0,0")
        );
        // The primary screen can be asked for.
        assert_eq!(
            key(choose_monitor(&all, Some(r"\\.\DISPLAY1@0,0"), None)),
            Some(r"\\.\DISPLAY1@0,0")
        );
        // No primary reported: the first screen.
        let mut unknown = screens(None);
        unknown.iter_mut().for_each(|m| m.primary = false);
        assert_eq!(
            key(choose_monitor(&unknown, None, None)),
            Some(r"\\.\DISPLAY1@0,0")
        );
        assert_eq!(choose_monitor(&[], Some(projector), Some(projector)), None);
    }

    #[test]
    fn monitor_info_has_the_shape_of_the_interface() {
        let info = MonitorInfo::new(
            "HDMI",
            1920,
            -10,
            1280,
            720,
            1.5,
            false,
            Some("HDMI@1920,-10"),
        );
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({
                "key": "HDMI@1920,-10",
                "name": "HDMI",
                "x": 1920,
                "y": -10,
                "width": 1280,
                "height": 720,
                "scale": 1.5,
                "primary": false,
                "chosen": true,
            })
        );
    }

    #[test]
    fn the_two_windows_have_their_own_label_and_view() {
        assert_eq!(ScreenWindow::Presentation.label(), "presentation");
        assert_eq!(
            ScreenWindow::Presentation.url(),
            "index.html?view=presentation"
        );
        assert_eq!(ScreenWindow::Projection.label(), "projection");
        assert_eq!(ScreenWindow::Projection.url(), "index.html?view=projection");
        // Neither is the main window, and each names its own view.
        for which in ScreenWindow::ALL {
            assert_ne!(which.label(), MAIN);
            assert!(which.url().ends_with(&format!("?view={}", which.label())));
        }
    }

    #[test]
    fn the_capability_of_the_windows_names_both() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/presentation.json")).unwrap();
        let windows: Vec<&str> = capability["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w.as_str().unwrap())
            .collect();
        let labels: Vec<&str> = ScreenWindow::ALL.iter().map(|w| w.label()).collect();
        assert_eq!(windows, labels);
    }

    #[test]
    fn the_choice_is_saved_per_computer() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path());
        assert_eq!(saved_projector(&context), None);
        save_projector(&context, r"\\.\DISPLAY2@2880,0").unwrap();
        assert_eq!(
            saved_projector(&context).as_deref(),
            Some(r"\\.\DISPLAY2@2880,0")
        );
        // In this PC's section, next to its library.
        let text = std::fs::read_to_string(settings.path().join("ardoise.toml")).unwrap();
        let section = text.find("PC-A").unwrap();
        assert!(text[section..].contains("projector = "), "{text}");
    }
}
