# Which Rust capture path freezes every monitor fastest at native pixels?

Research for Traccia **ANNOTA-4** (map ANNOTA-1). Written 2026-10-19. No timed
measurements were taken (shared-machine rule). Latencies below are **expected
ranges** from docs, crate source and published benchmarks; they are labelled
"estimate" and must be measured later (see the last section).

Machine: Windows 11 Pro 26200, i7-1370P + Iris Xe (single iGPU, no hybrid
graphics), laptop 1920×1200 primary plus 2560×1440 above it at negative Y,
mixed DPI.

## Answer

**Primary: GDI `BitBlt` straight from the screen DC, called with the `windows`
crate inside the already-running per-monitor-v2 Tauri process, once per monitor,
into a `CreateDIBSection` buffer, `SRCCOPY` without `CAPTUREBLT`.**
There's no setup cost, no border, no prompt and no cursor. Layered windows,
menus and tooltips come from the DWM-composed desktop. It takes about 80 lines
of code and needs no extra crate. Estimate: **about 10–40 ms for both monitors**
(about 6.0 MP).

**Fallback: DXGI Desktop Duplication (`IDXGIOutput1::DuplicateOutput`, or
`DuplicateOutput1` if HDR ever matters).** Keep the D3D11 device pre-created.
On each Snap, create one duplication per output, take the first frame with
`AcquireNextFrame`, then `CopyResource` and `Map`. Switch to it only if `BitBlt`
measures too slow or misses content. The `windows-capture` crate's
`DxgiDuplicationApi` is a ready-made reference.

**Don't use Windows.Graphics.Capture for the freeze.** It's built for
streaming. Each capture needs a session and waits for the next DWM frame. The
docs say turning off the yellow border needs a consent prompt plus a
package-manifest capability. Don't use `xcap` or `screenshots` as dependencies
either: xcap's default Windows path is the same `BitBlt` with extra copies, and
`screenshots` is deprecated in favour of xcap.

## Comparison

| | GDI `BitBlt` (own code) | DXGI Desktop Duplication | Windows.Graphics.Capture | `xcap` 0.9.8 | `screenshots` / `win-screenshot` |
|---|---|---|---|---|---|
| What it is | Read the DWM-composed desktop through the screen DC | Per-output GPU surface of the composed desktop | WinRT capture session per monitor (`CreateForMonitor`) | Default build: GDI `BitBlt` per monitor; `wgc` feature: WGC | `screenshots`: deprecated, "Move to XCap". `win-screenshot`: one `StretchBlt` of the whole virtual screen |
| Setup per Snap | None | `DuplicateOutput` ×2 (+ D3D device unless pre-warmed) | Frame pool + session + `StartCapture` ×2 | As GDI / as WGC | None |
| Grab latency, both monitors (estimate) | ~10–40 ms | ~10–40 ms warm device, on-demand duplications; +50–150 ms if the device is cold | ~30–100 ms (waits for the next DWM frame per session) | GDI path + about 5–15 ms of extra copies | As GDI |
| Physical pixels on mixed DPI | Yes, **only** if the calling thread is per-monitor aware (Tauri sets PMv2) | Yes (per output). `windows-capture` forces PMv2 before `DuplicateOutput1` | Yes (per monitor) | Yes if PMv2 (uses `EnumDisplaySettings` pixel sizes) | `win-screenshot` doesn't set awareness itself |
| Negative coordinates | Fine: screen-DC coordinates are virtual-desktop coordinates | N/A: each output's image starts at its own origin | N/A: per HMONITOR | Fine (`dmPosition`) | Fine (`SM_XVIRTUALSCREEN`) |
| Cursor | Not included | Separate: you draw it, or not | Included by default; `IsCursorCaptureEnabled = false` turns it off | No (GDI) / off (WGC) | No |
| Layered windows, menus, tooltips | Yes from the DWM composition (`CAPTUREBLT` only needed without DWM, and it makes the cursor flicker) | Yes (whole composed output) | Yes (whole composed monitor) | Same as its backend | Same as GDI |
| HDR / colour | 8-bit only. If HDR is on, expect clipped or washed-out SDR | `DuplicateOutput`: always BGRA8. `DuplicateOutput1`: FP16 possible (then you tone-map) | FP16 possible (then you tone-map) | 8-bit | 8-bit |
| Border / prompt | None | None | Yellow border unless `IsBorderRequired = false`, which per the docs needs a `RequestAccessAsync(Borderless)` prompt and a package capability | xcap tries to disable it, best effort | None |
| Failure modes | Secure desktop (UAC) not readable | `E_ACCESSDENIED` on the secure desktop; `ACCESS_LOST` on mode or desktop switch; at most 4 duplicating processes per session | Border flash if borderless isn't granted | n/a | n/a |

## Findings, with sources

### DPI awareness and physical pixels

- Per-monitor v2 means "the application seeing the raw pixels of each display".
  For unaware or system-aware threads, "Windows will virtualize the answer", and
  Microsoft says which APIs get virtualized "is not currently sufficiently
  documented", so "make sure your thread is running in the DPI context you
  expect when interacting with the screen".
  [High DPI Desktop Application Development](https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows)
