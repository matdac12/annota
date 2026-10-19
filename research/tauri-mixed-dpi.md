# Tauri windows on Mattia's mixed-DPI monitors (ANNOTA-5)

Research for Traccia ANNOTA-5. It is not a spec and this branch is never merged.
Sources are primary where possible: the tao 0.37.1, wry 0.57.0, tauri 2.12.4 and
tauri-plugin-single-instance 2.5.4 sources (the versions Parla locks today), the
Microsoft Learn docs, and Tauri's GitHub issues. Parla
(`Documents\parla\src-tauri\src\overlay.rs`, `tray.rs`) is the local reference:
a Tauri v2 tray app by the same author on the same machine. No timed measurements
were taken. The numbers quoted below come from Parla's README, measured earlier
on this machine.

## Answer in brief

- **Selection overlay: plain Win32, not WebView.** Make one borderless, topmost
  `WS_POPUP` per monitor, created by Rust on Tauri's main thread at the
  monitor's exact physical rect. Paint the frozen frame with `BitBlt` from the
  capture's DIB, and draw the crosshair, rectangle and size readout with GDI or
  Direct2D. Nothing is scaled and nothing is copied into a browser. Show the
  windows only after they have painted. Create them for each Snap (it costs
  little) or cache them keyed by the monitor layout.
- **Focus:** call `SetForegroundWindow` yourself, not tao's `set_focus`, which
  falls back to a synthetic Alt press. While the overlay is up, register Esc
  with `RegisterHotKey` so cancelling works even if Windows refused focus. The
  first mouse click on the overlay activates it anyway. The F13 `RegisterHotKey`
  fallback in ANNOTA-2 is also the more reliable trigger for focus.
- **Editor: a pre-warmed hidden Tauri `WebviewWindow`.** For each Snap, while
  it is hidden: `set_position(Physical)` first, then `set_size(Physical)`, then
  hand over the image, wait for the page to say it has painted, then
  `show()` + `SetForegroundWindow`. Size the canvas from
  `ResizeObserver`'s `device-pixel-content-box` so one image pixel is one
  device pixel. If the window is dragged to the other monitor, Windows and tao
  keep its *logical* size. WebView2 updates `devicePixelRatio` on its own, and
  the page must redo the layout or the image is resampled.
