//! Timing marks for the joint test session (ANNOTA-20). Everything here exists only with
//! `--features timings`; without it each function is an empty inline stub, so release builds are unchanged.
//!
//! A mark is a QueryPerformanceCounter reading under a name, kept in an in-memory ring. Nothing is written
//! to disk: the tray's "Dump timings" item puts the ring on the clipboard as text. The names, in the
//! order a Snap passes them:
//!
//! - second process: `proc_main` (entry of `main()`), `proc_sent` (just before `WM_COPYDATA`)
//! - `ahk_key`: `hyper.ahk` at the hotkey entry, handed over as `--qpc=<ticks>`
//! - `callback`: top of the single-instance callback in the Resident
//! - `grab_start`, `blit` (one per monitor), `grab_done`
//! - `mouse_up`, `region` (cropped, Overlay gone), `deliver` (pixels handed to the page)
//! - page: `page_bytes` (`next_snap` answered), `page_drawn` (`putImageData` done), `page_frame`
//!   (a frame presented), then `painted` (Rust got `editor_painted`), `shown` (Editor shown and focused)
//! - copy: `copy_key` (Ctrl+C in the page), `page_exported` (`getImageData` done), `copy_recv` (Rust got
//!   the pixels), `clipboard_set`, `closed` (Editor hidden); or `copy_requested` for Caps+S in the Editor

#[cfg(feature = "timings")]
mod on {
    use std::collections::VecDeque;
    use std::fmt::Write as _;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
    use std::sync::Mutex;

    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

    const CAPACITY: usize = 8192;

    struct Entry {
        snap: u32,
        name: &'static str,
        ticks: i64,
        detail: i64,
    }

