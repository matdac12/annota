//! How the exported pixels get from the page to Rust (spec §9, ANNOTA-23). Candidates, switchable in a
//! timings build with `--transfer=raw|delta|shared`:
//!
//! - **Raw**: `getImageData` as the body of `copy_snap`, 33 MB at 4K (what v1 shipped).
//! - **Delta**: Rust keeps the Region it sent; the page sends only the tiles Boxes and Callouts changed
//!   ([`apply_rects`]), and Rust patches its copy.
//! - **Shared**: a WebView2 shared buffer the size of the Region, posted to the page after the Editor shows;
//!   the page copies its pixels into it and `copy_shared` reads them in place.

use std::sync::Mutex;

/// Which way the page sends its pixels; the page learns it from the trailer of `next_snap`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Raw,
    Delta,
    Shared,
}

impl Mode {
    pub fn code(self) -> u32 {
        match self {
            Mode::Raw => 0,
            Mode::Delta => 1,
            Mode::Shared => 2,
        }
    }
}

/// Patches `base` (top-down RGBA, `width`×`height`) with the rectangles in `payload`: a little-endian `u32`
/// count, then `x y w h` as `u32`s for each, then each rectangle's pixels row by row. The page writes it in
/// `src/delta.ts`.
pub fn apply_rects(base: &mut [u8], width: usize, height: usize, payload: &[u8]) -> Result<(), String> {
    if base.len() != width * height * 4 {
        return Err(format!("{} bytes is not a {width}x{height} image", base.len()));
    }
    let word = |at: usize| -> Result<usize, String> {
        payload
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| "the changed rectangles end early".to_string())
    };
    let count = word(0)?;
    let mut pixels = 4usize.saturating_add(count.saturating_mul(16));
    for rect in 0..count {
        let at = 4 + rect * 16;
        let (x, y, w, h) = (word(at)?, word(at + 4)?, word(at + 8)?, word(at + 12)?);
        if w == 0 || h == 0 || x.checked_add(w).is_none_or(|r| r > width) || y.checked_add(h).is_none_or(|b| b > height) {
            return Err(format!("rectangle {w}x{h} at ({x}, {y}) is outside the {width}x{height} image"));
        }
        for row in 0..h {
            let from = payload.get(pixels..pixels + w * 4).ok_or("the changed rectangles end early")?;
            let to = ((y + row) * width + x) * 4;
            base[to..to + w * 4].copy_from_slice(from);
            pixels += w * 4;
        }
    }
    if pixels != payload.len() {
        return Err(format!("{} bytes follow the changed rectangles", payload.len() - pixels));
    }
    Ok(())
}

/// What the Resident keeps between `editor::open` and the copy, for the candidate that needs it.
#[derive(Default)]
pub struct Staged(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    mode: Mode,
    /// Counts Snaps handed to the page: the page only trusts a shared buffer tagged with the current one.
    serial: u64,
    size: (u32, u32),
    /// Delta: the Region as the page received it.
    base: Option<Vec<u8>>,
    #[cfg(windows)]
    shared: Option<shared::Held>,
}

impl Staged {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A Region is about to go to the page.
    pub fn stage(&self, mode: Mode, width: u32, height: u32, rgba: &[u8]) {
        let mut inner = self.lock();
        inner.mode = mode;
        inner.serial += 1;
        inner.size = (width, height);
        inner.base = (mode == Mode::Delta).then(|| rgba.to_vec());
    }

    /// The mode and the serial of the Snap now open, which the page is told with `transfer-mode`.
    pub fn announcement(&self) -> (u32, u64) {
        let inner = self.lock();
        (inner.mode.code(), inner.serial)
    }

    /// The Snap is over: let go of what it kept.
    pub fn release(&self) {
        let mut inner = self.lock();
        inner.base = None;
        #[cfg(windows)]
        if let Some(held) = inner.shared.take() {
            held.close();
        }
    }