- **Pre-warm cost:** about 55–75 MB private for one hidden WebView renderer,
  plus the shared WebView2 browser and GPU processes, which Annota pays once
  anyway (Parla's figures). Unless the webview itself is also hidden, an idle
  hidden window keeps compositing at 60 fps (Parla saw about 15 % of a core).
  Show-to-first-paint for a warm hidden webview has not been measured. Parla's
  *cold* flyout (create + load + show) takes about 130 ms median.

## Machine facts and an open question

- Windows 11 Pro 26200, i7-1370P. The stated layout is a laptop 1920×1200
  (primary) with a 2560×1440 above it at negative y.
- When this research ran, only one display was attached: an LG HDR 4K at
  3840×2160, 144 dpi (150 %). So the layout changes during the day (dock,
  lid). The design must re-read monitors at every Snap, never just at startup.
- `HKCU\Control Panel\Desktop\PerMonitorSettings` holds `DpiValue=-2`
  (two steps below Windows' recommended scale) for the Lenovo panel `LEN403B`,
  and `0` (recommended) for the others. **The real scale of each monitor is
  unconfirmed.** If the laptop sits at 100 % and the 1440p at its recommended
  100 %, the setup isn't mixed-DPI at all, but the 4K at 150 % is. The design
  below handles any mix.

## 1. Selection overlay

### Coordinates and DPI awareness

- tao makes the process Per-Monitor-V2 aware at startup
  (`SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`,
  falling back to V1; `tao/src/platform_impl/windows/dpi.rs`). Every Win32
  window Annota creates on any thread is therefore PMv2 too. `MONITORINFO`,
  `SetWindowPos` and `GetCursorPos` all use physical pixels with no
  virtualization, including negative coordinates for the monitor above.
- A window's DPI is set by the monitor it is on. Windows sends `WM_DPICHANGED`
  only when a window *moves* to a monitor with another DPI, or when that
  monitor's DPI changes ([WM_DPICHANGED][wm-dpichanged]). A window **created
  directly at its monitor's rect and never moved never gets a DPI change.**
  That is the property that keeps the overlay simple.
- Tauri reports monitors in physical pixels: `Monitor::position()`, `size()`,
  `work_area()` and `scale_factor()`. Parla's flyout places itself with them
  (`tray.rs::show_flyout`).

### Why not a Tauri/WebView overlay

It can be done, but every step adds latency or a risk of blur:

1. **Placing it.** `WebviewWindowBuilder::position(x, y)` and `inner_size`
   take *logical* values. At creation, tao converts the logical position with
   each monitor's scale and takes the first monitor that contains the result
   (`tao/src/platform_impl/windows/window.rs`, the `init` function). On mixed
   DPI that test can be ambiguous. Downstream projects report windows placed on
   the wrong monitor (tauri#8981, tauri#6784). The safe sequence is Parla's:
   build hidden, `set_position(PhysicalPosition)`, *then*
   `set_size(PhysicalSize)`. Moving onto a monitor with another scale makes
   tao resize the window, so a size set before the move is lost (Parla comment
   in `show_flyout`).
2. **Getting the frame in.** A 2560×1440 BGRA frame is 14.7 MB, and 4K is
   33 MB. It has to cross into the renderer process, through Tauri IPC
   (`tauri::ipc::Response` raw bytes), a custom URI scheme
   (`register_asynchronous_uri_scheme_protocol`), or WebView2's
   `CreateSharedBuffer`/`PostSharedBufferToScript` (bound in
   `webview2-com-sys` 0.39). After that it needs a BGRA→RGBA swizzle,
   `putImageData`, and a GPU upload. A native window simply `BitBlt`s the DIB
   it already has.
3. **First paint.** A hidden WebView2 shows *whatever it last rendered* until
   the new frame lands. Hiding with `IsVisible=false` makes it "transparent and
   not rendered" ([ICoreWebView2Controller][controller]), so the window has to
   stay hidden until the page signals that it has painted, which costs another
   IPC round trip. Hidden transparent Tauri windows are also known to flash
   white when shown (tauri#14515, closed with no fix; the reporter's
   workaround was to park the window off-screen instead of hiding it).
4. **Pixel exactness.** The page has to size the image to physical ÷
   `devicePixelRatio` and trust that WebView2's rasterization scale already
   matches the monitor. It does once the window is on that monitor, because
   `ShouldDetectMonitorScaleChanges` defaults to true and wry never changes it
   ([ICoreWebView2Controller3][controller3]). A window that has just moved can
   still lag by a frame.
5. **Cost.** Each monitor needs its own window, and a WebView per window means
   another renderer, about 55–75 MB in Parla's measurements.

Microsoft's own WebView2 performance guide says to use "lightweight XAML or
Win32 screens" for simple surfaces, not WebView2 ([WebView2 performance][perf]).
The overlay is exactly that kind of surface: an image, a rectangle, a crosshair
and a size label.

### Recommended overlay build (Win32 inside the Tauri process)

- Create the windows **on Tauri's main thread** (`app.run_on_main_thread`).
  tao's `GetMessage`/`DispatchMessage` loop then pumps them like any other
  window. Register one `WNDCLASSEXW` with your own `wndproc`, using the
  `windows`/`windows-sys` crates that Annota already needs for capture.
- Styles: `WS_POPUP`, `WS_EX_TOPMOST | WS_EX_TOOLWINDOW` (no taskbar button,
  no Alt-Tab entry). Create each window directly at `rcMonitor` (physical,
  possibly negative), so it never gets `WM_DPICHANGED`. Set
  `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND` so Windows 11 doesn't
  round the corners, and `DWMWA_TRANSITIONS_FORCEDISABLED` so it doesn't fade
  in.
- Paint: back each window with the monitor's capture as a DIB section. Its
  `WM_PAINT` blits the frozen frame plus the selection overlay from a
  back-buffer, so the drag doesn't flicker. Call `UpdateWindow` before
  `ShowWindow(SW_SHOWNOACTIVATE)` on the others and `SW_SHOW` on the one
  under the cursor, so the first frame shown is already the frozen frame.
- Mixed DPI: the frame for monitor *M* is captured at *M*'s native physical
  pixels and drawn 1:1 into *M*'s window. Scale factors only matter for
  *UI chrome* (the size-readout font and line widths), taken from
  `GetDpiForWindow`. The drag is clamped to the monitor where it started
  (ANNOTA-2), so the crop never spans two DPIs.
- Lifetime: creating a native popup costs little, so the simplest correct
  choice is **create the windows for each Snap and destroy them after.** If
  you would rather pre-create them, key the cache by the monitor list
  (`EnumDisplayMonitors` rects + DPI) and rebuild it when the list changes.
  Don't move an existing overlay window to another monitor.
- Hand-off: when the mouse is released, Rust holds the crop's pixels, the
  source monitor and its scale. It hides the overlays and moves on to the
  Editor (§2).

### Focus and Esc

What the sources say:

- Windows lets a process call `SetForegroundWindow` only if some condition
  holds: among others, it is the foreground process, was *started by* the
  foreground process, *received the last input event*, or the foreground lock
  timeout has expired ([SetForegroundWindow][sfw]). A process that may set the
  foreground can pass that right on with `AllowSetForegroundWindow`.
- **Single-instance hand-off:** `tauri-plugin-single-instance` 2.5.4 already
  calls `AllowSetForegroundWindow(primary_pid)` in the second process before
  `WM_COPYDATA` (`platform_impl/windows.rs`; plugins-workspace#3548, fixed by
  PR #3592). That only helps **if the second process itself had the right.**
  Here it is started by AutoHotkey (Hyperkey `Run annota.exe --snap`), a
  background process, not by the foreground app. Whether AHK's hook-driven
  `Run` passes the right on is undocumented. **Check it by hand** (Caps+S over
  a focused Notepad, then press Esc without clicking).
- **tao's `set_focus` is a trap here.** When `SetForegroundWindow` fails, it
  calls `force_window_active`, which injects a synthetic **LAlt down/up** with
  `SendInput` and retries (`tao/src/platform_impl/windows/window.rs`). The Alt
  goes to the *user's* foreground app and can open its menu bar or KeyTips
  (plugins-workspace#3548). Here the user may also still be holding CapsLock,
  the Hyperkey prefix. Don't call tao's `set_focus` from the Snap path. Call
  `SetForegroundWindow` directly and accept that it may return FALSE.
- **Registered hotkeys grant foreground rights.** "Pressing a registered
  hotkey gives you the foreground activation love" ([Raymond Chen][chen]):
  `SetForegroundWindow` inside the `WM_HOTKEY` handler succeeds. That makes
  ANNOTA-2's F13 fallback (Hyperkey sends a hidden F13 combo that Annota
  registers) **the more reliable route for focus**, not just for latency.
  Chromium uses the same mechanism on purpose (`ui/base/win/foreground_helper.cc`
  registers a hidden F22 hotkey and injects it to win foreground).

Recommendation:

1. On Snap: show the overlays, then call `SetForegroundWindow(overlay under
   cursor)` once, with no Alt fallback.
2. For the whole selection phase, `RegisterHotKey(hwnd, id, MOD_NOREPEAT,
   VK_ESCAPE)` on an overlay window, and unregister it on finish or cancel.
   Esc then cancels whether or not focus was granted, and the press itself
   brings foreground rights with it. Right-click cancel and the drag need no
   focus: mouse input goes to the window under the cursor, and a click
   activates a normal popup.
3. When the mouse is released, Annota has just *received the last input
   event*, so the Editor's `SetForegroundWindow` is allowed.
4. Tray left-click Snap: Parla's flyout gets focus with `show()` +
   `set_focus()` straight after a tray click (`tray.rs`), because the click
   counts as input to the app.
5. If the overlay is ever a WebView after all: if Annota also installs a
   `WH_KEYBOARD_LL` hook, set `device_event_filter(DeviceEventFilter::Always)`.
   Without it, hooks are reported silent while a WebView2 window has focus
   (wry#1761, open, community workaround).

## 2. Editor

### Placing and sizing on the capture's monitor

The ANNOTA-2 decision is a pre-warmed hidden window. On each Snap (Rust, main
thread, window still hidden):

1. Find the target monitor (the one the crop came from) through Tauri's
   `available_monitors()` / `monitor_from_point` (physical).
2. Work out the inner size in **physical** pixels: the crop's width and height
   plus the hint line's height (logical × monitor scale), capped to about 90 %
   of `work_area()`. If capped, the page scales the image down, per ANNOTA-2.
3. `set_position(PhysicalPosition)` centred in the work area, **then**
   `set_size(PhysicalSize)`. The order matters. If the hidden window was last
   on a monitor with another DPI, the move sends `WM_DPICHANGED`. On Windows 11
   tao then applies the rect Windows suggests (`event_loop.rs`, the
   `WM_DPICHANGED` arm: "The suggested position is fine w/o adjustment on
   Windows 11"), which resizes the window. Parla's flyout found the same thing
   and moves first. For a decorated window, centre the *outer* rect: inner
   size plus the frame, `outer_size() - inner_size()` after sizing.
4. Send the crop to the page. It is small next to a full frame, but can reach
   2560×1440. Use `tauri::ipc::Response` raw bytes, or a custom-scheme URL the
   page loads into an `ImageBitmap`. Then wait for the page to report "painted"
   (after `requestAnimationFrame`), then `show()` + `SetForegroundWindow`.
   Give the window `background_color` matching the page, so a slow first frame
   never flashes white.
5. Restore the webview from hidden: Parla's `window.as_ref().show()` sets the
   controller's `IsVisible`, plus `MemoryUsageTargetLevel` Normal. Do the
   reverse on hide (see §3).

### One image pixel per device pixel

- `devicePixelRatio` = WebView2's rasterization scale = monitor DPI scale ×
  text scale ([ICoreWebView2Controller3][controller3]). Zoom is off by
  default: wry sets `IsZoomControlEnabled` and `IsPinchZoomEnabled` from
  `zoom_hotkeys_enabled`, which Tauri defaults to false (wry `webview2/mod.rs`).
  So a touchpad pinch can't zoom the canvas either.
- CSS size = pixels ÷ dpr is exact for whole-monitor sizes at 125 % or 150 %,
  but not for an arbitrary crop (1001 px ÷ 1.5 = 667.33 CSS px). Browsers snap
  fractional sizes to device pixels in different ways. Use `ResizeObserver`
  with `{ box: 'device-pixel-content-box' }` (Chromium 84+, so every WebView2)
  to read the canvas's true device-pixel size, set `canvas.width/height` to it,
  and draw the image with `drawImage` at 1:1 in device pixels
  ([web.dev][dpcb]). Keep the canvas at an integer device-pixel offset (centre
  it with whole-pixel margins computed in device pixels).
- Keep Boxes and Notes in **image-pixel** coordinates and export from the
  original crop at native size. The PNG then never depends on the window's DPI.

### Dragging the Editor to the other monitor

What happens, from the sources:

1. Windows sends `WM_DPICHANGED` when the window's DPI changes during the
   drag. tao (Windows 11 branch) applies Windows' suggested rect, which keeps
   the window's **logical** size, so the physical size changes by the scale
   ratio. tao emits `ScaleFactorChanged`, which Tauri forwards as
   `WindowEvent::ScaleFactorChanged`.
2. WebView2 notices the monitor change (`ShouldDetectMonitorScaleChanges`
   defaults to true), updates `RasterizationScale`, and the page gets a new
   `devicePixelRatio` and a `resize`.
3. If the page does nothing, the canvas keeps its old CSS size and is now shown
   at old-size × new/old scale device pixels. The image is resampled, blurry
   going up and lossy going down. The annotation data is unaffected.

What to do: watch `matchMedia('(resolution: <dpr>dppx)')` or rely on the
device-pixel-content-box observer, and redo the layout at 1:1 for the new dpr,
still capped at 90 % of the window. Don't resize the window from Rust during
the drag: it fights the user's drag and tao's own handling. A later fit (on
`ScaleFactorChanged`, debounced until moves stop) is optional polish.
Moving the Editor is rare in this workflow.

Known tao/Tauri bugs here, all fixed in the tao Annota will use:

- tao#1053 / tauri#12626: windows grew or shrank and jumped away from the
  cursor when dragged between 100/150/250 % monitors on Windows 11. Fixed by
  tao PR #1056 (Feb 2025), which is the Windows 11 branch above.
- tauri#8499: with "Show window contents while dragging" turned off, a second
  DPI event over-sized the window. tao now keeps the old physical size in that
  case (`is_show_window_contents_while_dragging_enabled` check).
- macOS-only position bugs (tauri#7890, #5229) don't apply.

## 3. Pre-warm cost, and WebView vs Win32

Parla's figures, measured once on this machine and quoted from Parla's README:

- Parla idle with four hidden WebView windows: about 610 MB working set
  (270 MB private) in total, of which `parla.exe` was about 40 MB. "Idle
  memory is dominated by the hidden WebView2 windows."
- One extra WebView window (the flyout): its renderer is about 55–75 MB
  private while open. About 6–12 MB stays in the shared GPU and browser
  processes after it closes.
- Cold flyout (create the window and webview, load the page, show): 122–162 ms
  after the click, median about 130 ms, at 150 % scaling.
- A hidden *window* alone doesn't stop WebView2: it "keeps compositing at
  60 fps (~15 % of a core) while idle in the tray" until the webview is hidden
  too (`overlay.rs::hide`). Parla also sets `MemoryUsageTargetLevel` to Low
  while hidden, as Microsoft recommends for inactive WebViews
  ([WebView2 performance][perf]). `TrySuspend` goes further: it suspends the
  renderer and pauses scripts, but events stop arriving until `Resume`.

What follows for Annota:

- All of Annota's webviews share one environment and user data folder, so one
  browser process and one GPU process ([process model][process]). The Editor
  needs these anyway. Pre-warming it adds about one renderer (tens of MB) and
  saves the ~130 ms cold create. Worth it, as ANNOTA-2 decided. Hide it with
  `window.hide()` **and** `window.as_ref().hide()` plus the Low memory target,
  as Parla does.
- **Warm show-to-first-paint has not been measured.** On paper a warm hidden
  webview whose page is already loaded needs one or two compositor frames
  after `IsVisible=true`, plus the image hand-off. Measure it in an assigned
  machine-lock turn before trusting it.
- **Overlay: Win32.** A WebView overlay adds a renderer for each monitor, a
  frame copy of 15–33 MB into the browser, and a "painted" round trip before it
  can show. A GDI/D2D popup shows the frozen frame from the DIB it already
  holds, with no scaling at all. That matches Annota's latency-first rule
  (ANNOTA-2: the freeze must not be late). It costs a few hundred lines of
  `windows`-crate Rust for the window class, painting and hit-testing, with no
  new dependencies.

## Bugs and pitfalls to avoid (checklist)

- Don't use `WebviewWindowBuilder::position/inner_size` (logical) to place
  windows on the non-primary monitor. Build hidden, then physical
  `set_position`, then `set_size`.
- Don't size before moving across a DPI boundary. The `WM_DPICHANGED` resize
  overwrites it.
- Don't call tao `set_focus` from the Snap path: its synthetic Alt reaches the
  user's app. Call `SetForegroundWindow` directly, and register Esc while
  selecting.
- Don't hide a WebView window without also hiding the webview (it composites at
  60 fps), and don't show it before the page has painted (stale or white frame,
  tauri#14515). Set `background_color`.
- Undecorated Tauri windows: keep `shadow(false)`. `true` adds a 1 px white
  border and, on Windows 11, rounded corners (Tauri `shadow` doc).
- Monitor layout changes during the day (4K-only today). Re-read monitors at
  every Snap.
- Re-check by hand after building the overlay: Esc before any click, Caps+S
  while a menu is open (`SetForegroundWindow` is refused while "menus are
  active"), and both monitors at different scales.

## Sources

- tao 0.37.1: `src/platform_impl/windows/dpi.rs` (PMv2), `window.rs`
  (`set_focus`, `force_window_active`, `set_outer_position`, creation-time
  monitor choice), `event_loop.rs` (`WM_DPICHANGED`).
- wry 0.57.0: `src/webview2/mod.rs` (zoom/pinch settings, `SetIsVisible`,
  `MoveFocus` on `WM_SETFOCUS`).
- tauri 2.12.4: `webview/webview_window.rs` (`position`, `inner_size`, `shadow`,
  `background_color`), `ipc/mod.rs` (`Response`), `app.rs`
  (`register_asynchronous_uri_scheme_protocol`).
- tauri-plugin-single-instance 2.5.4: `src/platform_impl/windows.rs`.
- Parla: `src-tauri/src/overlay.rs`, `src-tauri/src/tray.rs`, `README.md`
  ("Resource use").
- [SetForegroundWindow][sfw], [WM_DPICHANGED][wm-dpichanged],
  [ICoreWebView2Controller][controller], [ICoreWebView2Controller3][controller3],
  [WebView2 performance][perf], [WebView2 process model][process].
- [Raymond Chen, "Pressing a registered hotkey gives you the foreground
  activation love"][chen].
- [web.dev, pixel-perfect rendering with devicePixelContentBox][dpcb].
- Issues: [tao#1053](https://github.com/tauri-apps/tao/issues/1053),
  [tauri#12626](https://github.com/tauri-apps/tauri/issues/12626),
  [tauri#8499](https://github.com/tauri-apps/tauri/issues/8499),
  [tauri#8981](https://github.com/tauri-apps/tauri/issues/8981),
  [tauri#6784](https://github.com/tauri-apps/tauri/issues/6784),
  [tauri#14515](https://github.com/tauri-apps/tauri/issues/14515),
  [plugins-workspace#3548](https://github.com/tauri-apps/plugins-workspace/issues/3548),
  [wry#1761](https://github.com/tauri-apps/wry/issues/1761),
  [Chromium foreground_helper.cc](https://chromium.googlesource.com/chromium/src.git/+/6327123fee94de63bebc47bfbe4487f6ee88c44f/ui/base/win/foreground_helper.cc).

[sfw]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow
[wm-dpichanged]: https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged
[controller]: https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller
[controller3]: https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller3
[perf]: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance
[process]: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model
[chen]: https://devblogs.microsoft.com/oldnewthing/20090226-00/?p=19013
[dpcb]: https://web.dev/articles/device-pixel-content-box
