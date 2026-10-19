//! The clipboard write (spec §9, ANNOTA-23): the registered `"PNG"` format and `CF_DIBV5`, byte for byte what
//! `arboard::set_image` wrote (32 bpp `BI_BITFIELDS`, `LCS_sRGB`, bottom-up, alpha kept), but built in one
//! pass each and in parallel.

use std::io::Cursor;

use crate::timings;

/// `BITMAPV5HEADER`, in bytes.
pub const DIBV5_HEADER: usize = 124;

/// The whole `CF_DIBV5` block for a `width`×`height` image: the header, then the pixels.
pub fn dibv5_len(width: usize, height: usize) -> usize {
    DIBV5_HEADER + width * height * 4
}

/// Writes the `CF_DIBV5` block for top-down `rgba` into `out` (`dibv5_len` bytes): the header, then the rows
/// bottom-up as BGRA. A negative height would mean top-down, but Word and WordPad can't paste that, so the
/// rows are flipped here, in the same pass as the swizzle.
pub fn write_dibv5(width: usize, height: usize, rgba: &[u8], out: &mut [u8]) {
    assert_eq!(rgba.len(), width * height * 4);
    assert_eq!(out.len(), dibv5_len(width, height));
    let (header, pixels) = out.split_at_mut(DIBV5_HEADER);
    write_header(width, height, header);
    let row = width * 4;
    for (y, src_row) in rgba.chunks_exact(row).enumerate() {
        let dst_row = &mut pixels[(height - 1 - y) * row..][..row];
        for (dst, src) in dst_row.chunks_exact_mut(4).zip(src_row.chunks_exact(4)) {
            dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
        }
    }
}

fn write_header(width: usize, height: usize, h: &mut [u8]) {
    const LCS_SRGB: u32 = 0x7352_4742;
    const LCS_GM_IMAGES: u32 = 4;
    const BI_BITFIELDS: u32 = 3;
    h.fill(0);
    let mut put = |at: usize, value: u32| h[at..at + 4].copy_from_slice(&value.to_le_bytes());
    put(0, DIBV5_HEADER as u32);
    put(4, width as u32);
    put(8, height as u32);
    put(12, 1 | (32 << 16)); // planes, bit count
    put(16, BI_BITFIELDS);
    put(20, (width * height * 4) as u32);
    put(40, 0x00ff_0000); // red mask
    put(44, 0x0000_ff00);
    put(48, 0x0000_00ff);
    put(52, 0xff00_0000); // alpha mask
    put(56, LCS_SRGB);
    put(108, LCS_GM_IMAGES);
}

/// How hard the PNG is compressed: the clipboard write waits for it.
// ANNOTA-23: only a timings build picks between the candidates; the loser goes once one is chosen.
#[cfg_attr(not(feature = "timings"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngLevel {
    /// Stored deflate blocks: no compression, the PNG is as big as the pixels.
    Stored,
    /// `fdeflate`, the fastest real compression.
    Fast,
}

/// `rgba` as an 8-bit RGBA PNG with no colour chunks, as `arboard` wrote it.
pub fn encode_png(width: usize, height: usize, rgba: &[u8], level: PngLevel) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::with_capacity(match level {
        PngLevel::Stored => rgba.len() + rgba.len() / 100 + 1024,
        PngLevel::Fast => rgba.len() / 8,
    }));
    let mut encoder = png::Encoder::new(&mut out, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(match level {
        PngLevel::Stored => png::Compression::NoCompression,
        PngLevel::Fast => png::Compression::Fast,
    });
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Who writes the clipboard.
#[cfg_attr(not(feature = "timings"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    /// `arboard::set_image`, as v1 shipped.
    Arboard,
    /// Our own `"PNG"` and `CF_DIBV5`, the PNG encoded on a second thread while the DIB is filled.
    Own(PngLevel),
}

/// Puts `rgba` (top-down, opaque) on the clipboard as `"PNG"` then `CF_DIBV5`, replacing what was there.
pub fn set_image(width: usize, height: usize, rgba: &[u8], writer: Writer) -> Result<(), String> {
    match writer {
        Writer::Arboard => arboard::Clipboard::new()
            .and_then(|mut clipboard| {
                clipboard.set_image(arboard::ImageData { width, height, bytes: std::borrow::Cow::Borrowed(rgba) })
            })
            .map_err(|e| e.to_string()),
        Writer::Own(level) => set_image_own(width, height, rgba, level),
    }
}

