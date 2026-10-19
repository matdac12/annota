# Annota

Windows tray app: Caps+S → drag a Region → numbered Boxes with Notes → Ctrl+C copies one PNG with Callouts. Tauri v2 + vanilla TypeScript; Rust owns capture, the Overlay, clipboard, tray, single-instance and autostart; a canvas in the Editor's WebView draws and exports.

## Before you start

- **Spec:** `annota-v1-spec.md`, a document in Traccia project `ANNOTA` (`list_documents`, then `get_document`). Your issue names the sections it implements; the decision tickets it cites hold the reasoning.
- **Vocabulary:** `GLOSSARY.md`. Name things with its terms in code, issues and commits.
- **Issues:** Traccia, see `docs/agents/issue-tracker.md`. Labels: `docs/agents/triage-labels.md`. Domain docs: `docs/agents/domain.md`.

## Standing rules

- **Nothing reaches disk.** No Snap, crop or PNG is ever written to a file; the clipboard is the only output. Settings live in the registry (`HKCU\Software\Annota`), nowhere else.
- **Grab first, show second.** On `--snap`, every monitor is captured before any Annota window appears.
- **Physical pixels end to end.** The process is per-monitor-v2 aware; Overlay and Editor geometry are physical pixels; Box and Callout geometry is image pixels.
- **Timed measurements** only with Mattia, in the turn assigned on `%LOCALAPPDATA%\parla-machine.lock`. Estimates are fine anywhere; say they are estimates.
- **Copy Traccia desktop** (`C:\Users\MattiaDaCampo\Documents\TRACCIA\traccia\apps\desktop\src-tauri`) where the spec names it: `launch.rs`, `resident.rs`, `autostart.rs`, `tray.rs`, `nsis-hooks.nsh`.
- `tauri-plugin-single-instance` stays pinned at `=2.5.4`: the `--snap` fast path depends on its private window names.
