//! The grab (spec §4): every monitor, frozen at its native physical pixels, before any Annota window shows.
//!
//! Per monitor, a 32 bpp BGRA `CreateDIBSection` the size of `rcMonitor`, filled with one `BitBlt` from the
//! screen DC. `SRCCOPY` only: `CAPTUREBLT` would make the cursor flicker, and with DWM composing the
//! desktop it isn't needed to keep layered windows, menus and tooltips. The cursor is never part of the
//! image. The process is per-monitor-v2 aware (tao sets it), so every rect here is physical.
//!
//! GDI objects belong to the thread that made them in practice: the grab, the Overlay and the drop all
//! happen on Tauri's main thread. Only the cropped RGBA [`Region`] leaves it.

use std::ffi::c_void;
use std::ptr;

use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EnumDisplayMonitors, GdiFlush, GetDC,
    GetMonitorInfoW, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    HDC, HGDIOBJ, HMONITOR, MONITORINFO, SRCCOPY,
};

use crate::geometry::{self, Rect};

/// A 32 bpp BGRA bitmap with a memory DC it is selected into. The rows are bottom-up (positive
/// `biHeight`), which is what [`geometry::crop_to_rgba`] expects.
pub struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    pub width: i32,
    pub height: i32,
}

impl Dib {
    pub fn new(width: i32, height: i32) -> Result<Dib, String> {
        if width <= 0 || height <= 0 {
            return Err(format!("cannot make a {width}x{height} bitmap"));
        }
        // SAFETY: plain GDI calls; every handle made here is owned by the returned `Dib` or released on
        // the error paths.
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err("CreateCompatibleDC failed".into());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) if !bits.is_null() => bitmap,
                result => {
                    if let Ok(bitmap) = result {
                        let _ = DeleteObject(bitmap.into());
                    }
                    let _ = DeleteDC(dc);
                    return Err(format!("CreateDIBSection {width}x{height} failed"));
                }
            };
            let previous = SelectObject(dc, bitmap.into());
            Ok(Dib { dc, bitmap, previous, bits: bits.cast(), width, height })
        }
    }

    /// The memory DC to blit from or into.
    pub fn dc(&self) -> HDC {
        self.dc
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `bits` points at the `width * height * 4` bytes `CreateDIBSection` allocated, which live
        // as long as `bitmap`, which `self` owns.
        unsafe { std::slice::from_raw_parts(self.bits, self.len()) }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in `bytes`, and `&mut self` makes this the only slice.
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.len()) }
    }

    fn len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: the bitmap is deselected before it is deleted, then its DC goes.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// One monitor's frozen frame.
pub struct MonitorCapture {
    /// The monitor's physical rect on the virtual desktop; `top` is negative for a monitor above the primary.
    pub rect: Rect,
    pub frame: Dib,
}

impl MonitorCapture {
    /// The Region at `local` (relative to this monitor), cropped at native pixels.
    pub fn crop(&self, local: Rect) -> Option<Region> {
        let rgba = geometry::crop_to_rgba(self.frame.bytes(), self.frame.width, self.frame.height, local)?;
        Some(Region {
            screen: geometry::to_screen(local, (self.rect.left, self.rect.top)),
            monitor: self.rect,
            rgba,
        })
    }
}

/// What the user dragged: handed from the Overlay to whoever shows it next (the Editor, ANNOTA-16).
pub struct Region {
    /// Physical rect on the virtual desktop.
    pub screen: Rect,
    /// The physical rect of the monitor it was dragged on.
    pub monitor: Rect,
    /// Top-down RGBA, alpha 255, `width() * height() * 4` bytes.
    pub rgba: Vec<u8>,
}

impl Region {
    pub fn width(&self) -> usize {
        self.screen.width() as usize
    }

    pub fn height(&self) -> usize {
        self.screen.height() as usize
    }
}

/// Re-reads the monitors and freezes each one. Monitors change during the day (dock, lid), so nothing is
/// cached between Snaps.
pub fn grab_all() -> Result<Vec<MonitorCapture>, String> {
    let rects = monitor_rects()?;
    // SAFETY: the screen DC is released before returning; each blit targets a `Dib` made for it.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("GetDC(screen) failed".into());
        }
        let grabbed = rects
            .into_iter()
            .map(|rect| {
                let frame = Dib::new(rect.width(), rect.height())?;
                BitBlt(frame.dc(), 0, 0, frame.width, frame.height, Some(screen), rect.left, rect.top, SRCCOPY)
                    .map_err(|e| format!("BitBlt of {rect:?} failed: {e}"))?;
                Ok(MonitorCapture { rect, frame })
            })
            .collect();
        // GDI batches its calls; the DIBs' memory is read directly from here on.
        let _ = GdiFlush();
        ReleaseDC(None, screen);
        grabbed
    }
}

/// Every monitor's `rcMonitor`, in physical pixels.
fn monitor_rects() -> Result<Vec<Rect>, String> {
    unsafe extern "system" fn collect(monitor: HMONITOR, _dc: HDC, _rect: *mut RECT, data: LPARAM) -> windows::core::BOOL {
        // SAFETY: `data` is the `&mut Vec<Rect>` passed below, alive for the whole enumeration.
        let rects = unsafe { &mut *(data.0 as *mut Vec<Rect>) };
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: `info` is a valid MONITORINFO with its size set.
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            let r = info.rcMonitor;
            rects.push(Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom });
        }
        true.into()
    }

    let mut rects: Vec<Rect> = Vec::new();
    // SAFETY: the callback only runs during this call, with the pointer to `rects`.
    let ok = unsafe { EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut rects as *mut _ as isize)) };
    if !ok.as_bool() || rects.is_empty() {
        return Err("EnumDisplayMonitors found no monitor".into());
    }
    Ok(rects)
}
