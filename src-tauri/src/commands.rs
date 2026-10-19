//! The Editor page's side of the IPC (spec §6.3, §9): `next_snap` parked from load, `editor_painted`, and
//! the copy and discard commands.
//!
//! The Overlay hands a Region over with [`Handoff::deliver`]; the parked `next_snap` wakes with its raw
//! RGBA and dimensions. Nothing here touches a file.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, State};
use tokio::sync::oneshot;

use crate::snap;
use crate::timings;
use crate::delta::Kept;

/// Where a Region waits for the page: either the page is parked on `waiting`, or the Region sits in
/// `ready` until the page asks.
#[derive(Default)]
struct Slot {
    waiting: Option<oneshot::Sender<Vec<u8>>>,
    ready: Option<Vec<u8>>,
}

/// What `next_snap` found.
enum Parked {
    Ready(Vec<u8>),
    Wait(oneshot::Receiver<Vec<u8>>),
}

#[derive(Default)]
pub struct Handoff {
    slot: Mutex<Slot>,
    /// Names the Snap whose paint is awaited: 0 when none is, so the Editor is already shown or closed.
    awaiting_paint: AtomicU64,
    snaps: AtomicU64,
}

impl Handoff {
    /// Gives the page the next Snap's bytes (RGBA, then the dimensions): to the parked `next_snap`, or to
    /// whichever calls it next.
    pub fn deliver(&self, bytes: Vec<u8>) {
        let mut slot = self.slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        slot.ready = match slot.waiting.take() {
            Some(page) => page.send(bytes).err(),
            None => Some(bytes),
        };
    }

    fn park(&self) -> Parked {
        let mut slot = self.slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(bytes) = slot.ready.take() {
            return Parked::Ready(bytes);
        }
        let (page, wait) = oneshot::channel();
        // A reloaded page parks again; the old call ends with an error nobody reads.
        slot.waiting = Some(page);
        Parked::Wait(wait)
    }

    /// The Editor is about to be shown once the page paints. The token names this wait.
    pub fn expect_paint(&self) -> u64 {
        let token = self.snaps.fetch_add(1, Ordering::SeqCst) + 1;
        self.awaiting_paint.store(token, Ordering::SeqCst);
        token
    }

    /// True once per [`expect_paint`](Self::expect_paint): whoever sees it shows the Editor.
    pub fn take_paint(&self) -> bool {
        self.awaiting_paint.swap(0, Ordering::SeqCst) != 0
    }

    /// Like [`take_paint`](Self::take_paint), but only for the wait `token` named, so a watchdog left over
    /// from an earlier Snap can't show a later one early.
    pub fn take_paint_for(&self, token: u64) -> bool {
        self.awaiting_paint.compare_exchange(token, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }

    /// A Region nobody took (the page never asked) doesn't outlive its Snap.
    pub fn forget_region(&self) {
        self.slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).ready = None;
    }
}

/// Parked by the page from load, and again after every Snap: answers with the next Region's raw RGBA
/// followed by its width and height as two little-endian `u32`s.
#[tauri::command]
pub async fn next_snap(handoff: State<'_, Handoff>) -> Result<Response, String> {
    match handoff.park() {
        Parked::Ready(bytes) => Ok(Response::new(bytes)),
        Parked::Wait(wait) => wait.await.map(Response::new).map_err(|_| "a newer next_snap took over".to_string()),
    }
}

/// The page has put the Region on the canvas and a frame has been presented: show the Editor.
#[tauri::command]
pub fn editor_painted(app: AppHandle) {
    snap::editor_painted(&app);
}

/// Ctrl+C, the way back when `copy_delta` can't be used: the canvas's pixels as raw RGBA in the body, `width`
/// and `height` in the headers. Sets the clipboard, then the Editor closes.
#[tauri::command]
pub async fn copy_snap(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    timings::mark("copy_recv");
    let InvokeBody::Raw(rgba) = request.body() else {
        return Err("copy_snap needs the raw RGBA as its body".into());
    };
    let header = |name: &str| {
        request.headers().get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok())
    };
    let (Some(width), Some(height)) = (header("width"), header("height")) else {
        return Err("copy_snap needs width and height headers".into());
    };
    snap::copy_image(&app, width, height, rgba)
}

