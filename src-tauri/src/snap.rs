//! The Resident's state machine (spec §3.1) and the entry point every trigger goes through.
//!
//! `--snap` (via the single-instance callback), the tray's left-click and its Snap item all call
//! [`request_snap`]. It runs on the main thread, inside `WM_COPYDATA` for `--snap`, so it only decides
//! and starts the work; it never waits on the WebView.

use std::borrow::Cow;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::capture::{self, Region};
use crate::overlay::{self, Outcome};

/// What the Resident is doing. A `--snap` that arrives while the Resident is still starting (spec §3.1,
/// "Starting") never gets here: the second process is dropped by the `Local\` mutex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing on screen.
    Idle,
    /// The Overlay is up and the user is dragging a Region.
    Selecting,
    /// The Editor is open on a Region.
    #[allow(dead_code)] // Entered by the Editor slice (ANNOTA-16).
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

/// The Overlay is gone. A cancel is back to `Idle`. A Region goes to the clipboard, a tracer bullet until
/// the Editor takes it from here (ANNOTA-16, which replaces this).
fn selection_ended(app: &AppHandle, outcome: Outcome) {
    match outcome {
        Outcome::Cancelled => set_state(app, State::Idle),
        Outcome::Selected(region) => {
            eprintln!(
                "snap: Region {}x{} at ({}, {}) on the monitor at ({}, {})",
                region.width(),
                region.height(),
                region.screen.left,
                region.screen.top,
                region.monitor.left,
                region.monitor.top
            );
            // Off the main thread: arboard encodes a PNG, which takes a while for a big Region. The
            // Resident stays `Selecting` until it is done, so a second Snap can't start mid-copy.
            let app = app.clone();
            std::thread::spawn(move || {
                if let Err(e) = copy_to_clipboard(&region) {
                    eprintln!("snap: clipboard: {e}");
                }
                set_state(&app, State::Idle);
            });
        }
    }
}

fn set_state(app: &AppHandle, state: State) {
    if let Some(resident) = app.try_state::<Resident>() {
        resident.set(state);
    }
}

/// `arboard` writes the "PNG" and `CF_DIBV5` formats (spec §9). The Region's alpha is already 255.
fn copy_to_clipboard(region: &Region) -> Result<(), arboard::Error> {
    arboard::Clipboard::new()?.set_image(arboard::ImageData {
        width: region.width(),
        height: region.height(),
        bytes: Cow::Borrowed(&region.rgba),
    })
}

/// Stub: renders the Snap, sets the clipboard and closes the Editor (spec §9), in a later slice.
pub fn copy_and_close(_app: &AppHandle) {
    eprintln!("copy_and_close");
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
    fn the_resident_starts_idle_and_remembers_the_state_it_is_given() {
        let resident = Resident::new();
        assert_eq!(resident.state(), State::Idle);
        resident.set(State::Selecting);
        assert_eq!(resident.state(), State::Selecting);
        resident.set(State::Editing);
        assert_eq!(resident.state(), State::Editing);
    }
}
