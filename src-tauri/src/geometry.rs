//! The pure maths of the Overlay and the Region hand-over (spec §5.3, §6.2, §6.3): clamping a drag to its
//! monitor, the 8×8 threshold, cropping the frozen frame, the label placement, and where the Editor opens.
//! No Win32 in here.
//!
//! Everything is in physical pixels. "Local" coordinates are relative to one monitor's top-left;
//! "screen" coordinates are virtual-desktop ones, where a monitor above the primary has a negative `top`.

/// A half-open rectangle: `left`/`top` are inside, `right`/`bottom` are one past the last pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(self) -> i32 {
        self.right - self.left
    }

    pub fn height(self) -> i32 {
        self.bottom - self.top
    }
}

/// A Region smaller than this in either direction is ignored (spec §5.3).
pub const MIN_REGION: i32 = 8;

/// The Region a drag from `anchor` to `current` selects on a monitor of `width`×`height`, in the monitor's
/// local coordinates. Both corners are pixels the pointer was on, so the Region includes both, and a drag
/// from the first to the last pixel selects the whole monitor. A pointer outside the monitor (the mouse is
/// captured during a drag) is clamped onto it.
pub fn drag_rect(anchor: (i32, i32), current: (i32, i32), width: i32, height: i32) -> Rect {
    let clamp = |(x, y): (i32, i32)| (x.clamp(0, width - 1), y.clamp(0, height - 1));
    let (a, b) = (clamp(anchor), clamp(current));
    Rect { left: a.0.min(b.0), top: a.1.min(b.1), right: a.0.max(b.0) + 1, bottom: a.1.max(b.1) + 1 }
}

/// Whether a Region is big enough to select; anything smaller is a click or a slip.
pub fn is_selectable(region: Rect) -> bool {
    region.width() >= MIN_REGION && region.height() >= MIN_REGION
}

/// `region` (local to a monitor whose top-left is `origin` on the virtual desktop) in screen coordinates.
/// A monitor above or left of the primary one has a negative origin.
pub fn to_screen(region: Rect, origin: (i32, i32)) -> Rect {
    Rect {
        left: region.left + origin.0,
        top: region.top + origin.1,
        right: region.right + origin.0,
        bottom: region.bottom + origin.1,
    }
}

/// Copies `region` out of a monitor's frozen frame as top-down RGBA with alpha 255, ready for the Editor
/// and the clipboard. `frame` is the 32 bpp BGRA bits of a bottom-up DIB, `width`×`height` pixels, so its
/// first row is the monitor's bottom row. `None` if the Region is not inside the frame or the frame is
/// too short.
pub fn crop_to_rgba(frame: &[u8], width: i32, height: i32, region: Rect) -> Option<Vec<u8>> {
    let inside = region.left >= 0 && region.top >= 0 && region.right <= width && region.bottom <= height;
    let (width, height) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
    if !inside || region.width() <= 0 || region.height() <= 0 || frame.len() < width * height * 4 {
        return None;
    }
    let (left, top) = (region.left as usize, region.top as usize);
    let (w, h) = (region.width() as usize, region.height() as usize);
    // Room for the dimensions trailer, so [`with_dimensions`] appends without copying the pixels again.
    let mut rgba = Vec::with_capacity(w * h * 4 + DIMENSIONS_BYTES);
    for y in top..top + h {
        // Bottom-up: picture row `y` sits `height - 1 - y` rows into the buffer.
        let start = ((height - 1 - y) * width + left) * 4;
        for bgra in frame[start..start + w * 4].chunks_exact(4) {
            rgba.extend_from_slice(&[bgra[2], bgra[1], bgra[0], 255]);
        }
    }
    Some(rgba)
}

/// Bytes [`with_dimensions`] appends: width then height, little-endian `u32`.
pub const DIMENSIONS_BYTES: usize = 8;

/// `rgba` (a `width`×`height` image from [`crop_to_rgba`]) with its dimensions appended, which is what the
/// Editor's page receives. At the end rather than the start so the page reads the pixels in place.
pub fn with_dimensions(mut rgba: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    rgba.extend_from_slice(&width.to_le_bytes());
    rgba.extend_from_slice(&height.to_le_bytes());
    rgba
}

/// How much of the capture monitor's work area the Editor may fill (spec §6.2).
pub const FIT_PERCENT: i32 = 90;

/// The Editor window's non-image parts, in physical pixels at the capture monitor's scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    /// Outer size minus inner size: the title bar and the borders.
    pub frame: (i32, i32),
    /// The hint line under the image.
    pub hint: i32,
    /// The narrowest client area that still shows the whole hint line, however small the Region.
    pub min_inner_width: i32,
}

