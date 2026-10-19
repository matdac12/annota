//! The Resident's state machine (spec §3.1) and the entry point every trigger goes through.
//!
//! `--snap` (via the single-instance callback), the tray's left-click and its Snap item all call
//! [`request_snap`]. It runs on the main thread, inside `WM_COPYDATA` for `--snap`, so it only decides
//! and starts the work; it never waits on the WebView.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

/// What the Resident is doing. A `--snap` that arrives while the Resident is still starting (spec §3.1,
/// "Starting") never gets here: the second process is dropped by the `Local\` mutex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing on screen.
    Idle,
    /// The Overlay is up and the user is dragging a Region.
    #[allow(dead_code)] // Entered by the Overlay slice (ANNOTA-15).
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

    #[allow(dead_code)] // Called by the Overlay and Editor slices as the Snap moves along.
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

/// Stub: the grab, the Overlay and the Editor hand-over arrive in later slices. The real one grabs every
/// monitor first, then moves the Resident to `Selecting` (before returning, so a second request can't
/// start another Snap) and shows the Overlay. The stub stays `Idle`, so
/// each request logs.
pub fn start_snap(_app: &AppHandle) {
    eprintln!("start_snap");
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
