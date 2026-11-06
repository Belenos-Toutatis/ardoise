//! Ardoise desktop application: window setup, command-line modes and the commands the
//! interface calls.

// A release build without `custom-protocol` would load its interface from the dev server
// instead of embedding it: refuse to produce such a non-portable executable.
#[cfg(all(dev, not(debug_assertions)))]
compile_error!("release builds need `--features custom-protocol` (use `cargo xtask check`)");

mod about;
mod app_bridge;
mod autoexport;
mod background;
mod clipboard;
mod codec;
mod commands;
mod console;
mod demo;
mod diagnostic;
mod drafts;
mod drag;
#[cfg(windows)]
mod drag_windows;
mod export;
mod handwriting;
mod import;
mod instance;
mod library;
mod live;
mod mcp;
mod narration;
mod notes;
mod operations;
mod organize;
mod original;
mod pdf_pages;
mod presentation;
mod print;
#[cfg(windows)]
mod print_windows;
mod revision;
mod selftest;
mod session_end;
mod share;
mod tablet;
mod textdoc;
mod thumbnails;
mod video;
mod watcher;

use std::path::PathBuf;
use std::time::Duration;

use ardoise_core::settings::SettingsStore;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Longest wait, as the application exits, for the tablet's link to close (and its log to end
/// with the disconnection) and for a guided test under way to stop.
const EXIT_TABLET_WAIT: Duration = Duration::from_secs(3);

/// Command-line options. Normal launches have none.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cli {
    /// `--self-test`: load the interface in a hidden window, check the environment,
    /// print a report and exit with 0 (ok) or 1 (failure).
    pub self_test: bool,
    /// `--diagnostic`: open the guided tablet test right away.
    pub diagnostic: bool,
    /// With `--diagnostic`: run the test without a window against the simulated tablet,
    /// print the report and exit with 0 (no failure) or 1.
    pub simulated: bool,
    /// `--import-inkspace [LocalState] [--library DIR] [--compare]`: import Wacom Inkspace's
    /// notes without a window, print a table and exit with 0 (all imported) or 1.
    pub import_inkspace: Option<import::ImportArgs>,
    /// `--create-demo-library DIR`: make DIR a library of synthetic notes without a window,
    /// print what was created and exit (an empty path when DIR is missing).
    pub create_demo_library: Option<PathBuf>,
    /// `--create-demo-inkspace DIR`: write made-up Inkspace data into the new or empty folder
    /// DIR (a stand-in for Inkspace's `LocalState`) and exit (an empty path when DIR is missing).
    pub create_demo_inkspace: Option<PathBuf>,
    /// `--demo-notes N` with `--create-demo-library`: a library of N notes (the 8 demo notes, then
    /// copies of them), for the performance measure; 0 for the 8 demo notes alone.
    pub demo_notes: usize,
    /// `--simulated-tablet`: the simulated tablet replaces Bluetooth (real time, 3 stored pages);
    /// its registration is kept in a throwaway folder, never in the library.
    pub simulated_tablet: bool,
    /// `--mcp [--read-only]`: serve the MCP protocol on standard input/output, without a
    /// window, until stdin closes.
    pub mcp: Option<mcp::McpArgs>,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Cli {
        let mut cli = Cli::default();
        let mut import_inkspace = false;
        let mut import_args = import::ImportArgs::default();
        let mut mcp = false;
        let mut read_only = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--self-test" => cli.self_test = true,
                "--diagnostic" => cli.diagnostic = true,
                "--simulated" => cli.simulated = true,
                "--simulated-tablet" => cli.simulated_tablet = true,
                "--import-inkspace" => import_inkspace = true,
                "--mcp" => mcp = true,
                "--read-only" => read_only = true,
                "--compare" => import_args.compare = true,
                "--library" => import_args.library = args.next().map(PathBuf::from),
                "--create-demo-library" => {
                    cli.create_demo_library =
                        Some(args.next().map(PathBuf::from).unwrap_or_default());
                }
                "--create-demo-inkspace" => {
                    cli.create_demo_inkspace =
                        Some(args.next().map(PathBuf::from).unwrap_or_default());
                }
                "--demo-notes" => {
                    cli.demo_notes = args.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                }
                other if !other.starts_with("--") && import_args.local_state.is_none() => {
                    import_args.local_state = Some(PathBuf::from(other));
                }
                _ => {}
            }
        }
        if import_inkspace {
            cli.import_inkspace = Some(import_args);
        }
        if mcp {
            cli.mcp = Some(mcp::McpArgs { read_only });
        }
        cli
    }
}

