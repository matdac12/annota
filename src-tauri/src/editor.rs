//! The Editor window, pre-warmed hidden at startup (spec §6.1) and placed around each Region (§6.2).
//!
//! Hidden means the window *and* its webview (a hidden window alone keeps compositing at 60 fps) with
//! WebView2's memory target Low. For a Snap the webview is made visible again while the window stays
//! hidden, so the page can paint the Region; only [`reveal`] shows the window.

use tauri::window::Color;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::capture::Region;
use crate::commands::Handoff;
use crate::geometry::{self, Chrome, Rect};
use crate::snap;

/// The Editor's window label; the capability and the Resident's events address it by this.
pub const LABEL: &str = "editor";

/// How the Editor window is built. Pinned by a test: later slices address it by `label`.
struct Spec {
    label: &'static str,
    title: &'static str,
    visible: bool,
    resizable: bool,
    always_on_top: bool,
}

const SPEC: Spec = Spec {
    label: LABEL,
    title: "Annota",
    visible: false,
    resizable: false,
    always_on_top: false,
};

/// The page's own background (`index.html`), so the window never flashes white.
const BACKGROUND: (u8, u8, u8) = (0x2a, 0x2a, 0x2f);

/// The hint line's height in CSS pixels (`--hint-height` in `index.html`); a test pins them together.
const HINT_CSS_HEIGHT: i32 = 28;

/// The narrowest client area, in CSS pixels, that shows the whole hint line.
const HINT_MIN_CSS_WIDTH: i32 = 300;

/// Placeholder size until [`open`] places the window around the first Region.
const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;

/// The Editor window, if it exists.
fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(SPEC.label)
}

/// Creates the Editor hidden, with its page loaded. Its title-bar ✕ discards the Snap and never destroys
/// the window: the Editor is pre-warmed once and lives as long as the Resident.
pub fn build(app: &tauri::App) -> tauri::Result<()> {
    let (r, g, b) = BACKGROUND;
    let window = WebviewWindowBuilder::new(app, SPEC.label, WebviewUrl::App("index.html".into()))
        .title(SPEC.title)
        .inner_size(WIDTH, HEIGHT)
        .background_color(Color(r, g, b, 255))
        .resizable(SPEC.resizable)
        .always_on_top(SPEC.always_on_top)
        .visible(SPEC.visible)
        .build()?;
    let handle = app.handle().clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            snap::discard(&handle);
        }
    });
    hide_webview(&window);
    Ok(())
}

/// Puts `region` in the Editor, still hidden: brings the webview back, places the window around the image,
/// and hands the pixels to the page. The page paints and calls `editor_painted`, which shows the window
/// ([`reveal`]); the returned token names this Snap's wait for that paint. Runs on the main thread.
pub fn open(app: &AppHandle, region: Region) -> Result<u64, String> {
    let window = window(app).ok_or("the Editor window does not exist")?;
    // Visible webview, hidden window: the page can paint (and run `requestAnimationFrame`) without
    // anything on screen. The memory target goes back to Normal before that first paint.
    show_webview(&window);
    place(&window, &region)?;
    let (width, height) = (region.width() as u32, region.height() as u32);
    let handoff = app.state::<Handoff>();
    let token = handoff.expect_paint();
    handoff.deliver(geometry::with_dimensions(region.rgba, width, height));
    Ok(token)
}

/// Centres the hidden window on the Region's monitor: `set_position`, then `set_size` (a cross-DPI move
/// fires `WM_DPICHANGED`, which would overwrite an earlier size), then the position again, centred on the
/// outer size the window really ended up with (Windows enforces a minimum, and the frame is only known at
/// the new scale).
fn place(window: &WebviewWindow, region: &Region) -> Result<(), String> {
    let (work, dpi) = monitor_work_area_and_dpi(region.monitor).ok_or("cannot read the capture monitor")?;
    window
        .set_position(PhysicalPosition::new(work.left, work.top))
        .map_err(|e| e.to_string())?;
    let image = (region.screen.width(), region.screen.height());
    let placement = geometry::place_editor(image, work, chrome_of(window, dpi)?);
    window
        .set_size(PhysicalSize::new(placement.inner.0 as u32, placement.inner.1 as u32))
        .map_err(|e| e.to_string())?;
    let outer = window.outer_size().map_err(|e| e.to_string())?;
    let position = geometry::centred(work, (outer.width as i32, outer.height as i32));
    window.set_position(PhysicalPosition::new(position.0, position.1)).map_err(|e| e.to_string())
}