/// Ctrl+C (spec §9): only the rectangles the Boxes and Callouts changed, which Rust patches into the Region
/// it kept (`delta::apply_rects` has the format). Sets the clipboard, then the Editor closes.
#[tauri::command]
pub async fn copy_delta(app: AppHandle, kept: State<'_, Kept>, request: Request<'_>) -> Result<(), String> {
    timings::mark("copy_recv");
    let InvokeBody::Raw(payload) = request.body() else {
        return Err("copy_delta needs the changed rectangles as its body".into());
    };
    let (rgba, width, height) = kept.patched(payload)?;
    timings::mark("delta_applied");
    snap::copy_image(&app, width, height, &rgba)
}

/// A timing mark from the page (ANNOTA-20); only the named ones are taken, and only in a timings build.
#[cfg(feature = "timings")]
#[tauri::command]
pub fn timing_mark(name: String) {
    timings::page_mark(&name);
}

/// Esc: close the Editor with nothing copied.
#[tauri::command]
pub fn discard_snap(app: AppHandle) {
    snap::discard(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tauri::async_runtime::block_on(f)
    }

    #[test]
    fn a_region_delivered_before_the_page_asks_is_waiting_for_it() {
        let handoff = Handoff::default();
        handoff.deliver(vec![1, 2, 3]);
        match handoff.park() {
            Parked::Ready(bytes) => assert_eq!(bytes, vec![1, 2, 3]),
            Parked::Wait(_) => panic!("the Region was already there"),
        }
        assert!(matches!(handoff.park(), Parked::Wait(_)), "and it is handed over once");
    }

    #[test]
    fn a_page_parked_on_next_snap_wakes_with_the_next_region() {
        let handoff = Handoff::default();
        let Parked::Wait(wait) = handoff.park() else { panic!("nothing was delivered") };
        handoff.deliver(vec![9, 9]);
        assert_eq!(run(wait).unwrap(), vec![9, 9]);
    }

    #[test]
    fn a_page_that_parks_again_replaces_the_old_call_and_still_gets_the_region() {
        let handoff = Handoff::default();
        let Parked::Wait(old) = handoff.park() else { panic!() };
        let Parked::Wait(new) = handoff.park() else { panic!() };
        handoff.deliver(vec![7]);
        assert!(run(old).is_err());
        assert_eq!(run(new).unwrap(), vec![7]);
    }

    #[test]
    fn a_region_sent_to_a_call_that_went_away_is_kept_for_the_next_one() {
        let handoff = Handoff::default();
        let Parked::Wait(gone) = handoff.park() else { panic!() };
        drop(gone);
        handoff.deliver(vec![5]);
        assert!(matches!(handoff.park(), Parked::Ready(bytes) if bytes == vec![5]));
    }

    #[test]
    fn the_editor_is_shown_once_per_expected_paint() {
        let handoff = Handoff::default();
        assert!(!handoff.take_paint(), "a stray editor_painted shows nothing");
        handoff.expect_paint();
        assert!(handoff.take_paint());
        assert!(!handoff.take_paint(), "the watchdog and the page don't both show it");
    }

    #[test]
    fn a_watchdog_from_an_earlier_snap_cannot_show_the_next_one() {
        let handoff = Handoff::default();
        let first = handoff.expect_paint();
        assert!(handoff.take_paint());
        let second = handoff.expect_paint();
        assert!(!handoff.take_paint_for(first), "the first Snap's timer fires during the second");
        assert!(handoff.take_paint_for(second));
        assert!(!handoff.take_paint_for(second));
    }

    #[test]
    fn a_region_nobody_took_is_dropped_with_its_snap() {
        let handoff = Handoff::default();
        handoff.deliver(vec![1]);
        handoff.forget_region();
        assert!(matches!(handoff.park(), Parked::Wait(_)));
    }
}
