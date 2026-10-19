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
//! - `grab_start`, `blit` (one per monitor), `grab_done`, `overlay_up` (dimmed copies made, windows shown)
//! - `mouse_up`, `region` (cropped, Overlay gone), `deliver` (pixels handed to the page)
//! - page: `page_bytes` (`next_snap` answered), `page_drawn` (`putImageData` done), `page_frame`
//!   (a frame presented), then `painted` (Rust got `editor_painted`), `shown` (Editor shown and focused)
//! - copy: `copy_key` (Ctrl+C in the page), `page_exported` (`getImageData` done), `copy_recv` (Rust got
//!   the pixels), `clipboard_set`, `closed` (Editor hidden); or `copy_requested` for Caps+S in the Editor
//! - ANNOTA-23: `copy_mode` (detail = clipboard writer * 10 + transfer, see `--clip` and `--transfer`),
//!   `shared_posted` (the shared buffer reached the page), `png_done` (the PNG is encoded) and `dib_done`
//!   (the `CF_DIBV5` block is filled); page: `page_packed` (the delta encoded or the shared buffer filled)

#[cfg(feature = "timings")]
mod on {
    use std::collections::VecDeque;
    use std::fmt::Write as _;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU8, Ordering};
    use std::sync::Mutex;

    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

    use crate::clipboard::{PngLevel, Writer};
    use crate::transfer::Mode;

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
    /// ANNOTA-23's candidates: the clipboard writer (0 arboard, 1 own + fast PNG, 2 own + stored PNG) and
    /// the pixel transfer (0 raw, 1 delta, 2 shared buffer).
    static CLIP: AtomicU8 = AtomicU8::new(0);
    static TRANSFER: AtomicU8 = AtomicU8::new(0);

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
        const NAMES: [&str; 7] =
            ["page_bytes", "page_drawn", "page_frame", "copy_key", "page_exported", "page_shown", "page_packed"];
        if let Some(known) = NAMES.iter().find(|n| **n == name) {
            mark(known);
        }
    }

    /// Top of the single-instance callback: starts a new row and reads the stamps the second process and
    /// `hyper.ahk` put on the command line.
    ///
    /// Also the remote control for the session: `--dump-timings` puts the ring on the clipboard as text,
    /// `--clear-timings` empties it, `--captureblt=on|off` sets the grab's blit flag, and for ANNOTA-23
    /// `--clip=arboard|fast|stored` and `--transfer=raw|delta|shared` pick the copy path. They are not Snaps:
    /// the return value says the command line was one of them.
    pub fn callback(argv: &[String]) -> bool {
        let t = now();
        for arg in argv {
            match arg.as_str() {
                "--dump-timings" => {
                    if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(dump())) {
                        eprintln!("timings: clipboard: {e}");
                    }
                    return true;
                }
                "--clear-timings" => {
                    clear();
                    return true;
                }
                "--clip=arboard" | "--clip=fast" | "--clip=stored" => {
                    let code = ["arboard", "fast", "stored"].iter().position(|n| arg.ends_with(n)).unwrap_or(0);
                    CLIP.store(code as u8, Ordering::SeqCst);
                    return true;
                }
                "--transfer=raw" | "--transfer=delta" | "--transfer=shared" => {
                    let code = ["raw", "delta", "shared"].iter().position(|n| arg.ends_with(n)).unwrap_or(0);
                    TRANSFER.store(code as u8, Ordering::SeqCst);
                    return true;
                }
                "--captureblt=on" | "--captureblt=off" => {
                    CAPTUREBLT.store(arg == "--captureblt=on", Ordering::SeqCst);
                    return true;
                }
                _ => {}
            }
        }
        SNAP.fetch_add(1, Ordering::SeqCst);
        push("callback", t, 0);
        for arg in argv {
            for (prefix, name) in [("--qpc=", "ahk_key"), ("--qpc-main=", "proc_main"), ("--qpc-sent=", "proc_sent")] {
                if let Some(ticks) = arg.strip_prefix(prefix).and_then(|v| v.parse::<i64>().ok()) {
                    push(name, ticks, 0);
                }
            }
        }
        false
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

    pub fn clip_writer() -> Writer {
        match CLIP.load(Ordering::SeqCst) {
            1 => Writer::Own(PngLevel::Fast),
            2 => Writer::Own(PngLevel::Stored),
            _ => Writer::Arboard,
        }
    }

    pub fn transfer_mode() -> Mode {
        match TRANSFER.load(Ordering::SeqCst) {
            1 => Mode::Delta,
            2 => Mode::Shared,
            _ => Mode::Raw,
        }
    }

    /// Names the candidates a copy ran with, for the analysis.
    pub fn mark_copy_mode() {
        mark_detail("copy_mode", i64::from(CLIP.load(Ordering::SeqCst)) * 10 + i64::from(TRANSFER.load(Ordering::SeqCst)));
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
    use crate::clipboard::Writer;
    use crate::transfer::Mode;

    #[inline(always)]
    pub fn mark(_name: &'static str) {}
    #[inline(always)]
    pub fn mark_detail(_name: &'static str, _detail: i64) {}
    #[inline(always)]
    pub fn callback(_argv: &[String]) -> bool {
        false
    }
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
    #[inline(always)]
    pub fn clip_writer() -> Writer {
        Writer::Arboard
    }
    #[inline(always)]
    pub fn transfer_mode() -> Mode {
        Mode::Raw
    }
    #[inline(always)]
    pub fn mark_copy_mode() {}
}

#[cfg(not(feature = "timings"))]
pub use off::*;
