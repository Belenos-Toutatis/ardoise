//! The presentation window (SPEC §4.2, classroom): full screen on the chosen screen (the
//! projector), no interface, showing the live session; the screens and the choice of one.
//!
//! Contract: docs/design/tablet-live-classroom.md (work package B), "Presentation window".
//! Screens are placed in physical pixels: with screens of different scale factors, logical
//! positions would be relative to the primary screen's scale (docs/research/windows-platform.md
//! §3). The window is created from an async command (SPEC §6.2: a window created from a
//! synchronous command can deadlock on Windows).

use serde::Serialize;
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, State, WebviewUrl,
    WebviewWindowBuilder, Window, WindowEvent,
};

use crate::AppContext;
use crate::commands::{CommandError, CommandResult, store, update};
use crate::live::LiveRuntime;

/// Label of the presentation window.
pub const PRESENTATION: &str = "presentation";
/// Label of the main (control) window, made in `lib.rs`.
const MAIN: &str = "main";
/// What the presentation window loads (`main.ts` mounts `Presentation.svelte` for it).
const PRESENTATION_URL: &str = "index.html?view=presentation";

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

/// Creates the presentation window on `monitor`, or moves it there, full screen.
fn show_on(app: &AppHandle, monitor: &MonitorInfo) -> tauri::Result<()> {
    let window = match app.get_webview_window(PRESENTATION) {
        Some(window) => {
            window.set_fullscreen(false)?;
            window
        }
        None => {
            let mut builder = WebviewWindowBuilder::new(
                app,
                PRESENTATION,
                WebviewUrl::App(PRESENTATION_URL.into()),
            )
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
            builder.build()?
        }
    };
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
/// window goes away the live session is written and the presentation window goes too, so that
/// the app ends.
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
            if let Some(presentation) = window.app_handle().get_webview_window(PRESENTATION) {
                let _ = presentation.destroy();
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

#[tauri::command]
pub async fn open_presentation(
    app: AppHandle,
    context: State<'_, AppContext>,
    runtime: State<'_, LiveRuntime>,
    monitor: Option<String>,
) -> CommandResult<MonitorInfo> {
    let saved = saved_projector(&context);
    let monitors = monitors(&app, saved.as_deref())?;
    let chosen = choose_monitor(&monitors, monitor.as_deref(), saved.as_deref())
        .ok_or_else(|| CommandError::new("noMonitor", "no screen is connected"))?;
    let chosen = MonitorInfo {
        chosen: true,
        ..chosen.clone()
    };
    show_on(&app, &chosen).map_err(unexpected)?;
    // The window is shown even if the settings cannot be written.
    if let Err(error) = save_projector(&context, &chosen.key) {
        eprintln!("presentation: cannot save the screen chosen: {error:?}");
    }
    runtime.hub().set_presentation(Some(chosen.clone()));
    Ok(chosen)
}

#[tauri::command]
pub async fn close_presentation(
    app: AppHandle,
    runtime: State<'_, LiveRuntime>,
) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window(PRESENTATION) {
        window.destroy().map_err(unexpected)?;
    }
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
