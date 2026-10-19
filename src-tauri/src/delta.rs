//! The Ctrl+C pixel transfer (spec §9, ANNOTA-23): the page doesn't send the whole 33 MB export back. Rust
//! keeps the Region it sent to the page; the page compares its export with that Region and sends only the
//! tiles Boxes and Callouts changed (`src/delta.ts`), and Rust patches its copy and writes the clipboard.
//! The patched Region equals the export exactly, because the page compares every pixel.

use std::sync::Mutex;

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

/// The Region Rust sent to the page, kept until the Snap ends or is copied.
#[derive(Default)]
pub struct Kept(Mutex<Option<Region>>);

struct Region {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

impl Kept {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Region>> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A Region is about to go to the page.
    pub fn keep(&self, width: u32, height: u32, rgba: &[u8]) {
        *self.lock() = Some(Region { width: width as usize, height: height as usize, rgba: rgba.to_vec() });
    }

    /// The Snap is over: let go of the Region.
    pub fn release(&self) {
        *self.lock() = None;
    }

    /// The Region patched with the page's changed rectangles, or why it can't be. The Region is spent: a
    /// retry sends the whole export instead.
    pub fn patched(&self, payload: &[u8]) -> Result<(Vec<u8>, usize, usize), String> {
        let Region { width, height, mut rgba } = self.lock().take().ok_or("no Region is waiting for its changed rectangles")?;
        apply_rects(&mut rgba, width, height, payload)?;
        Ok((rgba, width, height))
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
    fn kept_hands_a_patched_copy_once() {
        let kept = Kept::default();
        kept.keep(4, 3, &base());
        let mut payload = le(&[1, 0, 0, 1, 1]);
        payload.extend([7, 7, 7, 7]);
        let (image, w, h) = kept.patched(&payload).unwrap();
        assert_eq!((w, h), (4, 3));
        assert_eq!(&image[..4], &[7, 7, 7, 7]);
        assert!(kept.patched(&payload).is_err(), "the Region is spent");
    }

    #[test]
    fn a_released_region_cannot_be_patched() {
        let kept = Kept::default();
        kept.keep(4, 3, &base());
        kept.release();
        assert!(kept.patched(&le(&[0])).is_err(), "released with the Snap");
    }
}
