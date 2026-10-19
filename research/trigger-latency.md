# How fast does Caps+S reach a running Annota?

Research for Traccia ANNOTA-6 (map ANNOTA-1). Throwaway branch, never merged.
No timed measurements were taken (shared-machine rule). Every number below is an
estimate from source and docs; the last section lists what to measure in the
implementation turn.

## Answer

**Use `Run annota.exe --snap`**, as ANNOTA-2 decided, but give `main()` a
pre-Tauri fast path. Before `tauri::Builder`, the second process looks up the
resident instance's window, sends it `--snap` with `WM_COPYDATA`, and exits.
That way a second launch never builds the tao event loop or queries WebView2.

| Path | Keypress to callback in the resident Annota (estimate) | Costs |
|---|---|---|
| A. `Run`, plugin only (stock `tauri-plugin-single-instance`) | **~25-60 ms** warm | process start plus part of `tauri::Builder::build` |
| A'. `Run`, with the pre-Tauri fast path | **~15-35 ms** warm | process creation and DLL loading only |
| B1. `Send` F13, written per `hyper.ahk` rules (`KeyWait` first) | **~50-200+ ms**, set by how fast the user lets go of Caps | needs a global hotkey; waits on the human |
| B2. `SendEvent "{F13}"` with no `KeyWait` first | **~1-3 ms** | needs a global hotkey and foreground keystroke injection |
| C. AHK `SendMessage`/`PostMessage` straight to Annota's hidden window, `Run` if not found | **<1 ms** | ties `hyper.ahk` to an Annota window class (new open question) |