/// Shared state of the running app.
pub struct AppContext {
    /// `Err` holds a description when the settings location could not be determined.
    pub store: Result<SettingsStore, String>,
    pub system_locale: Option<String>,
    pub self_test: Option<selftest::SelfTest>,
    /// Started with `--diagnostic`: the interface opens the tablet test.
    pub open_diagnostic: bool,
}

/// WebView2 keeps caches in this folder: always local to the PC, never next to a portable
/// exe that may sit in a synced folder.
fn webview_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(|dir| PathBuf::from(dir).join("Ardoise").join("webview"))
}

pub fn run() {
    let cli = Cli::parse(std::env::args().skip(1));
    let headless_diagnostic = cli.diagnostic && cli.simulated;
    // The MCP server first: its stdout carries the protocol and nothing else, so no other mode
    // runs next to it. Started by an assistant, the program has pipes for standard handles
    // (left alone); started by hand from a terminal, it borrows the terminal's console.
    if let Some(args) = cli.mcp {
        console::attach_parent_console();
        std::process::exit(mcp::run_cli(args));
    }
    if cli.self_test
        || headless_diagnostic
        || cli.import_inkspace.is_some()
        || cli.create_demo_library.is_some()
        || cli.create_demo_inkspace.is_some()
    {
        console::attach_parent_console();
    }

    if let Some(import_args) = &cli.import_inkspace {
        std::process::exit(import::run_cli(import_args));
    }
    if let Some(dir) = &cli.create_demo_library {
        std::process::exit(demo::run_cli(dir, cli.demo_notes));
    }
    if let Some(dir) = &cli.create_demo_inkspace {
        std::process::exit(import::cli::run_demo_cli(dir));
    }

    let context = AppContext {
        store: SettingsStore::open_default().map_err(|e| e.to_string()),
        system_locale: sys_locale::get_locale(),
        self_test: cli.self_test.then(selftest::SelfTest::start),
        open_diagnostic: cli.diagnostic,
    };

    // One windowed app per copy (M10, `instance.rs`): a second launch of this copy brings the
    // first one's window to the front and ends here.
    let instance = instance::guarded(&cli)
        .then(|| instance::names_for(&context))
        .flatten();
    let _instance_guard = match instance.as_ref().map(instance::claim) {
        Some(instance::Launch::AlreadyRunning) => std::process::exit(0),
        Some(instance::Launch::First(guard)) => guard,
        None => None,
    };
    let window_class = instance.map(|names| names.class);
    let drafts = drafts::DraftRuntime::new(&context);

    // Leftovers of the drags out of the app (PNG files for other programs), removed once old.
    drag::start_cleaning();

    // A library made before `.ardoise/` became `_ardoise/` is moved over once (best effort:
    // opening the library retries later).
    if let Some(library) = commands::current_state(&context).library() {
        let library = std::path::Path::new(library);
        let _ = ardoise_core::library::migrate_legacy_meta_dir(library);
        // Upkeep, off the start (a synced folder can be slow): SPEC §3.2, the trash keeps notes
        // 30 days; and the temporary files of a write that a crash (or a killed `--mcp`) cut
        // short, a day old at least, are removed. Best effort.
        let library = library.to_path_buf();
        let _ = std::thread::Builder::new()
            .name("ardoise-library-upkeep".into())
            .spawn(move || {
                if let Ok(opened) = ardoise_core::library::Library::open(&library) {
                    let _ = opened.purge_expired();
                }
                ardoise_core::fsutil::sweep_stale_temp_files(
                    &library,
                    std::time::Duration::from_secs(24 * 60 * 60),
                );
            });
    }

    if headless_diagnostic {
        std::process::exit(diagnostic::run_simulated_cli(&context));
    }

    // The interface embedded in the executable, also read by the share on the network (M11:
    // the viewer's files, by exact path, never through Tauri's resolver).
    let mut tauri_context = tauri::generate_context!();
    let share = share::ShareRuntime::default();
    share.set_assets(share::assets::share_context_assets(&mut tauri_context));

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(context)
        .manage(diagnostic::DiagnosticRuns::default())
        .manage(thumbnails::ThumbnailCache::default_location())
        .manage(watcher::LibraryWatch::default())
        .manage(export::ExportCancel::default())
        .manage(tablet::TabletRuntime::default())
        .manage(live::LiveRuntime::default())
        .manage(video::VideoCancel::default())
        .manage(narration::NarrationRuntime::default())
        .manage(codec::CodecDownload::default())
        .manage(handwriting::HandwritingRuntime::default())
        .manage(textdoc::Structures::default())
        .manage(app_bridge::BridgeRuntime::default())
        .manage(import::InkspaceImport::default())
        .manage(drafts)
        .manage(share)
        // The presentation window closing, and the main one taking it along.
        .on_window_event(presentation::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::app_state,
            commands::set_language,
            commands::set_theme,
            commands::inspect_library,
            commands::set_library,
            commands::self_test_report,
            diagnostic::diagnostic_start,
            diagnostic::diagnostic_answer,
            diagnostic::diagnostic_cancel,
            notes::note_outlines,
            library::library_list,
            thumbnails::note_thumbnail,
            library::create_note,
            library::create_binder,
            library::move_notes,
            library::trash_notes,
            library::trash_list,
            library::trash_restore,
            library::trash_purge,
            library::trash_empty,
            library::set_note_tags,
            library::set_note_favorite,
            library::retitle_note,
            organize::rename_binder,
            organize::delete_binder,
            organize::tag_colors,
            organize::set_tag_color,
            notes::open_note,
            notes::save_note,
            notes::render_background,
            original::note_original,
            revision::revision_source,
            revision::note_picture,
            tablet::tablet_info,
            tablet::forget_tablet,
            tablet::tablet_status,
            tablet::tablet_subscribe,
            tablet::tablet_register,
            tablet::tablet_cancel,
            tablet::tablet_sync,
            tablet::tablet_rename,
            tablet::tablet_set_time,
            tablet::tablet_set_options,
            tablet::tablet_set_orientation,
            tablet::sim_press_button,
            live::live_subscribe,
            live::live_start,
            live::live_resume,
            live::live_stop,
            live::live_set_style,
            live::live_command,
            live::live_restore_cleared,
            live::live_edit_strokes,
            live::live_undo_edit,
            live::live_redo_edit,
            live::live_page_background,
            live::live_set_view,
            live::live_set_marks,
            presentation::list_monitors,
            presentation::open_presentation,
            presentation::close_presentation,
            commands::set_auto_export,
            export::export_notes,
            export::cancel_export,
            clipboard::copy_note_image,
            print::print_notes,
            textdoc::export_structured_text,
            textdoc::structured_text_drawings,
            drag::prepare_note_drag,
            drag::start_note_drag,
            drag::note_drag_supported,
            operations::split_note,
            operations::merge_notes,
            video::estimate_video_export,
            video::export_video,
            video::cancel_video_export,
            narration::list_microphones,
            narration::set_microphone,
            narration::microphone_monitor,
            narration::microphone_monitor_stop,
            narration::narration_start,
            narration::narration_status,
            narration::narration_stop,
            narration::narration_retry,
            narration::narration_discard,
            narration::narration_info,
            narration::narration_audio,
            narration::delete_narration,
            codec::openh264_state,
            codec::openh264_download,
            codec::openh264_cancel_download,
            codec::openh264_remove,
            codec::set_openh264_allowed,
            background::inspect_background_file,
            background::stage_background_image,
            background::stage_note_background,
            background::release_background_images,
            background::create_notes_from_file,
            background::cancel_create_notes,
            background::background_image_names,
            handwriting::handwriting_info,
            handwriting::set_handwriting_language,
            handwriting::handwriting_status,
            handwriting::handwriting_search,
            handwriting::handwriting_stop,
            handwriting::handwriting_resume,
            handwriting::handwriting_text,
            handwriting::export_handwriting_text,
            handwriting::copy_handwriting_text,
            about::about_info,
            about::about_text,
            app_bridge::bridge_reply,
            app_bridge::mcp_launch_info,
            import::inkspace_detect,
            import::inkspace_inspect,
            import::inkspace_import,
            import::inkspace_cancel,
            drafts::draft_write,
            drafts::draft_find,
            drafts::draft_list,
            drafts::draft_discard,
            drafts::draft_stage_image,
            drafts::draft_restore_copy,
            session_end::set_unsaved_work,
            share::share_status,
            share::share_subscribe,
            share::share_start,
            share::share_port,
            share::set_share_port,
            share::share_stop,
            share::share_set_paused,
            share::share_show,
        ])
        .setup(move |app| {
            if cli.self_test {
                selftest::arm_timeout(app.handle().clone());
            }
            let mut window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("Ardoise")
                    .inner_size(1200.0, 800.0)
                    .min_inner_size(800.0, 560.0)
                    .center()
                    .visible(!cli.self_test);
            if let Some(dir) = webview_data_dir() {
                window = window.data_directory(dir);
            }
            // The class a second launch of this copy looks for (`instance.rs`).
            if let Some(class) = &window_class {
                window = window.window_classname(class);
            }
            window.build()?;
            // The end of the Windows session with unsaved notes (M10, `session_end.rs`).
            session_end::install(app.handle());
            // Handwriting recognition of the library, in the background.
            app.state::<handwriting::HandwritingRuntime>()
                .start(app.handle());
            // Live refresh of the library; a failure is logged, the app works without it.
            let library = commands::current_state(&app.state::<AppContext>())
                .library()
                .map(str::to_owned);
            app.state::<watcher::LibraryWatch>()
                .follow(app.handle(), library.as_deref());
            // The tablet: auto-connect, sync, live mode (a failure shows in its status).
            tablet::start_service(app.handle(), cli.simulated_tablet);
            // The assistant's bridge (MCP), for the windowed app only; logged when unavailable.
            if !cli.self_test {
                app_bridge::start(app.handle());
            }
            Ok(())
        })
        .build(tauri_context);

    let app = match result {
        Ok(app) => app,
        Err(error) => {
            if cli.self_test {
                println!("SELF-TEST FAILED: {error}");
            }
            eprintln!("Ardoise could not start: {error}");
            std::process::exit(1);
        }
    };
    // What `Builder::run` does, plus at exit the end of the share on the network and of the
    // tablet's link: the diagnostic logs of the connection (and of a guided test under way) end
    // with the disconnection.
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            // The share first: the port closes and every browser is told it ended.
            share::stop_at_exit(handle);
            handle
                .state::<diagnostic::DiagnosticRuns>()
                .cancel_within(EXIT_TABLET_WAIT);
            handle
                .state::<tablet::TabletRuntime>()
                .stop_within(EXIT_TABLET_WAIT);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::parse(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn parses_command_line_modes() {
        assert_eq!(parse(&[]), Cli::default());
        assert!(parse(&["--self-test"]).self_test);
        let diagnostic = parse(&["--diagnostic"]);
        assert!(diagnostic.diagnostic && !diagnostic.simulated);
        let simulated = parse(&["--diagnostic", "--simulated"]);
        assert!(simulated.diagnostic && simulated.simulated && !simulated.self_test);
        assert_eq!(parse(&["--unknown"]), Cli::default());
        let tablet = parse(&["--simulated-tablet"]);
        assert!(tablet.simulated_tablet && !tablet.simulated && !tablet.diagnostic);
        assert_eq!(parse(&["C:\\somewhere"]).import_inkspace, None);
    }

    #[test]
    fn parses_the_demo_library_mode() {
        let demo = parse(&["--create-demo-library", "D:\\Démo"]);
        assert_eq!(demo.create_demo_library, Some(PathBuf::from("D:\\Démo")));
        assert_eq!(demo.import_inkspace, None);
        assert_eq!(
            parse(&["--create-demo-library"]).create_demo_library,
            Some(PathBuf::new())
        );
        let inkspace = parse(&["--create-demo-inkspace", "D:\\Faux\\LocalState"]);
        assert_eq!(
            inkspace.create_demo_inkspace,
            Some(PathBuf::from("D:\\Faux\\LocalState"))
        );
        assert_eq!(inkspace.import_inkspace, None);
        assert_eq!(inkspace.create_demo_library, None);
        assert_eq!(demo.demo_notes, 0);
        let large = parse(&["--create-demo-library", "D:\\Démo", "--demo-notes", "1000"]);
        assert_eq!(large.demo_notes, 1000);
        assert_eq!(parse(&["--demo-notes", "x"]).demo_notes, 0);
    }

    #[test]
    fn parses_the_inkspace_import() {
        let default = parse(&["--import-inkspace"]);
        assert_eq!(default.import_inkspace, Some(import::ImportArgs::default()));
        let full = parse(&[
            "--import-inkspace",
            "D:\\copy\\LocalState",
            "--library",
            "E:\\Notes",
            "--compare",
        ]);
        assert_eq!(
            full.import_inkspace,
            Some(import::ImportArgs {
                local_state: Some(PathBuf::from("D:\\copy\\LocalState")),
                library: Some(PathBuf::from("E:\\Notes")),
                compare: true,
            })
        );
        let reordered = parse(&["--compare", "--import-inkspace", "X"]);
        let args = reordered.import_inkspace.unwrap();
        assert!(args.compare && args.local_state == Some(PathBuf::from("X")));
        assert!(!reordered.self_test && !reordered.diagnostic);
    }
}
