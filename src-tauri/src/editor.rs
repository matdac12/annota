//! The Editor window, pre-warmed hidden at startup (spec §6.1).

use tauri::window::Color;
use tauri::{App, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// How the Editor window is built. Pinned by a test: later slices address it by `label`.
struct Spec {
    label: &'static str,
    title: &'static str,
    visible: bool,
    resizable: bool,
    always_on_top: bool,
}

const SPEC: Spec = Spec {
    label: "editor",
    title: "Annota",
    visible: false,
    resizable: false,
    always_on_top: false,
};

/// The page's own background (`index.html`), so the window never flashes white.
const BACKGROUND: (u8, u8, u8) = (0x1c, 0x1c, 0x1e);

/// Placeholder size until the Editor places itself around each Region.
const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;

/// Creates the Editor hidden, with its page loaded.
pub fn build(app: &App) -> tauri::Result<()> {
    let (r, g, b) = BACKGROUND;
    let window = WebviewWindowBuilder::new(app, SPEC.label, WebviewUrl::App("index.html".into()))
        .title(SPEC.title)
        .inner_size(WIDTH, HEIGHT)
        .background_color(Color(r, g, b, 255))
        .resizable(SPEC.resizable)
        .always_on_top(SPEC.always_on_top)
        .visible(SPEC.visible)
        .build()?;
    // An invisible window alone does not stop WebView2 compositing (~15% of a core):
    // hide the webview too, and let WebView2 trim its memory while hidden. Whoever shows
    // the Editor must show the webview (`window.as_ref().show()`) and restore the memory
    // target before the first paint.
    let _ = window.as_ref().hide();
    set_memory_target_low(&window);
    Ok(())
}

#[cfg(windows)]
fn set_memory_target_low(window: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
    };
    use windows_core::Interface;
    let _ = window.with_webview(|webview| {
        // SAFETY: runs on the UI thread that owns this live controller. Older runtimes
        // without ICoreWebView2_19 are left at their default.
        unsafe {
            if let Ok(core) = webview.controller().CoreWebView2() {
                if let Ok(core) = core.cast::<ICoreWebView2_19>() {
                    let _ =
                        core.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
                }
            }
        }
    });
}

#[cfg(not(windows))]
fn set_memory_target_low(_window: &WebviewWindow) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_starts_hidden_as_a_plain_titled_window() {
        assert_eq!(SPEC.label, "editor");
        assert_eq!(SPEC.title, "Annota");
        assert!(!SPEC.visible);
        assert!(!SPEC.resizable);
        assert!(!SPEC.always_on_top);
    }

    #[test]
    fn the_capability_grants_the_editor_window_by_its_label() {
        let capability = include_str!("../capabilities/default.json");
        assert!(capability.contains(&format!("\"{}\"", SPEC.label)));
    }

    #[test]
    fn the_window_background_matches_the_page_so_the_first_paint_does_not_flash() {
        let page = include_str!("../../index.html");
        let (r, g, b) = BACKGROUND;
        assert!(page.contains(&format!("background: #{r:02x}{g:02x}{b:02x};")));
    }
}
