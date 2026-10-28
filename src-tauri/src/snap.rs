//! The Resident's state machine (spec §3.1) and the entry point every trigger goes through.
//!
//! `--snap` (via the single-instance callback), the tray's left-click and its Snap item all call
//! [`request_snap`]. It runs on the main thread, inside `WM_COPYDATA` for `--snap`, so it only decides
//! and starts the work; it never waits on the WebView.

use std::borrow::Cow;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::capture;
use crate::commands::Handoff;
use crate::editor;
use crate::overlay::{self, Outcome};
use crate::delta::Kept;
use crate::timings;

/// What the Resident tells the Editor's page when `--snap` arrives while it is open: do what Ctrl+C does.
pub const COPY_REQUESTED: &str = "copy-requested";

/// If the page hasn't painted a Region by now, show the Editor anyway rather than leave the Resident
/// stuck in `Selecting` with nothing on screen.
const PAINT_DEADLINE: Duration = Duration::from_millis(1500);

/// If the page hasn't started the copy it was asked for by now, its `copy-requested` listener is gone and
/// the Editor stays open: worth a log line, not another wait.
const COPY_DEADLINE: Duration = Duration::from_secs(3);

/// What the Resident is doing. A `--snap` that arrives while the Resident is still starting (spec §3.1,
/// "Starting") never gets here: the second process is dropped by the `Local\` mutex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing on screen.
    Idle,
    /// The Overlay is up and the user is dragging a Region.
    Selecting,
    /// The Editor is open on a Region.
    Editing,
}

/// What a snap request does in a given state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    StartSnap,
    Ignore,
    CopyAndClose,
}

impl State {
    pub fn on_snap_request(self) -> Reaction {
        match self {
            State::Idle => Reaction::StartSnap,
            State::Selecting => Reaction::Ignore,
            State::Editing => Reaction::CopyAndClose,
        }
    }
}

/// The Resident's current [`State`], managed by the Tauri app. Whoever moves the Snap along (the grab,
/// the Overlay, the Editor) calls [`Resident::set`].
pub struct Resident(Mutex<State>);

impl Resident {
    pub fn new() -> Self {
        Resident(Mutex::new(State::Idle))
    }

    pub fn state(&self) -> State {
        *self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set(&self, state: State) {
        *self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
    }
}

/// `--snap`, the tray's left-click and its Snap item all end up here.
pub fn request_snap(app: &AppHandle) {
    // Not managed yet only if a request beats `main()`'s builder; drop it rather than abort the process.
    let Some(resident) = app.try_state::<Resident>() else {
        return;
    };
    match resident.state().on_snap_request() {
        Reaction::StartSnap => start_snap(app),
        Reaction::Ignore => {}
        Reaction::CopyAndClose => copy_and_close(app),
    }
}

/// Starts a Snap (spec §4, §5): grabs every monitor first, then puts the Overlay up.
///
/// The Resident is `Selecting` *before this returns*: for `--snap` it runs on the main thread inside
/// `WM_COPYDATA`, and the next request is only handled after it, so it must already see `Selecting` and be
/// ignored. The grab and the Overlay's windows belong on the main thread too (`run_on_main_thread` runs
/// inline when already there), so a request from another thread still grabs before any window shows.
pub fn start_snap(app: &AppHandle) {
    let Some(resident) = app.try_state::<Resident>() else {
        return;
    };
    resident.set(State::Selecting);
    let on_main = app.clone();
    if app.run_on_main_thread(move || grab_and_select(&on_main)).is_err() {
        resident.set(State::Idle);
    }
}

/// Grab first, show second; any failure leaves the Resident `Idle`.
fn grab_and_select(app: &AppHandle) {
    let finished = app.clone();
    let shown = capture::grab_all()
        .and_then(|captures| overlay::show(captures, Box::new(move |outcome| selection_ended(&finished, outcome))));
    if let Err(e) = shown {
        eprintln!("snap: {e}");
        set_state(app, State::Idle);
    }
}

/// The Overlay is gone. A cancel is back to `Idle`. A Region goes to the Editor, which shows itself once
/// its page has painted it ([`editor_painted`]).
fn selection_ended(app: &AppHandle, outcome: Outcome) {
    match outcome {
        Outcome::Cancelled => set_state(app, State::Idle),
        Outcome::Selected(region) => {
            timings::mark("region");
            eprintln!(
                "snap: Region {}x{} at ({}, {}) on the monitor at ({}, {})",
                region.width(),
                region.height(),
                region.screen.left,
                region.screen.top,
                region.monitor.left,
                region.monitor.top
            );
            match editor::open(app, region) {
                Ok(token) => watch_for_paint(app, token),
                Err(e) => {
                    eprintln!("snap: editor: {e}");
                    editor::hide(app);
                    set_state(app, State::Idle);
                }
            }
        }
    }
}

/// The page called `editor_painted` (or the deadline passed): show the Editor and move to `Editing`. Only
/// the first of the two does anything.
pub fn editor_painted(app: &AppHandle) {
    timings::mark("painted");
    if !app.state::<Handoff>().take_paint() {
        return;
    }
    editor::reveal(app);
    timings::mark("shown");
    set_state(app, State::Editing);
}

fn watch_for_paint(app: &AppHandle, token: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(PAINT_DEADLINE);
        let on_main = app.clone();
        let _ = app.run_on_main_thread(move || {
            if on_main.state::<Handoff>().take_paint_for(token) {
                eprintln!("snap: the page did not report a paint in time; showing the Editor");
                editor::reveal(&on_main);
                set_state(&on_main, State::Editing);
            }
        });
    });
}