/// Where the Editor goes for one Snap: the client-area size to `set_size`, and the outer top-left to
/// `set_position`, both physical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub inner: (i32, i32),
    pub position: (i32, i32),
}

/// The Editor for an `image` (width, height) on a monitor whose work area is `work` (virtual-desktop
/// physical pixels, so `left`/`top` may be negative): the image at 100%, scaled down only if the whole
/// window wouldn't fit [`FIT_PERCENT`] of the work area, centred on it.
pub fn place_editor(image: (i32, i32), work: Rect, chrome: Chrome) -> Placement {
    let room_w = (work.width() * FIT_PERCENT / 100 - chrome.frame.0).max(1);
    let room_h = (work.height() * FIT_PERCENT / 100 - chrome.frame.1 - chrome.hint).max(1);
    let (w, h) = (image.0.max(1), image.1.max(1));
    let shown = if w <= room_w && h <= room_h {
        (w, h)
    } else if room_w * h <= room_h * w {
        // The width is what binds: floor so the height can't spill over.
        (room_w, (h as i64 * room_w as i64 / w as i64).max(1) as i32)
    } else {
        ((w as i64 * room_h as i64 / h as i64).max(1) as i32, room_h)
    };
    let inner = (shown.0.max(chrome.min_inner_width.min(room_w)), shown.1 + chrome.hint);
    let outer = (inner.0 + chrome.frame.0, inner.1 + chrome.frame.1);
    Placement { inner, position: centred(work, outer) }
}

/// The top-left of a window of `outer` size centred on `work`.
pub fn centred(work: Rect, outer: (i32, i32)) -> (i32, i32) {
    (work.left + (work.width() - outer.0) / 2, work.top + (work.height() - outer.1) / 2)
}

/// The top-left of the `W × H` label for a pointer at `cursor` (monitor-local): `gap` pixels below and to
/// the right of it, flipped to the other side of the pointer on an axis where it would leave the
/// `width`×`height` monitor. `label` is the label's size.
pub fn label_origin(cursor: (i32, i32), label: (i32, i32), gap: i32, width: i32, height: i32) -> (i32, i32) {
    let place = |at: i32, size: i32, limit: i32| if at + gap + size <= limit { at + gap } else { at - gap - size };
    (place(cursor.0, label.0, width), place(cursor.1, label.1, height))
}

/// The text of the size readout, in physical pixels.
pub fn size_text(region: Rect) -> String {
    format!("{} \u{d7} {}", region.width(), region.height())
}