    static RING: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());
    /// Counts `callback` marks; every later mark belongs to the latest one.
    static SNAP: AtomicU32 = AtomicU32::new(0);
    static CAPTUREBLT: AtomicBool = AtomicBool::new(false);
    static MAIN_ENTRY: AtomicI64 = AtomicI64::new(0);

    pub fn now() -> i64 {
        let mut ticks = 0i64;
        // SAFETY: a plain out-pointer to a live local.
        let _ = unsafe { QueryPerformanceCounter(&mut ticks) };
        ticks
    }

    fn frequency() -> i64 {
        let mut hz = 0i64;
        // SAFETY: as in `now`.
        let _ = unsafe { QueryPerformanceFrequency(&mut hz) };
        hz
    }

    fn push(name: &'static str, ticks: i64, detail: i64) {
        let mut ring = RING.lock().unwrap_or_else(|p| p.into_inner());
        if ring.len() == CAPACITY {
            ring.pop_front();
        }
        ring.push_back(Entry { snap: SNAP.load(Ordering::SeqCst), name, ticks, detail });
    }

    pub fn mark(name: &'static str) {
        push(name, now(), 0);
    }

    pub fn mark_detail(name: &'static str, detail: i64) {
        push(name, now(), detail);
    }

    /// The page's marks arrive by name; only these are accepted.
    pub fn page_mark(name: &str) {
        const NAMES: [&str; 6] = ["page_bytes", "page_drawn", "page_frame", "copy_key", "page_exported", "page_shown"];
        if let Some(known) = NAMES.iter().find(|n| **n == name) {
            mark(known);
        }
    }

    /// Top of the single-instance callback: starts a new row and reads the stamps the second process and
    /// `hyper.ahk` put on the command line.
    pub fn callback(argv: &[String]) {
        let t = now();
        SNAP.fetch_add(1, Ordering::SeqCst);
        push("callback", t, 0);
        for arg in argv {
            for (prefix, name) in [("--qpc=", "ahk_key"), ("--qpc-main=", "proc_main"), ("--qpc-sent=", "proc_sent")] {
                if let Some(ticks) = arg.strip_prefix(prefix).and_then(|v| v.parse::<i64>().ok()) {
                    push(name, ticks, 0);
                }
            }
        }
    }

    /// First line of `main()`.
    pub fn process_start() {
        MAIN_ENTRY.store(now(), Ordering::SeqCst);
    }

    /// The second process adds its own stamps to what it forwards.
    pub fn extend_forwarded(args: &mut Vec<String>) {
        args.push(format!("--qpc-main={}", MAIN_ENTRY.load(Ordering::SeqCst)));
        args.push(format!("--qpc-sent={}", now()));
    }

    /// `--no-fast`: skip the fast path so the stock plugin handles the handoff (the "stock" arm).
    pub fn fast_path_disabled() -> bool {
        std::env::args().any(|a| a == "--no-fast")
    }

    pub fn captureblt() -> bool {
        CAPTUREBLT.load(Ordering::SeqCst)
    }

    pub fn toggle_captureblt() -> bool {
        !CAPTUREBLT.fetch_xor(true, Ordering::SeqCst)
    }

    pub fn clear() {
        RING.lock().unwrap_or_else(|p| p.into_inner()).clear();
        SNAP.store(0, Ordering::SeqCst);
    }

    /// The tray's timing items: dump the ring to the clipboard as text, clear it, and flip the grab between
    /// `SRCCOPY` and `SRCCOPY | CAPTUREBLT`.
    #[derive(Clone)]
    pub struct TrayItems {
        dump: tauri::menu::MenuItem<tauri::Wry>,
        clear: tauri::menu::MenuItem<tauri::Wry>,
        blt: tauri::menu::MenuItem<tauri::Wry>,
    }

    const DUMP: &str = "timings-dump";
    const CLEAR: &str = "timings-clear";
    const BLT: &str = "timings-blt";

    fn blt_label() -> &'static str {
        if captureblt() {
            "Grab: SRCCOPY + CAPTUREBLT (click for SRCCOPY)"
        } else {
            "Grab: SRCCOPY (click for + CAPTUREBLT)"
        }
    }

    pub fn tray_items(app: &tauri::App) -> tauri::Result<TrayItems> {
        use tauri::menu::MenuItem;
        Ok(TrayItems {
            dump: MenuItem::with_id(app, DUMP, "Dump timings to clipboard", true, None::<&str>)?,
            clear: MenuItem::with_id(app, CLEAR, "Clear timings", true, None::<&str>)?,
            blt: MenuItem::with_id(app, BLT, blt_label(), true, None::<&str>)?,
        })
    }

    impl TrayItems {
        pub fn append_to(&self, menu: &tauri::menu::Menu<tauri::Wry>) -> tauri::Result<()> {
            menu.append(&self.dump)?;
            menu.append(&self.clear)?;
            menu.append(&self.blt)
        }

        /// True if `id` was one of ours.
        pub fn handle(&self, id: &str) -> bool {
            match id {
                DUMP => {
                    let text = dump();
                    if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
                        eprintln!("timings: clipboard: {e}");
                    }
                }
                CLEAR => clear(),
                BLT => {
                    toggle_captureblt();
                    let _ = self.blt.set_text(blt_label());
                }
                _ => return false,
            }
            true
        }
    }

    /// `freq=<Hz>` then one `snap name ticks detail` line per mark, in the order they were taken.
    pub fn dump() -> String {
        let ring = RING.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = format!("freq={}\n", frequency());
        for e in ring.iter() {
            let _ = writeln!(out, "{} {} {} {}", e.snap, e.name, e.ticks, e.detail);
        }
        out
    }
}

#[cfg(feature = "timings")]
pub use on::*;

#[cfg(not(feature = "timings"))]
mod off {
    #[inline(always)]
    pub fn mark(_name: &'static str) {}
    #[inline(always)]
    pub fn mark_detail(_name: &'static str, _detail: i64) {}
    #[inline(always)]
    pub fn callback(_argv: &[String]) {}
    #[inline(always)]
    pub fn process_start() {}
    #[inline(always)]
    pub fn extend_forwarded(_args: &mut Vec<String>) {}
    #[inline(always)]
    pub fn fast_path_disabled() -> bool {
        false
    }
    #[inline(always)]
    pub fn captureblt() -> bool {
        false
    }
}

#[cfg(not(feature = "timings"))]
pub use off::*;
