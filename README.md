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
pnpm check          # typecheck, front-end tests, vite build, cargo test, cargo clippy -D warnings
```

`pnpm tauri dev` runs the debug build, which keeps a console so `start_snap` shows
there; the release exe has no console. Pull requests run the same checks in
GitHub Actions (`.github/workflows/ci.yml`). There is no release pipeline.

## For agents

Start at `AGENTS.md`. Vocabulary is in `GLOSSARY.md`.
