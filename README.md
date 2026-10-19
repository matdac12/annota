# Annota

Annotated screenshots to paste into agent chats. A small Windows tray app: press
Caps+S (via Hyperkey), drag a region, draw numbered boxes with a note each, and
Ctrl+C copies one PNG with the notes as callouts beside their boxes. Nothing is
saved to disk; the clipboard is the only output.

Separate from Parla: you dictate into Annota's note field with Parla like into
any text box.

Paste with Ctrl+V in T3 Code, the Claude desktop app, browser chats and Word;
with Alt+V in Claude Code and Codex inside Windows Terminal.

## Status

v1 is specified and sliced. The scaffold is in: a tray icon (Snap, Quit) and a hidden
Editor. Capture, the Overlay and the Editor itself land in the build issues. Planning
lives in Traccia, project ANNOTA: the map "Map: Annota v1", the spec document
`annota-v1-spec.md`, and the issues in milestone v1.

## Build

Windows only. You need Node 22 with pnpm 10 (`packageManager` in `package.json`), a
stable Rust toolchain, and the WebView2 runtime (already on Windows 11).

```
pnpm install
pnpm tauri dev      # starts in the tray, no window; left-click / Snap logs start_snap
pnpm build:exe      # vite build + tauri build --no-bundle -> src-tauri/target/release/annota.exe
pnpm build:installer # vite build + tauri build --bundles nsis -> the per-user setup (see Install)
pnpm check          # typecheck, front-end tests, vite build, cargo test, cargo clippy -D warnings
```

`pnpm tauri dev` runs the debug build, which keeps a console so `start_snap` shows
there; the release exe has no console. Pull requests run the same checks in
GitHub Actions (`.github/workflows/ci.yml`). There is no release pipeline.

## Install

```
pnpm build:installer
```

writes `src-tauri/target/release/bundle/nsis/Annota_<version>_x64-setup.exe`: a per-user NSIS
installer, unsigned (SmartScreen may warn on the first run), no administrator rights. The first
build downloads NSIS into Tauri's cache.

- **Install:** run the setup (`/S` installs silently). It installs
  `%LOCALAPPDATA%\Annota\annota.exe` with a Start menu entry; the finish page starts Annota.
  Hyperkey's Caps+S runs `%LOCALAPPDATA%\Annota\annota.exe --snap`.
- **Start with Windows:** the first time the installed exe runs, it writes
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `Annota` = `"<exe>"` and records that in
  `HKCU\Software\Annota` (`AutostartInitialized`). After that the tray's **Start with Windows**
  checkbox is the switch: ticked when the Run value exists and Task Manager's Startup apps hasn't
  disabled it. Unticking deletes the Run value and sticks across upgrades and reinstalls. A debug
  build (`pnpm tauri dev`) never registers itself and shows the checkbox greyed out.
- **Upgrade:** there is no updater. Rebuild, then run the newer setup. It closes the running Annota,
  replaces the files, and its finish page starts Annota again. Your Start with Windows choice is kept.
- **Uninstall:** Settings → Apps → Annota (or `%LOCALAPPDATA%\Annota\uninstall.exe`). It removes the
  Run value, the Task Manager entry (`…\Explorer\StartupApproved\Run`, value `Annota`) and
  `HKCU\Software\Annota`. Annota keeps nothing else on disk.

## For agents

Start at `AGENTS.md`. Vocabulary is in `GLOSSARY.md`.
