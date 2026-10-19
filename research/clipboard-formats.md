# Which clipboard formats make Annota's PNG paste into every agent chat?

Research for Traccia ANNOTA-3 (parent map ANNOTA-1). Date: 2026-10-19.
Machine checked: Windows 11 Pro 26200, Claude Code 2.1.302, Codex CLI 0.163.0,
Claude desktop 2.19675.1.0 (MSIX), Windows Terminal with Mattia's `settings.json`.
No timings were taken.

## Answer

Put **exactly two formats** on the clipboard, in this order, in one
`OpenClipboard` / `EmptyClipboard` session:

1. the registered format **`"PNG"`**: the PNG file bytes
2. **`CF_DIBV5`**: a 32 bpp `BI_BITFIELDS` `BITMAPV5HEADER` with an alpha mask and
   `LCS_sRGB`, bottom-up rows

Windows synthesises `CF_DIB` and `CF_BITMAP` from `CF_DIBV5` on request, so don't
write them yourself. Don't add `CF_HDROP` (file drop), text or HTML.

**Crate: `arboard` 3.6** (`Clipboard::new()?.set_image(ImageData { width, height, bytes: rgba })`).
On Windows it writes exactly this pair, in this order. It's the same pair, in the same
order, that Chromium writes when it copies an image itself. Call it from an
`async` Tauri command (or `spawn_blocking`) so it stays off the main thread.

**One caveat about the "one Ctrl+V" goal:** in the browsers and the Claude desktop app,
Ctrl+V works. In **Claude Code and Codex inside Windows Terminal, Ctrl+V doesn't paste
an image-only clipboard. Alt+V does.** That's a terminal and CLI behaviour, and no
clipboard format fixes it without writing a temp file (see "Why not a file drop").

## Per-target table