Path A' is tens of milliseconds. The frozen frame is unaffected in that window:
the user is holding still on Caps+S, and Hyperkey swallows the keys, so menus,
tooltips and hover states are still on screen (see "Why tens of ms don't
matter"). Switch to Send only if the measurement shows p95 above ~100 ms.

## 1. How the single-instance plugin works on Windows

Source: `tauri-plugin-single-instance` 2.5.4,
`src/platform_impl/windows.rs`, the version in Traccia desktop's `Cargo.lock`.

The plugin uses a named mutex plus a hidden window and `WM_COPYDATA`. There is no
named pipe.

- **Names** come from the bundle identifier: mutex `{id}-sim`, window class
  `{id}-sic`, window title `{id}-siw` (lines 73-75).
- **First instance:** `CreateMutexW(.., bInitialOwner = true, {id}-sim)`
  succeeds, so it creates a hidden window
  (`WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW`,
  then restyled `WS_VISIBLE | WS_POPUP`; it is a top-level window, not a
  message-only one) on the thread running plugin setup, which is the main
  thread (lines 133-143, 221-277).
- **Second instance:** `CreateMutexW` returns `ERROR_ALREADY_EXISTS`, so it calls
  `FindWindowW(class, title)`, then `AllowSetForegroundWindow(pid of that
  window)`. It builds `cwd \0 arg0 \0 arg1 ...` (UTF-8, `dwData = 1543`) and
  sends it with `SendMessageTimeoutW(WM_COPYDATA, SMTO_NORMAL, 10_000 ms)`, then
  `app.cleanup_before_exit()` and `std::process::exit(0)` (lines 80-131).
- **Delivery:** the first instance's window proc runs the user callback
  **synchronously, on its main thread, inside `WM_COPYDATA`**
  (lines 181-209). The second process stays blocked until the callback
  returns. The callback should therefore start the capture and return quickly:
  it must not wait on the WebView.
- **Gap:** if the mutex exists but `FindWindowW` finds nothing (the resident is
  still starting), the plugin falls through and the second launch starts a full
  app. Traccia handles this with its own mutex taken in `main()`, then
  `exit(0)` in `setup` when it isn't first
  (`TRACCIA/traccia/apps/desktop/src-tauri/src/main.rs`, `resident.rs`).

## 2. What a second launch costs before the handoff (Tauri v2 startup order)

Source: `tauri` 2.12.4 `src/app.rs`, `tauri-runtime-wry` 2.12.2 `src/lib.rs`,
`tao` 0.37.1 `src/platform_impl/windows/event_loop.rs`.

The plugin's check runs inside **plugin setup**, which
`Builder::build` calls as its last step (`app.manager.initialize_plugins`,
app.rs ~L2607). Before that, `build` has already done the following:

1. `AppManager::with_handlers(context, ...)`: the embedded assets are static,
   so this is cheap.
2. `R::new(...)` (app.rs ~L2504-2508) leads to `Wry::init_with_builder`:
   - **tao `EventLoop::new`**: `become_dpi_aware()`, creates a hidden
     thread-message window, `allow_dark_mode_for_app`, **spawns a wait
     thread**, and registers raw input for all mice and keyboards
     (event_loop.rs L171-200).
   - **`wry::webview_version()`**, which calls
     `GetAvailableCoreWebView2BrowserVersionString` to fill
     `webview_runtime_installed` (runtime-wry lib.rs ~L2783). This reads
     the registry and file versions. It does **not** create a WebView2
     environment.
3. Event handlers for the menu and tray, `register_core_plugins()`, the
   channel plugin.
4. **The tray icon from `tauri.conf.json` (`app.trayIcon`), if one is declared,
   is built here, before plugin setup** (app.rs ~L2585-2605). A config-declared
   tray would make every Caps+S briefly add a second tray icon, which
   `cleanup_before_exit` then drops.
5. `initialize_plugins`, which runs the single-instance setup, then hand-off and
   exit.

Windows from config (`create: true`) and `Builder::setup` run only on
`RuntimeRunEvent::Ready` inside `App::run` (app.rs L1442-1447, `fn setup`
L2688-2699). The second instance exits before `run`, so **it never initialises
WebView2**. The pre-warmed Editor window costs a second launch nothing.

So the stock plugin already short-circuits before any WebView work. The
remaining overhead (step 2: the event loop, a wait thread, raw-input
registration, the WebView2 version probe) is modest, estimated at 5-20 ms, but
it is pure waste for a process whose only job is to send one message.

## 3. The cheap second process (the fast path)

In `main()`, before `tauri::Builder::default()`:

1. Parse args (Traccia's `launch.rs` pattern).
2. `FindWindowW("{id}-sic", "{id}-siw")`. If found:
   `GetWindowThreadProcessId`, then `AllowSetForegroundWindow(pid)`, then
   `SendMessageTimeoutW(WM_COPYDATA)` with the plugin's own payload
   (`dwData = 1543`, `cwd\0args...`), then `std::process::exit(0)`. This repeats
   what the plugin's second-instance branch does (windows.rs L82-130) while
   skipping `Builder::build`. The plugin's window proc stays the single
   receiver, so the callback behaves the same whichever path delivered the
   message.
3. Otherwise fall through to Traccia's pattern: own `Local\` mutex claim, then
   the elevation check, then `tauri::Builder` with the single-instance plugin
   registered **first**.

Keep the second process cheap in other ways too:

- `#![windows_subsystem = "windows"]`: no console allocation.
- **Build the tray in `setup`, not in `tauri.conf.json`** (see §2, step 4).
  Traccia already does this (`tray::build` in `setup`).
- No heavy statics, logging or file I/O before the fast path.
- `[profile.release]` as in Traccia (`lto`, `codegen-units = 1`,
  `panic = "abort"`, `strip`). A smaller image loads faster. Size still matters
  less than the import table: user32, ole32 and the rest are KnownDLLs, already
  mapped.
- The coupling cost: the fast path depends on the plugin's private names and
  payload format. Pin the plugin to an exact version (`=2.5.4`) and keep the
  names in one constant module next to a unit test, so an upgrade has to be a
  deliberate change. The alternative is a tiny window of Annota's own with a
  registered message (no plugin coupling), but then the plugin has to stay
  anyway for the start-up race.

What is left is OS process creation plus the loader: an estimated ~10-30 ms warm
on an i7-1370P. **Defender rescans a new image** after every build, so the first
Caps+S after an install or rebuild can take 100 ms or more. Measure warm and
first-run separately.

## 4. The Hyperkey side

Local sources: `%USERPROFILE%\Scripts\hyper.ahk` and
`Documents\HYPERKEY\CLAUDE.md` ("AutoHotkey traps").

### Run

- AHK v2 docs (`lib/Run.htm`): "If no verb is specified, Target is passed as-is
  to the lpCommandLine parameter of CreateProcess", with ShellExecuteEx only as
  a fallback. "Performance may be slightly improved if Target is an exact
  path." `Run` doesn't wait (that's `RunWait`).
- `Run` sends no keystrokes, so **the prefix wedge cannot happen**. `CapsLock & r`
  already uses this order: `Run` first, then `KeyWait "CapsLock", "T0.4"`, so
  the wait adds nothing to latency. Mirror it exactly:
  ```ahk
  CapsLock & s:: {
      Run '"' EnvGet("LOCALAPPDATA") '\Annota\annota.exe" --snap'
      KeyWait "CapsLock", "T0.4"
  }
  ```
  (The install path is the packaging ticket's call; quote it and keep it
  exact.)
- **Elevation:** the `hyper.ahk` comment on Caps+R says "This script runs
  elevated, so the exe it starts would be elevated too". With Annota running
  (the normal case, since Start with Windows is on), this is harmless: the
  elevated second process only sends to the medium-integrity resident, and UIPI
  only blocks messages "from a lower-integrity-level sender"
  (ChangeWindowMessageFilterEx remarks). With **no** Annota running, an elevated
  `--snap` must not become the resident instance. Copy Traccia's
  `resident.rs`: relaunch through `explorer.exe` unelevated and exit.

### Send

- AHK v2 docs (`lib/Send.htm`, `lib/SendMode.htm`): `Send` defaults to
  SendInput, which "buffers any physical keyboard or mouse activity during the
  send" ("Anything the user types during a SendInput is postponed until
  afterward"). This is the cause of the prefix wedge recorded in `hyper.ahk` and
  HYPERKEY `CLAUDE.md`: if Caps is released mid-send, its key-up never reaches
  the hook and Caps stays "held".
- The Hyperkey rule for any `CapsLock & key` that sends keys is to wait for Caps
  to come up first, or to use SendEvent. Both have a cost:
  - **B1, `KeyWait` then `Send`:** the hotkey fires only once the user lets go
    of Caps. That is roughly the human key-release time, estimated at
    ~50-200 ms after S goes down and often more. **This is slower than Run.**
  - **B2, `SendEvent "{F13}"` with no `KeyWait` first:** SendEvent doesn't buffer
    physical input. Its default is "a short delay (sleep) of 10 milliseconds
    ... after every keystroke" (`lib/SetKeyDelay.htm`), but `WM_HOTKEY` is
    posted on F13 key-down (RegisterHotKey remarks: "the system posts the
    WM_HOTKEY message"), so delivery takes ~1-3 ms.
- Send's other costs: Annota must call `RegisterHotKey`, which ANNOTA-2 wanted
  to avoid. It "fails if the keystrokes ... have already been registered for
  another hot key". A combo must avoid **Ctrl+Shift**, which is Parla's
  `LCtrl+Shift` command chord, and **Alt**, which can trigger menu-bar or
  AltGr side effects. Bare `F13` or `Shift+F13` is the safe choice.

### Option C: a window message straight from AHK (not recommended for v1)

AHK v2 has `SendMessage`/`PostMessage` (`lib/PostMessage.htm`). The script could
find Annota's hidden window (`WinExist("ahk_class {id}-sic")`) and message it
directly, using `Run` only when no window is found. That removes the process
spawn: under 1 ms, with no keystrokes and no global hotkey. UIPI allows it
because the elevated script sends down to a medium-integrity window. On a
failure the docs mention, a "target window ... running at a higher integrity
level than the script", AHK throws. The cost: `hyper.ahk` would have to know an
Annota window class and message format (`WM_COPYDATA` with a `Buffer`-built
`COPYDATASTRUCT`, or a custom registered message), which breaks the "same
pattern as Caps+R" simplicity. Keep it in reserve in case A' measures badly.
Mattia decides.

### Focus (a hand check, either path)

The overlay needs keyboard focus for Esc. SetForegroundWindow is allowed only if
the caller "was started by the foreground process" or "received the last input
event" (among other conditions); "It is possible for a process to be denied the
right ... even if it meets these conditions". On the Run path, focus comes from
the plugin's or the fast path's `AllowSetForegroundWindow(pid)` from a process
that AHK, not the foreground app, started. Traccia's Caps+R relies on the same
chain (`let_resident_take_focus`, `AllowSetForegroundWindow(ASFW_ANY)`), so it is
known to work in practice, but no doc guarantees it. Mattia should confirm by
hand that the region overlay takes focus and Esc cancels on the first try.

## Why tens of ms don't matter here

The goal is "grab every monitor before any UI appears, so menus, tooltips and
hover states are kept" (ANNOTA-2). AHK's hook suppresses both Caps and S, so the
foreground app never sees a keystroke that could dismiss a menu or tooltip. The
mouse is typically still while the user presses the chord. Between keypress and
callback, nothing on screen changes because of the trigger itself. What matters
is that **no Annota UI appears before the grab**. That is true on every path if
the callback grabs first and shows the overlay second. A 20-40 ms trigger is
also below the latency humans perceive as a delay for a "snap" action.

## What to measure in the implementation turn (under the machine lock)

1. **Keypress to callback, per path.** In `hyper.ahk`, log
   `QueryPerformanceCounter` (via `DllCall`) at hotkey entry. In Annota, log QPC
   at the top of the single-instance callback, or at `WM_HOTKEY` for the Send
   path. QPC is system-wide, so the two can be subtracted. 30 presses each,
   p50/p95, for: A (plugin only), A' (fast path), and B2 only if A' fails.
2. **Second-process lifetime** (`CreateProcess` to exit), warm, and on the
   **first launch after a rebuild** (the Defender scan).
3. **Callback to frozen frame**: the capture cost, which belongs to the capture
   ticket but finishes the same end-to-end number.
4. **By hand (Mattia):** the overlay takes focus and Esc cancels; Caps never
   wedges after a burst of Caps+S; Caps+S with Annota not running starts it
   unelevated.

## Sources

- `tauri-plugin-single-instance` 2.5.4: `src/lib.rs`,
  `src/platform_impl/windows.rs` (cargo registry; github.com/tauri-apps/plugins-workspace,
  `plugins/single-instance`).
- `tauri` 2.12.4 `src/app.rs` (`Builder::build`, `fn setup`, `App::run`);
  `tauri-runtime-wry` 2.12.2 `src/lib.rs` (`init_with_builder`, `init`);
  `tao` 0.37.1 `src/platform_impl/windows/event_loop.rs` (`EventLoop::new`).
- AutoHotkey v2 docs: `lib/Run.htm`, `lib/Send.htm`, `lib/SendMode.htm`,
  `lib/SetKeyDelay.htm`, `lib/KeyWait.htm`, `lib/PostMessage.htm`
  (autohotkey.com/docs/v2).
- Microsoft Learn: RegisterHotKey, SetForegroundWindow, WM_COPYDATA,
  ChangeWindowMessageFilterEx (UIPI).
- Local: `%USERPROFILE%\Scripts\hyper.ahk` (`CapsLock & r`, the `Send`/`KeyWait`
  comments), `Documents\HYPERKEY\CLAUDE.md` ("The prefix wedge"),
  `Documents\TRACCIA\traccia\apps\desktop\src-tauri\src\{main,resident,launch}.rs`,
  `docs/desktop-app.md` §8.