- **Tauri already runs per-monitor v2.** `tao` calls
  `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`
  (falling back to PMv1) inside `become_dpi_aware()`, which runs when the event
  loop is created (`tao/src/platform_impl/windows/dpi.rs:20`, called from
  `event_loop.rs:185`, tao main @ 2026-10). Tauri's default embedded manifest
  (`tauri-build/src/windows-app-manifest.xml`) declares only Common-Controls v6
  and **no** DPI awareness. So awareness is set at run time, not in the manifest.
  Consequence for Annota: capture runs in the resident process after the event
  loop exists, so it is PMv2. The short-lived `--snap` process forwards
  the request and never captures, so its awareness doesn't matter. Optionally
  add `<dpiAwareness>PerMonitorV2</dpiAwareness>` to a custom manifest so it holds
  from the process's first instruction (tao ignores the resulting
  `E_ACCESSDENIED`).
- Under PMv2, `EnumDisplayMonitors`/`GetMonitorInfo` rects and
  `EnumDisplaySettings` (`dmPosition`, `dmPelsWidth/Height`) are physical. xcap
  uses the latter for monitor geometry
  (`xcap/src/windows/impl_monitor.rs:222-243`).

### GDI BitBlt

- `CAPTUREBLT`: "Includes any windows that are layered on top of your window in
  the resulting image."
  [BitBlt](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-bitblt)