fn set_state(app: &AppHandle, state: State) {
    if let Some(resident) = app.try_state::<Resident>() {
        resident.set(state);
    }
}

/// The page's `getImageData` goes to the clipboard (spec §9): `arboard` writes the "PNG" and `CF_DIBV5`
/// formats, opaque sRGB. The Editor closes once the clipboard is set. Anything but `Editing` is a stale
/// request (the Snap already ended) and is ignored.
pub fn copy_image(app: &AppHandle, width: usize, height: usize, rgba: &[u8]) -> Result<(), String> {
    if app.try_state::<Resident>().is_none_or(|r| r.state() != State::Editing) {
        return Ok(());
    }
    if width == 0 || height == 0 || rgba.len() != width * height * 4 {
        return Err(format!("{} bytes is not a {width}x{height} RGBA image", rgba.len()));
    }
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_image(arboard::ImageData { width, height, bytes: Cow::Borrowed(rgba) }))
        .map_err(|e| format!("clipboard: {e}"))?;
    timings::mark("clipboard_set");
    finish(app);
    Ok(())
}

/// Esc or the title-bar ✕: close the Editor with nothing copied.
pub fn discard(app: &AppHandle) {
    if app.try_state::<Resident>().is_some_and(|r| r.state() == State::Editing) {
        finish(app);
    }
}

/// Hides the Editor and goes back to `Idle`, on the main thread where the window lives.
fn finish(app: &AppHandle) {
    app.state::<Kept>().release();
    let on_main = app.clone();
    let queued = app.run_on_main_thread(move || {
        editor::hide(&on_main);
        timings::mark("closed");
        on_main.state::<Handoff>().forget_region();
        set_state(&on_main, State::Idle);
    });
    if queued.is_err() {
        set_state(app, State::Idle);
    }
}

/// `--snap` (or the tray) while the Editor is open: the same as Ctrl+C. The canvas lives in the page, so
/// ask it to copy; a request the page can't hear, or never answers, is logged rather than lost.
pub fn copy_and_close(app: &AppHandle) {
    timings::mark("copy_requested");
    if let Err(e) = app.emit_to(editor::LABEL, COPY_REQUESTED, ()) {
        eprintln!("snap: could not ask the Editor to copy: {e}");
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(COPY_DEADLINE);
        let on_main = app.clone();
        let _ = app.run_on_main_thread(move || {
            if copy_went_unanswered(on_main.state::<Resident>().state()) {
                eprintln!("snap: the page did not answer the copy request; the Editor is still open");
            }
        });
    });
}

/// Whether the page never answered `copy-requested`: the Snap is still in the Editor after the deadline,
/// so its `copy_snap`/`copy_delta` never ran.
fn copy_went_unanswered(state: State) -> bool {
    state == State::Editing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snap_request_in_idle_starts_a_snap() {
        assert_eq!(State::Idle.on_snap_request(), Reaction::StartSnap);
    }

    #[test]
    fn a_snap_request_while_selecting_is_ignored() {
        assert_eq!(State::Selecting.on_snap_request(), Reaction::Ignore);
    }

    #[test]
    fn a_snap_request_in_the_editor_copies_and_closes() {
        assert_eq!(State::Editing.on_snap_request(), Reaction::CopyAndClose);
    }

    #[test]
    fn a_copy_request_the_page_has_not_answered_by_the_deadline_is_reported() {
        assert!(copy_went_unanswered(State::Editing));
    }

    #[test]
    fn a_copy_request_that_was_answered_is_not_reported() {
        assert!(!copy_went_unanswered(State::Idle), "the page copied and the Editor closed");
        assert!(!copy_went_unanswered(State::Selecting), "a new Snap started in the meantime");
    }

    #[test]
    fn the_resident_starts_idle_and_remembers_the_state_it_is_given() {
        let resident = Resident::new();
        assert_eq!(resident.state(), State::Idle);
        resident.set(State::Selecting);
        assert_eq!(resident.state(), State::Selecting);
        resident.set(State::Editing);
        assert_eq!(resident.state(), State::Editing);
    }
}
