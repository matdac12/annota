# App icon decision

**Chosen: option 3, the Callout** (`docs/mockups/icon-annota.svg`).
The other four prototypes (numbered Box, Region, boxed A, marked Snap) and the
comparison page are in `prototype/icon/`; they are throwaway.

## Why

One idea: a red Box with a yellow sticky Note laid over it, which is what the
copied PNG contains (a Box and its Callout). It keeps its shape at 16 px.

## Palette departs from the scrivi/Tasca family

Same rounded tile geometry, gloss and edge as scrivi and Tasca, but a navy tile
(`#2D3C5E` to `#172033`), red Box (`#F25C60` to `#D13439`) and yellow Callout
(`#FCD65A` to `#F0B323`). Reason: in the tray, three ivory-and-rust icons would
be indistinguishable. Red and yellow are also the Box and Callout colours the
app itself draws.

## What the build ticket must do

- **Small variant for 16 and 20 px** (`icon-annota-small.svg`): thicker Box
  stroke, drop the two lines on the sticky (they blur), no gloss. Check it
  first; if the 16 px render of the full icon reads fine, skip it.
- **Regenerate `src-tauri/icons`** from the SVG with the Tauri icon pipeline
  (`tauri icon`), then replace the 16/20/24 `.ico` entries with the small variant.
- **Tray icon**: use the small variant. Check it on light and dark taskbars; the
  navy tile is the risk on dark ones (the thin light edge on the tile is there
  for that).