fn set_image_own(width: usize, height: usize, rgba: &[u8], level: PngLevel) -> Result<(), String> {
    let (png, dib) = std::thread::scope(|scope| {
        let png = scope.spawn(|| {
            let png = encode_png(width, height, rgba, level);
            timings::mark("png_done");
            png
        });
        let dib = win::Block::new(dibv5_len(width, height)).map(|mut block| {
            write_dibv5(width, height, rgba, block.bytes());
            timings::mark("dib_done");
            block
        });
        (png.join().unwrap_or_else(|_| Err("the PNG encoder panicked".into())), dib)
    });
    let png_block = win::Block::new(png.as_ref().map_or(0, Vec::len)).map(|mut block| {
        block.bytes().copy_from_slice(png.as_deref().unwrap_or_default());
        block
    });
    let (dib, png_block) = (dib?, png_block?);
    png?;
    win::publish(png_block, dib)
}

mod win {
    use std::time::Duration;

    use windows::core::w;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    const CF_DIBV5: u32 = 17;
    /// Another process may hold the clipboard for a moment; Chromium and `arboard` retry the same way.
    const OPEN_ATTEMPTS: u32 = 5;

    /// A moveable global memory block, locked while we fill it and freed unless the clipboard took it.
    pub struct Block {
        handle: HGLOBAL,
        ptr: *mut u8,
        len: usize,
    }

    impl Block {
        pub fn new(len: usize) -> Result<Block, String> {
            // SAFETY: a plain allocation, locked at once; `Drop` or `give` releases both.
            unsafe {
                let handle = GlobalAlloc(GMEM_MOVEABLE, len.max(1)).map_err(|e| format!("GlobalAlloc: {e}"))?;
                let ptr = GlobalLock(handle).cast::<u8>();
                if ptr.is_null() {
                    let _ = GlobalFree(Some(handle));
                    return Err("GlobalLock failed".into());
                }
                Ok(Block { handle, ptr, len })
            }
        }

        pub fn bytes(&mut self) -> &mut [u8] {
            // SAFETY: `ptr` points at `len` bytes of the locked block, which lives as long as `self`.
            unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
        }
    }

    impl Drop for Block {
        fn drop(&mut self) {
            if !self.handle.is_invalid() {
                // SAFETY: still ours: `give` empties `handle` once the clipboard owns it.
                unsafe {
                    let _ = GlobalUnlock(self.handle);
                    let _ = GlobalFree(Some(self.handle));
                }
            }
        }
    }

    /// Opens the clipboard, empties it, and sets `"PNG"` then `CF_DIBV5`.
    pub fn publish(mut png: Block, mut dib: Block) -> Result<(), String> {
        let mut attempts = OPEN_ATTEMPTS;
        // SAFETY: plain clipboard calls on this thread; every open is paired with `CloseClipboard`.
        unsafe {
            while let Err(e) = OpenClipboard(None) {
                if attempts == 0 {
                    return Err(format!("OpenClipboard: {e}"));
                }
                attempts -= 1;
                std::thread::sleep(Duration::from_millis(5));
            }
            let result = (|| {
                EmptyClipboard().map_err(|e| format!("EmptyClipboard: {e}"))?;
                let png_format = RegisterClipboardFormatW(w!("PNG"));
                if png_format == 0 {
                    return Err("cannot register the PNG clipboard format".to_string());
                }
                give(png_format, &mut png)?;
                give(CF_DIBV5, &mut dib)
            })();
            let _ = CloseClipboard();
            result
        }
    }

