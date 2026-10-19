//! The selection Overlay (spec §5): every monitor's frozen frame, dimmed, with a Region dragged out on one.
//!
//! Plain Win32, not WebView: one `WS_POPUP` per monitor, created on Tauri's main thread at the monitor's
//! physical rect and never moved, so it never sees `WM_DPICHANGED`. Tao's message loop pumps these windows
//! like its own. Each paints its monitor's DIB 1:1, composing every frame in a back buffer. Chrome (the
//! label's font and padding) is sized from `GetDpiForWindow`.
//!
//! All state is one [`Session`] in a thread-local: the window procedures only ever run on the main thread.
//! `finish` takes the session out *before* tearing the windows down, so the messages that teardown sends
//! find no session and fall through to `DefWindowProcW`.

use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    GetTextExtentPoint32W, IntersectClipRect, InvalidateRect, PatBlt, SelectClipRgn, SelectObject, SetBkMode,
    SetTextColor, UpdateWindow, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DEFAULT_PITCH,
    DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
    SRCCOPY, TRANSPARENT, WHITENESS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, ReleaseCapture, SetCapture, UnregisterHotKey, MOD_NOREPEAT, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, LoadCursorW, RegisterClassExW, SetCursor,
    SetForegroundWindow, ShowWindow, IDC_CROSS, SW_SHOW, SW_SHOWNOACTIVATE, WM_CAPTURECHANGED,
    WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

use crate::capture::{Dib, MonitorCapture, Region};
use crate::geometry::{self, Rect};
use crate::timings;

const CLASS: PCWSTR = w!("AnnotaOverlay");
const HOTKEY_ID: i32 = 1;

/// The frozen frame is dimmed to this much of its brightness (about 30 % black, spec §5.2).
const DIM_KEEP_PERCENT: u32 = 70;

/// Chrome sizes at 96 dpi, scaled by the window's DPI.
const LABEL_FONT_PX: i32 = 14;
const LABEL_GAP_PX: i32 = 16;
const LABEL_PAD_X_PX: i32 = 8;
const LABEL_PAD_Y_PX: i32 = 4;
/// The label's fill, as `0x00BBGGRR`: a near-black.
const LABEL_FILL: COLORREF = COLORREF(0x001e_1c1c);
const LABEL_TEXT: COLORREF = COLORREF(0x00ff_ffff);

/// How a selection ended.
pub enum Outcome {
    Cancelled,
    Selected(Region),
}

type Done = Box<dyn FnOnce(Outcome)>;

/// Sizes and the font of one monitor's chrome.
struct Chrome {
    font: HFONT,
    gap: i32,
    pad_x: i32,
    pad_y: i32,
}

impl Chrome {
    fn new(dpi: u32) -> Chrome {
        let scale = |px: i32| (px * dpi as i32 + 48) / 96;
        // SAFETY: a plain font creation; the font is deleted in `Drop`.
        let font = unsafe {
            CreateFontW(
                -scale(LABEL_FONT_PX),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PITCH.0 as u32,
                w!("Segoe UI"),
            )
        };
        Chrome { font, gap: scale(LABEL_GAP_PX), pad_x: scale(LABEL_PAD_X_PX), pad_y: scale(LABEL_PAD_Y_PX) }
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        // SAFETY: the font is never left selected into a DC (`label_size` and `draw_label` restore it).
        unsafe {
            let _ = DeleteObject(self.font.into());
        }
    }
}

/// One monitor's Overlay window and the pixels behind it.
struct View {
    hwnd: HWND,
    capture: MonitorCapture,
    dimmed: Dib,
    back: Dib,
    chrome: Chrome,
}