| Target | How it reads a clipboard image | Format it actually uses from Annota's pair | Key | Works with PNG + DIBV5? |
|---|---|---|---|---|
| Claude Code CLI (native Windows, in WT) | Starts `powershell -Sta` twice: `[System.Windows.Forms.Clipboard]::ContainsImage()`, then `GetImage()` and `.Save(tmp, Png)`. Reads the temp file, then deletes it | `CF_BITMAP` (synthesised from `CF_DIBV5`). Alpha is dropped | **Alt+V** (`chat:imagePaste`; Ctrl+V is bound only on WSL) | Yes, with Alt+V |
| Codex CLI (in WT) | `arboard`: `file_list()` first, then `get_image()`, which reads `"PNG"` if present, else `CF_DIBV5`. Re-encodes to PNG in a temp file | `"PNG"` | **Alt+V** (Ctrl+V and Shift+Insert also bound, but WT eats Ctrl+V) | Yes, with Alt+V |
| Claude desktop app (Electron) | Chromium `ui/base/clipboard` gives the DOM paste event a `File` named `image.png` | `"PNG"` bytes passed through as-is | Ctrl+V | Yes |
| Claude.ai / ChatGPT in Brave or Edge | Same Chromium path | `"PNG"` bytes as-is | Ctrl+V | Yes |
| Traccia web dashboard | **No paste handler.** Attachments come only from drag-and-drop or the file picker (`apps/web/components/issue-detail/attachments.tsx`) | none | n/a | **No.** Traccia needs an `onPaste` handler (out of Annota's scope) |

## Evidence

### Windows: synthesised formats

Microsoft's "Clipboard Formats" page lists `CF_DIBV5 → CF_BITMAP`,
`CF_DIBV5 → CF_DIB` and `CF_DIBV5 → CF_PALETTE` as system conversions. It says "there is no
advantage to placing the conversion format(s) on the clipboard", and "it is best to
place the CF_DIB or CF_DIBV5 format on the clipboard". When `CF_DIBV5` carries colour
space information, the system converts to sRGB when `CF_DIB` or `CF_DIBV5` is requested.
It also says formats are enumerated in the order they were placed, and readers usually
take the first one they recognise, so put the richest format first.
Source: https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats

`"PNG"` is a *registered* format (`RegisterClipboardFormat(L"PNG")`). It isn't a standard
format, so Windows never synthesises it or converts it to or from DIB. Same page,
"Registered Clipboard Formats".

### Chromium (Brave, Edge, Claude desktop / Electron)

From `ui/base/clipboard/clipboard_win.cc` and `clipboard_format_type_win.cc` (main, fetched 2026-10-19):

- `ClipboardFormatType::PngType()` = `RegisterClipboardFormatChecked(L"PNG")` and
  `BitmapType()` = `CF_DIBV5`.
- **Detection:** `GetStandardFormatsInternal` advertises `image/png` to web pages only
  `if (::IsClipboardFormatAvailable(CF_DIB))`. **A clipboard with only `"PNG"` shows web
  pages no image.** `CF_DIB` must be available, and it is when `CF_DIBV5` is
  present, through synthesis.
- **Reading:** `ReadPngInternal` reads `"PNG"` first ("On Windows, PNG and bitmap are separate
  formats. Read PNG if possible, otherwise fall back to reading as a bitmap"). The fallback
  reads `CF_DIB` and re-encodes it to PNG. With Annota's pair, pages get Annota's PNG bytes unchanged.
- **Writing:** `ClipboardWin::WriteBitmap` writes `"PNG"` first, then `CF_DIBV5`. The comment
  explains that some programs only support one or the other, Word's DIBV5 support is
  buggy, and order matters because some programs take the first compatible format.
- **Limit:** `kMaxClipboardSize = base::MiB(256)` per format.

From Blink `third_party/blink/renderer/core/clipboard/data_object_item.cc`: for type
`image/png`, `GetAsFile()` calls `ReadPng` and returns
`File("image.png", ...)` with content type `image/png`. That's what
`event.clipboardData.files` / `items` give a chat page.

From Blink `data_object.cc` `CreateFromClipboard`: when `text/uri-list` is available
(a `CF_HDROP` is present), the dropped files are added as file items **as well as** the
`image/png` item. Adding a file drop next to the image could hand a page two files.

### Claude Code CLI 2.1.302

Strings in the installed `~/.local/bin/claude.exe`, Windows branch:

```
checkImage: powershell -NoProfile -NonInteractive -Sta -Command
  "Add-Type -AssemblyName System.Windows.Forms; if (-not [System.Windows.Forms.Clipboard]::ContainsImage()) { exit 1 }"
saveImage:  ... "$img = [System.Windows.Forms.Clipboard]::GetImage(); if ($null -eq $img) { exit 1 };
  $img.Save(<tmp>, [System.Drawing.Imaging.ImageFormat]::Png)"
```

After that it reads the file (and converts a BMP signature to PNG if it sees one), then
resizes or compresses it before sending.

- WinForms: `ContainsImage() => ContainsData(DataFormats.Bitmap, autoConvert: true)` and
  `GetImage() => GetTypedDataIfAvailable<Image>(DataFormats.Bitmap)`
  (dotnet/winforms `Clipboard.cs`). The OLE helper asks for `CF_BITMAP` with `TYMED_GDI` and
  builds the image with `Image.FromHbitmap` (`WinFormsOleServices.cs`). `Image.FromHbitmap`
  ignores alpha. Windows PowerShell 5.1 runs on .NET Framework, which uses the same
  `DataFormats.Bitmap` / `CF_BITMAP` contract. **Claude Code never reads `"PNG"`.** It
  depends on a DIB-family format being present, and it gets one through synthesis
  from `CF_DIBV5`.
- Keys: `Le = (windows || wsl) ? "alt+v" : "ctrl+v"` is bound to `chat:imagePaste`, plus
  `ctrl+v` on WSL only. The official docs agree: "`Alt+V` (Windows and WSL) | Paste image from clipboard"
  (https://code.claude.com/docs/en/interactive-mode).
- The bracketed-paste handler treats an *empty* paste as "read the clipboard image" only
  when `platform === macos || platform === wsl`. On native Windows an empty paste does nothing.
- When `chat:imagePaste` finds no image, it falls back to inserting clipboard text
  (`input_image_paste` / `text_fallback`).

### Codex CLI

Source: `openai/codex` `codex-rs/tui/src/clipboard_paste.rs` and
`chatwidget/interaction.rs` (main):

- `paste_image_as_png()` opens `arboard::Clipboard`, tries `get().file_list()` first ("prefer
  files if both are present"), then `get_image()`, then re-encodes to PNG and writes a
  `codex-clipboard-*.png` temp file.
- arboard 3.6.1 `get_image` on Windows: if `"PNG"` is available it decodes that.
  Otherwise it reads `CF_DIBV5` (`src/platform/windows.rs`).
- Trigger: a key press of `v` with `CONTROL | ALT`, or on Windows `Shift+Insert`. On
  Windows, Ctrl+V and Shift+Insert carry a text fallback. An empty bracketed paste doesn't
  trigger an image read (`App` passes `TuiEvent::Paste` to `handle_paste` →
  `apply_paste`, which does nothing with an empty string).
- `apply_paste` attaches an image when the pasted *text* is an image file path
  (`handle_paste_image_path`).

### Windows Terminal

Source: `microsoft/terminal` `src/cascadia/TerminalApp/TerminalPage.cpp`:

- `clipboard::read()` reads `CF_UNICODETEXT`, else the first path in `CF_HDROP`,
  else returns an empty string. **It never reads image formats.**
- `_PasteFromClipboardHandler`: "LOAD BEARING: Send an empty bracketed paste even if the
  clipboard was empty." With an image-only clipboard the CLI gets an empty bracketed paste.
- Mattia's `settings.json` binds `ctrl+v` → `User.paste` (paste), so WT handles Ctrl+V
  and the CLI never sees the key. WT's `userDefaults.json` ships the same binding.

So with Annota's PNG on the clipboard, Ctrl+V in WT gives Claude Code (native Windows)
and Codex an empty paste, and both ignore it. **Alt+V** isn't bound in WT, reaches the CLI,
and both CLIs paste the image.

### arboard 3.6.1 (what `set_image` writes on Windows)

From `src/platform/windows.rs`:

- `Set::image`: `EmptyClipboard`, then `add_png_file` (encodes RGBA → PNG with the `image`
  crate's `PngEncoder` and sets it as registered `"PNG"`), then `add_cf_dibv5`. The comment says "The ordering
  ... is important, as some programs will grab the first format available. PNGs tend to
  have better compatibility on Windows, so it is set first."
- The DIBV5 header has `bV5BitCount 32`, `BI_BITFIELDS`, masks R `0x00ff0000`, G `0x0000ff00`, B `0x000000ff`,
  A `0xff000000`, and `bV5CSType = LCS_sRGB`. Rows are flipped bottom-up because "MS Word (and WordPad)
  cannot paste an image that has a negative height".
- Opening retries 5 × 5 ms (`DEFAULT_OPEN_ATTEMPTS`, the same as Chromium) and returns
  `Error::ClipboardOccupied` on failure. An open clipboard can't cross threads
  (`PhantomData<*const ()>`).
- `set().exclude_from_history()` / `exclude_from_cloud()` exist. Leave them off so Win+V history keeps the Snap.
- Default feature `image-data` pulls in `image` (PNG codec) and `windows-sys`.

`tauri-plugin-clipboard-manager` (v2) wraps the same `arboard` 3.6 `set_image`, so its
`writeImage` gives the same formats. Calling arboard directly from Annota's own Rust
command keeps control of threading and avoids a JSON round trip.

Tauri v2 docs (`develop/calling-rust`): "Commands without the async keyword are executed
on the main thread unless defined with `#[tauri::command(async)]`". Large binary
arguments can arrive raw through `tauri::ipc::Request` / `InvokeBody::Raw` instead of
JSON.

## Recommended implementation shape (for the spec, not code)

- The WebView canvas has the final pixels. Send them to Rust as raw RGBA
  (`getImageData`) in an `InvokeBody::Raw` request, and call
  `arboard::Clipboard::new()?.set_image(...)` inside an `async` command. arboard does the
  single PNG encode and the DIBV5. (Alternative: send `canvas.toBlob('image/png')` bytes and use
  `clipboard-win` to set `"PNG"` raw plus a hand-built DIBV5. That still needs RGBA for the
  DIB, so it saves nothing and adds code.)
- Keep the output **opaque** (alpha 255 everywhere). The Callouts are clamped inside the image
  (ANNOTA-2), so this comes free. Claude Code's `CF_BITMAP` path drops alpha, and any
  transparent pixel would come out black there.
- Keep the PNG **plain sRGB**: no ICC, gAMA or cHRM chunks (arboard's encoder adds none).
  `LCS_sRGB` in the DIBV5 means Windows does no colour conversion.
- The pixel data is placed immediately (no delayed rendering), so the clipboard keeps the
  image after the Editor hides, or even after Annota exits.

## Size limits

- Claude API / claude.ai: 8000×8000 px max and 10 MB per image. Claude 4.7+ models downscale to a
  **2576 px** long edge (older models: 1568 px)
  (https://platform.claude.com/docs/en/build-with-claude/vision). ANNOTA-2's Callout text
  sizing assumes roughly 1568. That still holds as a lower bound.
- ChatGPT: 20 MB per image (OpenAI File Uploads FAQ,
  https://help-lb.openai.com/en/articles/8555545-file-uploads-faq).
- Chromium: 256 MiB per clipboard format.
- A full 4K capture is about 33 MB of DIBV5 plus a few MB of PNG. All limits hold. The PNG is
  well under 10 MB for screenshots.

## Why not a file drop (`CF_HDROP`)

A temp PNG plus `CF_HDROP` would make **Ctrl+V** work in WT. WT pastes the path as text,
and both CLIs turn an image path into an attachment. But:

- ANNOTA-2 says "Nothing is written to disk".
- Per Blink `CreateFromClipboard`, browsers would see both the file and `image.png`, so a chat
  page could attach two copies.
- Codex prefers `file_list()` over image data. Fine, but it's one more path.
- Someone has to clean up the temp files.

The alternative is a terminal setting, not an Annota feature: unbind `ctrl+v` in WT and bind
`ctrl+v` → `chat:imagePaste` in `~/.claude/keybindings.json`. Codex already handles a raw
Ctrl+V with a text fallback on Windows, and Claude Code's image paste falls back to text.
That's Mattia's call (open question below).

## Open questions surfaced

1. **Ctrl+V vs Alt+V in the CLIs.** Accept Alt+V in Claude Code and Codex, or change WT
   (unbind `ctrl+v`) and Claude Code keybindings so Ctrl+V does image-or-text? This isn't Annota code.
2. **Traccia paste-to-attach.** Traccia's web dashboard can't take a pasted image today. It needs an
   `onPaste` handler that reads `clipboardData.files` (a Traccia issue, not Annota).
3. Not verified by hand: ChatGPT's and Claude.ai's own paste handlers (closed source). They
   receive the standard Chromium `image.png` File, the same one a Chrome "Copy image" produces.
   A one-off manual paste check during v1 acceptance would close this.

## Sources

- Microsoft Learn, Clipboard Formats: https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats
- Chromium `ui/base/clipboard/clipboard_win.cc`, `clipboard_format_type_win.cc`: https://chromium.googlesource.com/chromium/src/+/refs/heads/main/ui/base/clipboard/
- Blink `core/clipboard/data_object.cc`, `data_object_item.cc`: https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/clipboard/
- Windows Terminal `TerminalPage.cpp`, `userDefaults.json`, `defaults.json`: https://github.com/microsoft/terminal
- Codex `codex-rs/tui/src/clipboard_paste.rs`, `chatwidget/interaction.rs`, `app.rs`, `bottom_pane/chat_composer/paste_input.rs`: https://github.com/openai/codex
- arboard 3.6.1 `src/platform/windows.rs` (local cargo registry); https://docs.rs/arboard/3.6.1
- tauri-plugin-clipboard-manager `src/desktop.rs`: https://github.com/tauri-apps/plugins-workspace/tree/v2/plugins/clipboard-manager
- dotnet/winforms `Clipboard.cs`, `WinFormsOleServices.cs`: https://github.com/dotnet/winforms
- Claude Code 2.1.302 binary strings (local) and docs: https://code.claude.com/docs/en/interactive-mode
- Tauri v2 Calling Rust: https://v2.tauri.app/develop/calling-rust/
- Claude vision limits: https://platform.claude.com/docs/en/build-with-claude/vision
- Traccia `apps/web/components/issue-detail/attachments.tsx` (local checkout, commit 4474d4d)