    /// Hands `block` to the clipboard; on success it is the clipboard's, not ours to free.
    unsafe fn give(format: u32, block: &mut Block) -> Result<(), String> {
        unsafe {
            let _ = GlobalUnlock(block.handle);
            SetClipboardData(format, Some(HANDLE(block.handle.0))).map_err(|e| format!("SetClipboardData: {e}"))?;
        }
        block.handle = HGLOBAL::default();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    /// A 3×2 image whose every byte tells which pixel and channel it is.
    fn image() -> Vec<u8> {
        (0..3 * 2).flat_map(|p| [p * 10 + 1, p * 10 + 2, p * 10 + 3, 255]).collect()
    }

    fn dibv5(width: usize, height: usize, rgba: &[u8]) -> Vec<u8> {
        let mut out = vec![0xAA; dibv5_len(width, height)];
        write_dibv5(width, height, rgba, &mut out);
        out
    }

    #[test]
    fn the_header_is_what_arboard_wrote() {
        let out = dibv5(3, 2, &image());
        assert_eq!(le32(&out, 0), 124, "bV5Size");
        assert_eq!(le32(&out, 4), 3, "bV5Width");
        assert_eq!(le32(&out, 8), 2, "bV5Height is positive: bottom-up");
        assert_eq!(&out[12..16], &[1, 0, 32, 0], "one plane, 32 bits");
        assert_eq!(le32(&out, 16), 3, "BI_BITFIELDS");
        assert_eq!(le32(&out, 20), 3 * 2 * 4, "bV5SizeImage");
        assert_eq!(&out[24..40], &[0; 16], "no resolution, no palette");
        assert_eq!(le32(&out, 40), 0x00ff_0000, "red mask");
        assert_eq!(le32(&out, 44), 0x0000_ff00, "green mask");
        assert_eq!(le32(&out, 48), 0x0000_00ff, "blue mask");
        assert_eq!(le32(&out, 52), 0xff00_0000, "alpha mask");
        assert_eq!(le32(&out, 56), 0x7352_4742, "LCS_sRGB");
        assert_eq!(&out[60..108], &[0; 48], "no endpoints or gamma");
        assert_eq!(le32(&out, 108), 4, "LCS_GM_IMAGES");
        assert_eq!(&out[112..124], &[0; 12], "no profile");
    }

    #[test]
    fn the_rows_are_bottom_up_and_the_pixels_bgra() {
        let out = dibv5(3, 2, &image());
        let pixels = &out[DIBV5_HEADER..];
        // The top row is pixels 0..3 (RGBA 1,2,3 / 11,12,13 / 21,22,23); it comes last, each as B,G,R,A.
        assert_eq!(&pixels[12..16], &[3, 2, 1, 255]);
        assert_eq!(&pixels[16..20], &[13, 12, 11, 255]);
        assert_eq!(&pixels[20..24], &[23, 22, 21, 255]);
        // The bottom row (pixels 3..6) comes first.
        assert_eq!(&pixels[0..4], &[33, 32, 31, 255]);
        assert_eq!(&pixels[8..12], &[53, 52, 51, 255]);
    }

    #[test]
    fn a_translucent_pixel_keeps_its_alpha() {
        let out = dibv5(1, 1, &[10, 20, 30, 40]);
        assert_eq!(&out[DIBV5_HEADER..], &[30, 20, 10, 40]);
    }

    fn decode(png: &[u8]) -> (png::OutputInfo, Vec<u8>, bool) {
        let decoder = png::Decoder::new(Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let colour_chunks = {
            let info = reader.info();
            info.gama_chunk.is_some() || info.icc_profile.is_some() || info.srgb.is_some() || info.chrm_chunk.is_some()
        };
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        pixels.truncate(info.buffer_size());
        (info, pixels, colour_chunks)
    }

    #[test]
    fn a_png_round_trips_at_every_level() {
        // Enough rows, with flat runs and noise, to exercise the compressor.
        let (width, height) = (37, 29);
        let rgba: Vec<u8> = (0..width * height)
            .flat_map(|p| if p % 5 == 0 { [(p * 7) as u8, (p * 3) as u8, p as u8, 255] } else { [9, 9, 9, 255] })
            .collect();
        for level in [PngLevel::Stored, PngLevel::Fast] {
            let png = encode_png(width, height, &rgba, level).unwrap();
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            let (info, pixels, colour_chunks) = decode(&png);
            assert_eq!((info.width as usize, info.height as usize), (width, height), "{level:?}");
            assert_eq!(info.color_type, png::ColorType::Rgba);
            assert_eq!(info.bit_depth, png::BitDepth::Eight);
            assert_eq!(pixels, rgba, "{level:?}");
            assert!(!colour_chunks, "{level:?}: opaque sRGB, no ICC or gamma");
        }
    }

    #[test]
    fn fast_compression_shrinks_a_flat_image() {
        let rgba = [200u8, 30, 30, 255].repeat(256 * 256);
        let stored = encode_png(256, 256, &rgba, PngLevel::Stored).unwrap();
        let fast = encode_png(256, 256, &rgba, PngLevel::Fast).unwrap();
        assert!(stored.len() > rgba.len(), "stored blocks are as big as the pixels");
        assert!(fast.len() < rgba.len() / 20, "{} bytes", fast.len());
    }
}