impl Drop for View {
    fn drop(&mut self) {
        // SAFETY: this window was made here, on this thread. Dropping a session is how its windows close.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// A drag in progress, in the local coordinates of `views[view]`.
struct Drag {
    view: usize,
    anchor: (i32, i32),
    region: Rect,
    label: Rect,
    /// Everything the chrome covers now (Region, its border, label): what the next paint must redo.
    dirty: Rect,
}

struct Session {
    views: Vec<View>,
    drag: Option<Drag>,
    /// A right-button press went to an Overlay; only its release cancels (not one that began elsewhere).
    right_pressed: bool,
    hotkey_window: HWND,
    done: Option<Done>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// Runs `f` on the session, unless there is none or it is already borrowed (a message sent from inside a
/// handler): then the message is left to `DefWindowProcW`.
fn with_session<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSION.with(|session| session.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

/// Puts the Overlay up over `captures` and returns; `done` runs later, on this thread, when the selection
/// ends. Must be called on Tauri's main thread, whose message loop drives the windows.
pub fn show(captures: Vec<MonitorCapture>, done: Done) -> Result<(), String> {
    if SESSION.with(|s| s.try_borrow().map_or(true, |s| s.is_some())) {
        return Err("an Overlay is already up".into());
    }
    register_class()?;
    let views = captures.into_iter().map(make_view).collect::<Result<Vec<_>, _>>()?;
    let under_cursor = view_under_cursor(&views);
    let windows: Vec<(HWND, bool)> = views.iter().enumerate().map(|(i, v)| (v.hwnd, i == under_cursor)).collect();
    let hotkey_window = views[under_cursor].hwnd;
    SESSION.with(|s| *s.borrow_mut() = Some(Session { views, drag: None, right_pressed: false, hotkey_window, done: Some(done) }));

    // SAFETY: handles of windows this thread just made. No session borrow is held: these calls send
    // messages (`WM_PAINT`, ...) straight into `wndproc`.
    unsafe {
        // Esc cancels even if Windows refuses us the foreground, and from the first frame. A taken
        // hotkey just leaves `WM_KEYDOWN`.
        if let Err(e) = RegisterHotKey(Some(hotkey_window), HOTKEY_ID, MOD_NOREPEAT, VK_ESCAPE.0 as u32) {
            eprintln!("overlay: could not register Esc: {e}");
        }
        for &(hwnd, focused) in &windows {
            // Paint first, then show; a hidden window has nothing to update, so paint again once it is up.
            let _ = UpdateWindow(hwnd);
            let _ = ShowWindow(hwnd, if focused { SW_SHOW } else { SW_SHOWNOACTIVATE });
            let _ = UpdateWindow(hwnd);
        }
        // Never tao's `set_focus`: its fallback injects an Alt into the user's app.
        let _ = SetForegroundWindow(hotkey_window);
    }
    timings::mark("overlay_up");
    Ok(())
}

fn register_class() -> Result<(), String> {
    // SAFETY: the class lives for the process; registering it twice just fails with ERROR_CLASS_ALREADY_EXISTS.
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
    }
    Ok(())
}

/// The window, dimmed copy and back buffer of one monitor.
fn make_view(capture: MonitorCapture) -> Result<View, String> {
    let r = capture.rect;
    // The fallible allocations come first: once the window exists, the `View` below owns it.
    let dimmed = dimmed_copy(&capture.frame)?;
    let back = Dib::new(r.width(), r.height())?;
    // SAFETY: creates a window on this thread, directly at the monitor's physical rect.
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS,
            w!(""),
            WS_POPUP,
            r.left,
            r.top,
            r.width(),
            r.height(),
            None,
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|e| format!("CreateWindowExW at {r:?}: {e}"))?
    };
    let view = View {
        hwnd,
        dimmed,
        back,
        // SAFETY: `hwnd` is the window just made.
        chrome: Chrome::new(unsafe { GetDpiForWindow(hwnd) }.max(96)),
        capture,
    };
    // SAFETY: both DCs are live, and the same size.
    unsafe {
        let _ = BitBlt(view.back.dc(), 0, 0, r.width(), r.height(), Some(view.dimmed.dc()), 0, 0, SRCCOPY);
        // No Windows 11 rounded corners, no show transition.
        let round = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            view.hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const round).cast(),
            size_of_val(&round) as u32,
        );
        let no_transition = 1i32;
        let _ = DwmSetWindowAttribute(
            view.hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&raw const no_transition).cast(),
            size_of_val(&no_transition) as u32,
        );
    }
    Ok(view)
}

/// The frame at [`DIM_KEEP_PERCENT`] of its brightness. Alpha is ignored by every blit.
fn dimmed_copy(frame: &Dib) -> Result<Dib, String> {
    let mut dimmed = Dib::new(frame.width, frame.height)?;
    let table = geometry::dim_table(DIM_KEEP_PERCENT);
    for (out, value) in dimmed.bytes_mut().iter_mut().zip(frame.bytes()) {
        *out = table[*value as usize];
    }
    Ok(dimmed)
}