- Why `CAPTUREBLT` flickers the cursor: GDI hides the software cursor so it can
  include layered windows, which grew out of the software-cursor mechanism
  (Raymond Chen, "The Case of the Disappearing Cursor",
  [TechNet Magazine](https://technet.microsoft.com/magazine/dd392008(v=msdn.10))).
  Mozilla found that with DWM composition (always on since Windows 8) layered
  windows are captured without the flag, and dropped it to stop the flicker
  ([bug 1053264](https://bugzilla.mozilla.org/show_bug.cgi?id=1053264),
  secondary source). xcap's GDI path uses plain `SRCCOPY`
  (`xcap/src/windows/gdi.rs`, `capture_monitor`); python-mss uses
  `SRCCOPY | CAPTUREBLT`. **Verify by hand** that a tooltip, a context menu with
  a shadow and a translucent window appear with `SRCCOPY` alone.
- The cursor is never part of a `BitBlt` from the screen DC, which is right for
  an annotation tool.
- Faster shape than xcap: xcap does `GetWindowDC(GetDesktopWindow())` →
  `CreateCompatibleBitmap` → `BitBlt` → `GetDIBits` into a `Vec` → BGRA→RGBA
  pass → `RgbaImage` (`xcap/src/windows/gdi.rs`). That's three passes over
  about 24 MB. python-mss blits straight into a `CreateDIBSection` buffer
  (`python-mss/src/mss/windows/gdi.py`), which saves the `GetDIBits` copy. Keep
  BGRA if the overlay or canvas can take it.
- Published throughput: DXcam's benchmark (Ryzen 5900X + RTX 3090, resolution
  not stated, likely 1080p) gives python-mss (GDI `BitBlt`) **75.9 fps**
  (about 13 ms per grab including Python overhead) against DXcam (DXGI
  duplication) **239 fps**, which is capped by the 240 Hz test source
  ([DXcam README](https://github.com/ra1nty/DXcam)). That is streaming
  throughput, not a cold single shot. Scale it to about 6 MP on Iris Xe for the
  10–40 ms estimate. On an iGPU with unified memory the GPU→CPU readback is
  relatively cheap, which favours GDI.

### DXGI Desktop Duplication

- One duplication per output: "If an application wants to duplicate the entire
  desktop, it must create a desktop duplication interface on each active
  output". Limit: four duplicating processes per session.
  `E_ACCESSDENIED` on the secure desktop. `DXGI_ERROR_UNSUPPORTED` for 8bpp or
  non-DWM modes.
  [IDXGIOutput1::DuplicateOutput](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutput1-duplicateoutput)
- `DuplicateOutput` always returns BGRA8. `DuplicateOutput1` can hand back the
  original format (for example FP16).
  [IDXGIOutput5::DuplicateOutput1](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_5/nf-dxgi1_5-idxgioutput5-duplicateoutput1)
  Microsoft's docs don't list a DPI requirement. In practice `windows-capture`
  calls `SetProcessDpiAwarenessContext(PMv2)` right before `DuplicateOutput1`
  (`windows-capture/src/dxgi_duplication_api.rs:125-131, 221`), and a reported
  `DXGI_ERROR_UNSUPPORTED` went away under PMv2 (community report). Annota is
  PMv2 anyway.
- The pointer is delivered separately (position plus shape), unless it's
  already drawn into the image. Rotated outputs come back un-rotated.
  [Desktop Duplication API](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)
- `AcquireNextFrame` returns only when there is a *new* frame. A long-lived,
  idle duplication would time out on a static screen. `ACCESS_LOST` follows a
  desktop switch, a mode change or a full-screen app.
  [AcquireNextFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe)
  So for a single shot, either create the duplications on demand (each costs a
  `DuplicateOutput` call per Snap) or keep them alive and drain frames in the
  background (always-on GPU/CPU work, against "lean"). On-demand is the
  fallback design.
- Hybrid-GPU limitation (doesn't apply here: Iris Xe only):
  [KB 3019314](https://learn.microsoft.com/en-us/troubleshoot/windows-client/shell-experience/error-when-dda-capable-app-is-against-gpu).

### Windows.Graphics.Capture

- "A yellow notification border is drawn by the system around the actively
  captured item". Frames arrive through `FrameArrived` or `TryGetNextFrame`
  after `StartCapture`. If HD colour is on, use FP16 to avoid "washed out"
  captures.
  [Screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)
- `IsBorderRequired` (build 20348+): before the system drops the border "your
  app must get consent from the user by calling
  `GraphicsCaptureAccess.RequestAccessAsync` ... `Borderless`, which displays a
  prompt", and must declare `graphicsCaptureWithoutBorder` "in your app's
  package manifest". If the user refuses, the border stays.
  [IsBorderRequired](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired),
  [RequestAccessAsync](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscaptureaccess.requestaccessasync).
  Annota is an unpackaged Tauri exe. xcap's WGC path calls
  `SetIsBorderRequired(false)` as "best effort" and keeps going if it fails
  (`xcap/src/windows/wgc.rs`, `wgc_capture`). Whether an unpackaged app gets
  borderless silently on build 26200 is unverified, and a border flashing
  across both monitors on every Snap is a deal-breaker.
- xcap's WGC path builds a new frame pool and session for every capture and
  blocks on the first `FrameArrived` (3 s timeout). That's per monitor, so two
  sessions per Snap.

### Crates

- `xcap` 0.9.8 (crates.io, updated 2026-08-01): default Windows monitor capture
  is GDI, and the `wgc` feature switches to WGC (`xcap/src/windows/capture.rs`).
  It pulls in `image` and converts to `RgbaImage`. Useful reference, but an
  unnecessary dependency.
- `windows-capture` 2.0.1 (2026-08-08): WGC streaming plus a `DxgiDuplicationApi`
  (`DuplicateOutput1` with FP16/RGBA8/BGRA8 formats). Pulls in rayon. Good
  reference for the DXGI fallback.
- `screenshots` 0.8.10: description is "Move to XCap". Dead.
- `win-screenshot` 4.0.14: `capture_display()` does
  `GetDC(NULL)` + `GetSystemMetrics(SM_*VIRTUALSCREEN)` + one `StretchBlt` of the
  whole virtual screen with `SRCCOPY`, RGB output, and doesn't set DPI awareness
  (`win-screenshot/src/capture.rs:165-206`). It has no per-monitor slicing.

### Other things that touch every path

- Windows that set `WDA_EXCLUDEFROMCAPTURE` / `WDA_MONITOR` are missing or black
  in captures; this only works while DWM composes.
  [SetWindowDisplayAffinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity)
  This is expected behaviour, not something to work around.
- HDR: only `DuplicateOutput1` and WGC can capture HDR without loss, and both
  then need tone mapping for an SDR PNG.
  [Advanced Color](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range)
  Mattia's panels are assumed SDR; check Settings > Display > HDR. v1 should
  treat HDR as out of scope and accept 8-bit.

## Recommended shape (for the spec, not code)

1. On the single-instance callback, **before showing any window**, on a worker
   thread (it inherits the process's PMv2 context):
   `EnumDisplayMonitors` → for each HMONITOR, `GetMonitorInfoW` (physical
   `rcMonitor`) → `BitBlt(dib_dc, 0, 0, w, h, screen_dc, rc.left, rc.top,
   SRCCOPY)` into a per-monitor `CreateDIBSection` (top-down BGRA). The
   monitors are read back to back, so both images are effectively the same
   instant.
2. Keep the buffers as BGRA. The selected region is cropped from its monitor's
   buffer at native pixels.
3. Fallback switch: on-demand DXGI duplication per output with a device that's
   created at startup and kept.

## What to measure later (in an assigned machine-lock turn)

Use QPC timestamps in the resident process: t0 = single-instance callback, t1 =
each monitor's buffer filled, t2 = all monitors done. Do 20 runs per path
(GDI `SRCCOPY`, GDI `SRCCOPY|CAPTUREBLT`, DXGI on-demand with a warm device,
optionally xcap) in each of four states: idle desktop, a video playing, on AC
and on battery. Iris Xe power states may dominate.

Correctness checks by hand:
- Output size is exactly 1920×1200 and 2560×1440, with no scaling blur.
- The monitor above (negative Y) is captured fully, with no offset.
- An open context menu, a tooltip, a hover state and a translucent window are
  all present.
- No cursor is in the image.
- The cursor doesn't flicker during the grab (only the `CAPTUREBLT` variant
  should flicker).
- Colours match the screen.
- A Snap while UAC is up fails cleanly.