/// Frame (outer minus inner) and hint line in physical pixels at the window's current scale.
fn chrome_of(window: &WebviewWindow, dpi: u32) -> Result<Chrome, String> {
    let outer = window.outer_size().map_err(|e| e.to_string())?;
    let inner = window.inner_size().map_err(|e| e.to_string())?;
    Ok(Chrome {
        frame: ((outer.width as i32 - inner.width as i32).max(0), (outer.height as i32 - inner.height as i32).max(0)),
        hint: physical(HINT_CSS_HEIGHT, dpi),
        min_inner_width: physical(HINT_MIN_CSS_WIDTH, dpi),
    })
}

/// `css` pixels at `dpi`, rounded.
fn physical(css: i32, dpi: u32) -> i32 {
    (css * dpi as i32 + 48) / 96
}

/// The page has painted the Region: show the window and take the foreground. Never tao's `set_focus`,
/// which injects a synthetic Alt into the user's app.
pub fn reveal(app: &AppHandle) {
    let Some(window) = window(app) else { return };
    let _ = window.show();
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
        if let Ok(hwnd) = window.hwnd() {
            // SAFETY: a live top-level window handle of this process.
            let _ = unsafe { SetForegroundWindow(HWND(hwnd.0)) };
        }
    }
    // Keyboard focus goes to the page, so Ctrl+C and Esc reach it.
    let _ = window.as_ref().set_focus();
}

/// Hides the Editor, window and webview, and lets WebView2 trim its memory again.
pub fn hide(app: &AppHandle) {
    let Some(window) = window(app) else { return };
    let _ = window.hide();
    hide_webview(&window);
}

fn show_webview(window: &WebviewWindow) {
    let _ = window.as_ref().show();
    set_memory_target(window, MemoryTarget::Normal);
}

fn hide_webview(window: &WebviewWindow) {
    // An invisible window alone does not stop WebView2 compositing (~15% of a core): hide the webview
    // too, and let WebView2 trim its memory while hidden.
    let _ = window.as_ref().hide();
    set_memory_target(window, MemoryTarget::Low);
}

#[derive(Clone, Copy)]
enum MemoryTarget {
    Low,
    Normal,
}

#[cfg(windows)]
fn set_memory_target(window: &WebviewWindow, target: MemoryTarget) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows_core::Interface;
    let level = match target {
        MemoryTarget::Low => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        MemoryTarget::Normal => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    let _ = window.with_webview(move |webview| {
        // SAFETY: runs on the UI thread that owns this live controller. Older runtimes
        // without ICoreWebView2_19 are left at their default.
        unsafe {
            if let Ok(core) = webview.controller().CoreWebView2() {
                if let Ok(core) = core.cast::<ICoreWebView2_19>() {
                    let _ = core.SetMemoryUsageTargetLevel(level);
                }
            }
        }
    });
}

#[cfg(not(windows))]
fn set_memory_target(_window: &WebviewWindow, _target: MemoryTarget) {}

/// The work area (physical, virtual-desktop coordinates) and effective DPI of the monitor `monitor` is.
#[cfg(windows)]
fn monitor_work_area_and_dpi(monitor: Rect) -> Option<(Rect, u32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    let centre = POINT { x: monitor.left + monitor.width() / 2, y: monitor.top + monitor.height() / 2 };
    // SAFETY: plain queries on a monitor handle `MonitorFromPoint` just returned; `info` has its size set.
    unsafe {
        let handle = MonitorFromPoint(centre, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(handle, &mut info).as_bool() {
            return None;
        }
        let (mut x, mut y) = (96, 96);
        let _ = GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        let w = info.rcWork;
        Some((Rect { left: w.left, top: w.top, right: w.right, bottom: w.bottom }, x))
    }
}

#[cfg(not(windows))]
fn monitor_work_area_and_dpi(_monitor: Rect) -> Option<(Rect, u32)> {
    None
}

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

    #[test]
    fn the_hint_height_placement_assumes_is_the_one_the_page_lays_out() {
        let page = include_str!("../../index.html");
        assert!(page.contains(&format!("--hint-height: {HINT_CSS_HEIGHT}px;")));
    }
}