/// The monitor the pointer is on, or the first one.
fn view_under_cursor(views: &[View]) -> usize {
    let mut at = POINT::default();
    // SAFETY: `at` is a valid POINT.
    if unsafe { GetCursorPos(&mut at) }.is_err() {
        return 0;
    }
    views
        .iter()
        .position(|v| {
            let r = v.capture.rect;
            at.x >= r.left && at.x < r.right && at.y >= r.top && at.y < r.bottom
        })
        .unwrap_or(0)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_SETCURSOR => {
            // SAFETY: stock cursor.
            unsafe {
                if let Ok(cursor) = LoadCursorW(None, IDC_CROSS) {
                    SetCursor(Some(cursor));
                }
            }
            return LRESULT(1);
        }
        WM_PAINT => {
            paint(hwnd);
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            begin_drag(hwnd, pointer(lparam));
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            move_drag(hwnd, pointer(lparam));
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            timings::mark("mouse_up");
            end_drag(hwnd);
            return LRESULT(0);
        }
        // The press is swallowed, and its release cancels: the click never reaches the app underneath.
        WM_RBUTTONDOWN => {
            with_session(|session| session.right_pressed = true);
            return LRESULT(0);
        }
        WM_RBUTTONUP => {
            if with_session(|session| session.right_pressed) == Some(true) {
                finish(false);
            }
            return LRESULT(0);
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
            finish(false);
            return LRESULT(0);
        }
        WM_HOTKEY if wparam.0 == HOTKEY_ID as usize => {
            finish(false);
            return LRESULT(0);
        }
        // The monitors changed under the frozen frames: they no longer match the desktop.
        WM_DISPLAYCHANGE => {
            finish(false);
            return LRESULT(0);
        }
        WM_CAPTURECHANGED => {
            abandon_drag();
            return LRESULT(0);
        }
        _ => {}
    }
    // SAFETY: forwarding the message this procedure received.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The pointer in the window's client coordinates, which are the monitor's local physical pixels. Signed
/// because a captured pointer can be left of or above the window.
fn pointer(lparam: LPARAM) -> (i32, i32) {
    (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32)
}

fn begin_drag(hwnd: HWND, at: (i32, i32)) {
    let started = with_session(|session| {
        if session.drag.is_some() {
            return false;
        }
        let Some(view) = session.views.iter().position(|v| v.hwnd == hwnd) else { return false };
        let (w, h) = (session.views[view].back.width, session.views[view].back.height);
        let region = geometry::drag_rect(at, at, w, h);
        session.drag = Some(Drag { view, anchor: at, region, label: region, dirty: region });
        update_chrome(session, at);
        true
    });
    if started == Some(true) {
        // SAFETY: capture follows the pointer out of the window, so a drag past the edge keeps arriving.
        unsafe {
            SetCapture(hwnd);
        }
    }
}

fn move_drag(hwnd: HWND, at: (i32, i32)) {
    with_session(|session| {
        if session.drag.as_ref().is_some_and(|d| session.views[d.view].hwnd == hwnd) {
            update_chrome(session, at);
        }
    });
}

/// Recomputes the Region and label for a pointer at `at`, and invalidates what changed.
fn update_chrome(session: &mut Session, at: (i32, i32)) {
    let Some(drag) = session.drag.as_mut() else { return };
    let view = &session.views[drag.view];
    let (w, h) = (view.back.width, view.back.height);
    let region = geometry::drag_rect(drag.anchor, at, w, h);
    let cursor = (at.0.clamp(0, w - 1), at.1.clamp(0, h - 1));
    let size = label_size(view, &geometry::size_text(region));
    let origin = geometry::label_origin(cursor, size, view.chrome.gap, w, h);
    let label = Rect { left: origin.0, top: origin.1, right: origin.0 + size.0, bottom: origin.1 + size.1 };
    let dirty = union(inflate(region, 1), label);
    invalidate(view.hwnd, union(drag.dirty, dirty));
    (drag.region, drag.label, drag.dirty) = (region, label, dirty);
}

fn end_drag(hwnd: HWND) {
    // `Some(true)`: a Region worth keeping. `Some(false)`: a click or a slip, ignored.
    let selected = with_session(|session| {
        let drag = session.drag.as_ref().filter(|d| session.views[d.view].hwnd == hwnd)?;
        if geometry::is_selectable(drag.region) {
            return Some(true);
        }
        invalidate(hwnd, drag.dirty);
        session.drag = None;
        Some(false)
    })
    .flatten();
    match selected {
        Some(true) => finish(true),
        // SAFETY: ends the capture `begin_drag` took; `drag` is already clear, so the resulting
        // `WM_CAPTURECHANGED` has nothing to abandon.
        Some(false) => unsafe {
            let _ = ReleaseCapture();
        },
        None => {}
    }
}

/// The pointer was taken from us mid-drag (another window grabbed it): drop the half-made Region.
fn abandon_drag() {
    with_session(|session| {
        if let Some(drag) = session.drag.take() {
            invalidate(session.views[drag.view].hwnd, drag.dirty);
        }
    });
}

