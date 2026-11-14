//! WebView2's browser accelerator keys, switched off in every window of a production build.
//!
//! WebView2 behaves like a browser window unless told otherwise: F5 and Ctrl+R reload the page (and drop an
//! unsaved note), Ctrl+P and Ctrl+Shift+P print the window like a web page (the toolbar on paper), Ctrl+F and F3
//! open a find bar, Ctrl+U, Ctrl+J, Ctrl+S and Ctrl+O reach pages and dialogs of a browser, F7 starts caret
//! browsing, F12 opens the developer tools. Ardoise has its own print (Ctrl+P, `lib/browser-guard.ts` and the
//! screens) and none of the others make sense in it.
//!
//! `AreBrowserAcceleratorKeysEnabled = false` turns them all off at the source, whichever element has the focus and
//! even before the page's script has run; the page still receives every key (its own shortcuts, the editing keys
//! of the fields, the remote debugging port the journeys drive are untouched). The page's own guard
//! (`lib/browser-guard.ts`) does the same job with `preventDefault` for the cases the setting does not reach (a
//! WebView2 runtime older than 92.0.902.0 ignores it) and for the browser.
//!
//! Not applied to a development build (`tauri dev`), where F5 and F12 are the developer's tools.

use tauri::WebviewWindow;

/// Turns the browser accelerator keys off in `window`. A failure is logged and the window goes on without (the
/// page's guard still works).
pub fn disable_browser_keys(window: &WebviewWindow) {
    if tauri::is_dev() {
        return;
    }
    #[cfg(windows)]
    {
        let label = window.label().to_owned();
        let result = window.with_webview(move |webview| {
            if let Err(error) = unsafe { turn_off(&webview.controller()) } {
                eprintln!("could not turn the browser keys off in window {label}: {error}");
            }
        });
        if let Err(error) = result {
            eprintln!(
                "could not reach the webview of window {}: {error}",
                window.label()
            );
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// The setting itself, on the webview behind `controller`.
#[cfg(windows)]
unsafe fn turn_off(
    controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
) -> windows::core::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows::core::Interface;

    let settings = unsafe { controller.CoreWebView2()?.Settings()? };
    let settings = settings.cast::<ICoreWebView2Settings3>()?;
    unsafe { settings.SetAreBrowserAcceleratorKeysEnabled(false) }
}