    /// The Region patched with the page's changed rectangles, or why it can't be.
    pub fn patched(&self, payload: &[u8]) -> Result<(Vec<u8>, usize, usize), String> {
        let mut inner = self.lock();
        let (width, height) = (inner.size.0 as usize, inner.size.1 as usize);
        let mut base = inner.base.take().ok_or("no Region is waiting for its changed rectangles")?;
        apply_rects(&mut base, width, height, payload)?;
        Ok((base, width, height))
    }
}

#[cfg(windows)]
pub mod shared {
    use tauri::{AppHandle, Manager};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment12, ICoreWebView2SharedBuffer, ICoreWebView2_17,
        COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
    };
    use windows_core::{Interface, PCWSTR};

    use super::{Mode, Staged};
    use crate::editor;

    /// A WebView2 shared buffer and where its bytes are.
    pub struct Held {
        buffer: ICoreWebView2SharedBuffer,
        ptr: *mut u8,
        len: usize,
    }

    // SAFETY: the interface is only released (`close`) on the main thread, where it was made; other
    // threads read the mapped bytes, which stay valid until then, and `Staged`'s mutex orders both.
    unsafe impl Send for Held {}

    impl Held {
        pub fn close(self) {
            // SAFETY: closing a buffer we made; its mapping goes away with it.
            unsafe {
                let _ = self.buffer.Close();
            }
        }
    }

    impl Staged {
        /// Creates the buffer for the staged Region and posts it to the page. On the main thread, once the
        /// Editor is shown. The page copies into it only if it has arrived by Ctrl+C; otherwise it sends raw.
        pub fn post_shared(&self, app: &AppHandle) {
            let (mode, (width, height), serial) = {
                let inner = self.lock();
                (inner.mode, inner.size, inner.serial)
            };
            if mode != Mode::Shared {
                return;
            }
            let len = width as usize * height as usize * 4;
            let tag = windows_core::HSTRING::from(format!("{{\"serial\":{serial}}}"));
            let Some(window) = app.get_webview_window(editor::LABEL) else { return };
            let app = app.clone();
            let _ = window.with_webview(move |webview| {
                // SAFETY: on the UI thread that owns this live controller and environment.
                let made = (|| unsafe {
                    let environment = webview.environment().cast::<ICoreWebView2Environment12>()?;
                    let buffer = environment.CreateSharedBuffer(len as u64)?;
                    let mut ptr = std::ptr::null_mut();
                    buffer.Buffer(&mut ptr)?;
                    let core = webview.controller().CoreWebView2()?.cast::<ICoreWebView2_17>()?;
                    core.PostSharedBufferToScript(&buffer, COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE, PCWSTR(tag.as_ptr()))?;
                    windows_core::Result::Ok(Held { buffer, ptr, len })
                })();
                match made {
                    Ok(held) => {
                        crate::timings::mark("shared_posted");
                        let staged = app.state::<Staged>();
                        let old = staged.lock().shared.replace(held);
                        if let Some(old) = old {
                            old.close();
                        }
                    }
                    Err(e) => eprintln!("transfer: shared buffer: {e}"),
                }
            });
        }

        /// Runs `use_pixels` on the shared buffer's bytes, which the page filled.
        pub fn with_shared<T>(&self, use_pixels: impl FnOnce(&[u8], usize, usize) -> T) -> Result<T, String> {
            let inner = self.lock();
            let held = inner.shared.as_ref().ok_or("no shared buffer was posted for this Snap")?;
            // SAFETY: `ptr` maps `len` bytes until `close`, which also takes this lock.
            let bytes = unsafe { std::slice::from_raw_parts(held.ptr, held.len) };
            Ok(use_pixels(bytes, inner.size.0 as usize, inner.size.1 as usize))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4×3, every byte its index, so a patch shows where it landed.
    fn base() -> Vec<u8> {
        (0..4 * 3 * 4).map(|i| i as u8).collect()
    }

    fn le(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn a_rectangle_lands_where_it_says_and_nowhere_else() {
        let mut image = base();
        // One 2×2 rectangle at (1, 1): four pixels, all 0xEE.
        let mut payload = le(&[1, 1, 1, 2, 2]);
        payload.extend([0xEE; 2 * 2 * 4]);
        apply_rects(&mut image, 4, 3, &payload).unwrap();
        let mut expected = base();
        for (row, x) in [(1, 1), (1, 2), (2, 1), (2, 2)] {
            expected[(row * 4 + x) * 4..][..4].copy_from_slice(&[0xEE; 4]);
        }
        assert_eq!(image, expected);
    }

    #[test]
    fn several_rectangles_apply_in_order() {
        let mut image = base();
        let mut payload = le(&[2, 0, 0, 1, 1, 3, 2, 1, 1]);
        payload.extend([1, 2, 3, 4]); // (0, 0)
        payload.extend([9, 9, 9, 9]); // (3, 2)
        apply_rects(&mut image, 4, 3, &payload).unwrap();
        assert_eq!(&image[..4], &[1, 2, 3, 4]);
        assert_eq!(&image[(2 * 4 + 3) * 4..], &[9, 9, 9, 9]);
        assert_eq!(&image[4..8], &base()[4..8], "its neighbour is untouched");
    }

    #[test]
    fn no_rectangles_is_no_change() {
        let mut image = base();
        apply_rects(&mut image, 4, 3, &le(&[0])).unwrap();
        assert_eq!(image, base());
    }

    #[test]
    fn a_rectangle_outside_the_image_is_refused() {
        let mut image = base();
        let mut payload = le(&[1, 3, 0, 2, 1]);
        payload.extend([0; 8]);
        assert!(apply_rects(&mut image, 4, 3, &payload).unwrap_err().contains("outside"));
        let mut payload = le(&[1, 0, 2, 1, 2]);
        payload.extend([0; 8]);
        assert!(apply_rects(&mut image, 4, 3, &payload).unwrap_err().contains("outside"));
        assert!(apply_rects(&mut image, 4, 3, &le(&[1, 0, 0, 0, 1])).is_err(), "an empty rectangle");
    }

    #[test]
    fn a_payload_that_is_short_or_long_is_refused() {
        let mut image = base();
        assert!(apply_rects(&mut image, 4, 3, &[]).is_err());
        assert!(apply_rects(&mut image, 4, 3, &le(&[1, 0, 0])).is_err(), "the table ends early");
        let mut short = le(&[1, 0, 0, 2, 1]);
        short.extend([0; 7]);
        assert!(apply_rects(&mut image, 4, 3, &short).is_err(), "a pixel is missing");
        let mut long = le(&[1, 0, 0, 1, 1]);
        long.extend([0; 5]);
        assert!(apply_rects(&mut image, 4, 3, &long).unwrap_err().contains("follow"));
    }

    #[test]
    fn the_payload_the_page_test_pins_applies() {
        // The same bytes `test/delta.test.ts` expects `encodeDelta` to write for a 2×1 image whose second
        // pixel changed: one 2×1 rectangle at (0, 0).
        let payload: Vec<u8> = vec![
            1, 0, 0, 0, // one rectangle
            0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, // x=0 y=0 w=2 h=1
            1, 2, 3, 255, 40, 50, 60, 255,
        ];
        let mut image = vec![1, 2, 3, 255, 4, 5, 6, 255];
        apply_rects(&mut image, 2, 1, &payload).unwrap();
        assert_eq!(image, [1, 2, 3, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn staged_hands_a_patched_copy_once() {
        let staged = Staged::default();
        staged.stage(Mode::Delta, 4, 3, &base());
        let mut payload = le(&[1, 0, 0, 1, 1]);
        payload.extend([7, 7, 7, 7]);
        let (image, w, h) = staged.patched(&payload).unwrap();
        assert_eq!((w, h), (4, 3));
        assert_eq!(&image[..4], &[7, 7, 7, 7]);
        assert!(staged.patched(&payload).is_err(), "the Region is spent");
    }

    #[test]
    fn only_delta_keeps_a_copy_of_the_region() {
        let staged = Staged::default();
        staged.stage(Mode::Raw, 4, 3, &base());
        assert!(staged.patched(&le(&[0])).is_err());
        staged.stage(Mode::Delta, 4, 3, &base());
        staged.release();
        assert!(staged.patched(&le(&[0])).is_err(), "released with the Snap");
    }
}