/// Ends the selection: with the dragged Region if `select`, else cancelled. Closes every Overlay first,
/// then runs the callback.
fn finish(select: bool) {
    let Some(mut session) = SESSION.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.take())) else {
        return;
    };
    let outcome = select
        .then_some(session.drag.as_ref())
        .flatten()
        .and_then(|d| session.views[d.view].capture.crop(d.region))
        .map_or(Outcome::Cancelled, Outcome::Selected);
    let done = session.done.take();
    // SAFETY: undoes `show`'s hotkey and `begin_drag`'s capture. Dropping the session closes the windows.
    unsafe {
        let _ = UnregisterHotKey(Some(session.hotkey_window), HOTKEY_ID);
        let _ = ReleaseCapture();
    }
    drop(session);
    if let Some(done) = done {
        done(outcome);
    }
}

fn paint(hwnd: HWND) {
    // SAFETY: the BeginPaint/EndPaint pair brackets the drawing; the GDI calls in between use DCs the
    // session owns.
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        with_session(|session| {
            let Some(index) = session.views.iter().position(|v| v.hwnd == hwnd) else { return };
            let view = &session.views[index];
            let clip = from_rect(ps.rcPaint);
            if clip.width() <= 0 || clip.height() <= 0 {
                return;
            }
            let drag = session.drag.as_ref().filter(|d| d.view == index);
            compose(view, drag, clip);
            let _ = BitBlt(dc, clip.left, clip.top, clip.width(), clip.height(), Some(view.back.dc()), clip.left, clip.top, SRCCOPY);
        });
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Redraws `clip` of the back buffer: the dimmed frame, then (if dragging) the undimmed Region, its 1 px
/// white border just outside it, and the size label.
///
/// # Safety
/// The view's DCs must be live, which they are while the session is.
unsafe fn compose(view: &View, drag: Option<&Drag>, clip: Rect) {
    let back = view.back.dc();
    // SAFETY: per the contract above.
    unsafe {
        IntersectClipRect(back, clip.left, clip.top, clip.right, clip.bottom);
        let _ = BitBlt(back, clip.left, clip.top, clip.width(), clip.height(), Some(view.dimmed.dc()), clip.left, clip.top, SRCCOPY);
        if let Some(drag) = drag {
            let r = drag.region;
            let _ = BitBlt(back, r.left, r.top, r.width(), r.height(), Some(view.capture.frame.dc()), r.left, r.top, SRCCOPY);
            let edges = [
                Rect { left: r.left - 1, top: r.top - 1, right: r.right + 1, bottom: r.top },
                Rect { left: r.left - 1, top: r.bottom, right: r.right + 1, bottom: r.bottom + 1 },
                Rect { left: r.left - 1, top: r.top, right: r.left, bottom: r.bottom },
                Rect { left: r.right, top: r.top, right: r.right + 1, bottom: r.bottom },
            ];
            for edge in edges {
                let _ = PatBlt(back, edge.left, edge.top, edge.width(), edge.height(), WHITENESS);
            }
            draw_label(view, drag);
        }
        SelectClipRgn(back, None);
    }
}

/// # Safety
/// As for [`compose`].
unsafe fn draw_label(view: &View, drag: &Drag) {
    let back = view.back.dc();
    let mut text: Vec<u16> = geometry::size_text(drag.region).encode_utf16().collect();
    let mut rect = to_rect(drag.label);
    // SAFETY: per the contract above; the font is deselected again.
    unsafe {
        let fill = CreateSolidBrush(LABEL_FILL);
        FillRect(back, &rect, fill);
        let _ = DeleteObject(fill.into());
        let previous = SelectObject(back, view.chrome.font.into());
        SetBkMode(back, TRANSPARENT);
        SetTextColor(back, LABEL_TEXT);
        DrawTextW(back, &mut text, &mut rect, DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_NOPREFIX);
        SelectObject(back, previous);
    }
}

/// The label's size for `text`: the text plus the chrome's padding.
fn label_size(view: &View, text: &str) -> (i32, i32) {
    let text: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE::default();
    // SAFETY: measures with the view's own back-buffer DC, restoring its font.
    unsafe {
        let previous = SelectObject(view.back.dc(), view.chrome.font.into());
        let _ = GetTextExtentPoint32W(view.back.dc(), &text, &mut size);
        SelectObject(view.back.dc(), previous);
    }
    (size.cx + 2 * view.chrome.pad_x, size.cy + 2 * view.chrome.pad_y)
}

fn invalidate(hwnd: HWND, area: Rect) {
    let rect = to_rect(area);
    // SAFETY: `hwnd` is one of the session's windows.
    unsafe {
        let _ = InvalidateRect(Some(hwnd), Some(&rect), false);
    }
}

fn inflate(r: Rect, by: i32) -> Rect {
    Rect { left: r.left - by, top: r.top - by, right: r.right + by, bottom: r.bottom + by }
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) }
}

fn to_rect(r: Rect) -> RECT {
    RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

fn from_rect(r: RECT) -> Rect {
    Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}