/// What each channel value becomes when the frozen frame is dimmed to `keep_percent` of its brightness
/// (black at `100 - keep_percent` %).
pub fn dim_table(keep_percent: u32) -> [u8; 256] {
    std::array::from_fn(|value| (value as u32 * keep_percent / 100) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (i32, i32) = (1920, 1200);

    fn region(anchor: (i32, i32), current: (i32, i32)) -> Rect {
        drag_rect(anchor, current, SCREEN.0, SCREEN.1)
    }

    #[test]
    fn a_drag_selects_every_pixel_between_both_corners_whichever_way_it_goes() {
        let expected = Rect { left: 100, top: 200, right: 301, bottom: 351 };
        assert_eq!(region((100, 200), (300, 350)), expected);
        assert_eq!(region((300, 350), (100, 200)), expected);
        assert_eq!(region((300, 200), (100, 350)), expected);
        assert_eq!(region((100, 350), (300, 200)), expected);
    }

    #[test]
    fn a_drag_from_the_first_to_the_last_pixel_selects_the_whole_monitor() {
        let all = region((0, 0), (1919, 1199));
        assert_eq!((all.width(), all.height()), (1920, 1200));
    }

    #[test]
    fn a_pointer_outside_the_monitor_is_clamped_to_the_edge_it_left_by() {
        // Past the right and bottom edges, then past the left and top edges.
        assert_eq!(region((1000, 1000), (5000, 5000)), Rect { left: 1000, top: 1000, right: 1920, bottom: 1200 });
        assert_eq!(region((1000, 1000), (-40, -7)), Rect { left: 0, top: 0, right: 1001, bottom: 1001 });
    }

    #[test]
    fn a_click_is_a_one_pixel_region_and_is_not_selectable() {
        let click = region((500, 500), (500, 500));
        assert_eq!((click.width(), click.height()), (1, 1));
        assert!(!is_selectable(click));
    }

    #[test]
    fn a_region_is_selectable_from_eight_by_eight_physical_pixels() {
        assert!(is_selectable(region((10, 10), (17, 17))), "8×8 is the smallest");
        assert!(!is_selectable(region((10, 10), (16, 17))), "7×8");
        assert!(!is_selectable(region((10, 10), (17, 16))), "8×7");
        assert!(!is_selectable(region((10, 10), (400, 11))), "a 391×2 sliver is not a Region");
    }

    #[test]
    fn a_region_on_a_monitor_above_the_primary_one_gets_negative_screen_coordinates() {
        let local = Rect { left: 100, top: 50, right: 300, bottom: 250 };
        let screen = to_screen(local, (-320, -1440));
        assert_eq!(screen, Rect { left: -220, top: -1390, right: -20, bottom: -1190 });
        assert_eq!((screen.width(), screen.height()), (local.width(), local.height()));
    }

    /// A 3×2 frame, bottom-up BGRA: the first row in memory is the bottom row of the picture.
    /// Picture, top row: red, green, blue. Bottom row: white, black, grey. Alpha is junk (GDI leaves 0).
    fn frame_3x2() -> Vec<u8> {
        vec![
            // bottom row: white, black, grey
            255, 255, 255, 0, 0, 0, 0, 0, 128, 128, 128, 0, //
            // top row: red, green, blue (BGRA order)
            0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0,
        ]
    }

    #[test]
    fn the_whole_frame_comes_out_top_down_as_opaque_rgba() {
        let all = Rect { left: 0, top: 0, right: 3, bottom: 2 };
        let rgba = crop_to_rgba(&frame_3x2(), 3, 2, all).unwrap();
        assert_eq!(
            rgba,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, // top: red, green, blue
                255, 255, 255, 255, 0, 0, 0, 255, 128, 128, 128, 255, // bottom: white, black, grey
            ]
        );
    }

    #[test]
    fn a_crop_takes_only_the_pixels_inside_the_region() {
        // The right-hand column of both rows: blue over grey.
        let right_column = Rect { left: 2, top: 0, right: 3, bottom: 2 };
        assert_eq!(
            crop_to_rgba(&frame_3x2(), 3, 2, right_column).unwrap(),
            vec![0, 0, 255, 255, 128, 128, 128, 255]
        );
        // The bottom row's middle pixel alone: black.
        let one = Rect { left: 1, top: 1, right: 2, bottom: 2 };
        assert_eq!(crop_to_rgba(&frame_3x2(), 3, 2, one).unwrap(), vec![0, 0, 0, 255]);
    }

    #[test]
    fn a_region_outside_the_frame_or_a_short_frame_crops_to_nothing() {
        let frame = frame_3x2();
        assert_eq!(crop_to_rgba(&frame, 3, 2, Rect { left: 0, top: 0, right: 4, bottom: 2 }), None);
        assert_eq!(crop_to_rgba(&frame, 3, 2, Rect { left: -1, top: 0, right: 2, bottom: 2 }), None);
        assert_eq!(crop_to_rgba(&frame[..frame.len() - 1], 3, 2, Rect { left: 0, top: 0, right: 3, bottom: 2 }), None);
    }

    #[test]
    fn the_label_sits_below_and_right_of_the_pointer() {
        assert_eq!(label_origin((500, 400), (90, 24), 16, 1920, 1200), (516, 416));
    }

    #[test]
    fn the_label_flips_to_the_left_and_above_where_it_would_leave_the_monitor() {
        // Near the right edge: left of the pointer, same height. Near the bottom: above, same column.
        assert_eq!(label_origin((1900, 400), (90, 24), 16, 1920, 1200), (1794, 416));
        assert_eq!(label_origin((500, 1190), (90, 24), 16, 1920, 1200), (516, 1150));
        // The bottom-right corner flips both.
        assert_eq!(label_origin((1919, 1199), (90, 24), 16, 1920, 1200), (1813, 1159));
    }

    #[test]
    fn the_label_stops_flipping_the_moment_it_fits() {
        // 1920 - 90 = 1830 is the last x where the label still fits; the pointer is 16 left of that.
        assert_eq!(label_origin((1814, 0), (90, 24), 16, 1920, 1200), (1830, 16));
        assert_eq!(label_origin((1815, 0), (90, 24), 16, 1920, 1200), (1709, 16));
    }

    #[test]
    fn the_size_readout_shows_physical_width_times_height() {
        let region = drag_rect((100, 100), (1379, 819), 2560, 1440);
        assert_eq!(size_text(region), "1280 \u{d7} 720");
    }

    #[test]
    fn the_dimensions_ride_after_the_pixels_without_moving_them() {
        let rgba = crop_to_rgba(&frame_3x2(), 3, 2, Rect { left: 0, top: 0, right: 3, bottom: 2 }).unwrap();
        let pixels = rgba.as_ptr();
        let sent = with_dimensions(rgba, 3, 2);
        assert_eq!(sent.as_ptr(), pixels, "the 24 pixel bytes were not copied again");
        assert_eq!(sent.len(), 3 * 2 * 4 + DIMENSIONS_BYTES);
        assert_eq!(&sent[24..], &[3, 0, 0, 0, 2, 0, 0, 0]);
    }

    const CHROME: Chrome = Chrome { frame: (16, 39), hint: 28, min_inner_width: 300 };
    const WORK: Rect = Rect { left: 0, top: 0, right: 1920, bottom: 1160 };

    #[test]
    fn a_region_that_fits_opens_at_100_percent_centred_on_the_work_area() {
        let p = place_editor((800, 600), WORK, CHROME);
        assert_eq!(p.inner, (800, 628), "the image plus the hint line, one image pixel per screen pixel");
        // Outer is 816×667: (1920-816)/2 and (1160-667)/2.
        assert_eq!(p.position, (552, 246));
    }

    #[test]
    fn a_region_exactly_the_size_of_the_room_is_not_scaled() {
        // 90% of 1920 is 1728, minus the frame 1712; 90% of 1160 is 1044, minus 39 and 28 is 977.
        assert_eq!(place_editor((1712, 977), WORK, CHROME).inner, (1712, 1005));
        assert_eq!(place_editor((1713, 977), WORK, CHROME).inner.0, 1712, "one pixel wider is scaled");
    }

    #[test]
    fn a_region_too_wide_is_scaled_down_to_the_width_it_has_room_for() {
        // A whole 3840×2160 monitor onto 1920×1160: the width binds (1712/3840 < 977/2160).
        let p = place_editor((3840, 2160), WORK, CHROME);
        assert_eq!(p.inner, (1712, 963 + 28));
        let outer = (p.inner.0 + CHROME.frame.0, p.inner.1 + CHROME.frame.1);
        assert!(outer.0 <= 1728 && outer.1 <= 1044, "within 90% of the work area: {outer:?}");
    }

    #[test]
    fn a_region_too_tall_is_scaled_down_to_the_height_it_has_room_for() {
        let p = place_editor((600, 2000), WORK, CHROME);
        assert_eq!(p.inner, (300, 977 + 28), "the image is 293 wide, in a canvas wide enough for the hint");
    }

    #[test]
    fn scaling_down_keeps_the_aspect_ratio_to_within_a_pixel() {
        let (w, h) = (3000, 1111);
        let p = place_editor((w, h), WORK, CHROME);
        let shown = (p.inner.0, p.inner.1 - CHROME.hint);
        assert!((shown.1 as f64 - shown.0 as f64 * h as f64 / w as f64).abs() < 1.0, "{shown:?}");
    }

    #[test]
    fn a_work_area_above_the_primary_monitor_centres_in_negative_coordinates() {
        // The 2560×1440 above the laptop, its taskbar-less work area at y = -1440.
        let work = Rect { left: -320, top: -1440, right: 2240, bottom: 0 };
        let p = place_editor((1000, 500), work, CHROME);
        assert_eq!(p.inner, (1000, 528));
        // Outer 1016×567: -320 + (2560-1016)/2 and -1440 + (1440-567)/2.
        assert_eq!(p.position, (452, -1440 + 436));
        assert!(p.position.1 < 0);
    }

    #[test]
    fn a_work_area_to_the_left_of_the_primary_monitor_centres_in_negative_x() {
        let work = Rect { left: -1920, top: 0, right: 0, bottom: 1040 };
        let p = place_editor((500, 400), work, CHROME);
        assert_eq!(p.position, (-1920 + (1920 - 516) / 2, (1040 - 467) / 2));
    }

    #[test]
    fn a_small_region_gets_a_window_wide_enough_for_the_hint_line_and_is_centred_by_it() {
        let p = place_editor((100, 80), WORK, CHROME);
        assert_eq!(p.inner, (300, 80 + 28), "the image sits centred in the wider canvas");
        assert_eq!(p.position, (WORK.width() / 2 - 316 / 2, (1160 - (108 + 39)) / 2));
    }

    #[test]
    fn centring_uses_the_outer_size_the_window_really_has() {
        let work = Rect { left: -1920, top: -300, right: 0, bottom: 740 };
        assert_eq!(centred(work, (600, 400)), (-1920 + 660, -300 + 320));
    }

    #[test]
    fn a_tiny_work_area_still_gives_a_window_of_at_least_one_pixel() {
        let p = place_editor((100, 100), Rect { left: 0, top: 0, right: 10, bottom: 10 }, CHROME);
        assert!(p.inner.0 >= 1 && p.inner.1 > CHROME.hint);
    }

    #[test]
    fn dimming_keeps_seventy_percent_of_each_channel() {
        let table = dim_table(70);
        assert_eq!((table[0], table[100], table[200], table[255]), (0, 70, 140, 178));
    }
}
