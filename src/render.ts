import type { Snap } from "./snap";

export interface RenderOptions {
  /**
   * True for the PNG that Ctrl+C copies: only what the agent should see. False for the Editor's canvas,
   * which also shows what is only for editing (a selected Box's outline and handles).
   */
  forExport: boolean;
}

/**
 * The one function that draws a Snap, for the Editor's canvas and for the export alike, in image pixels.
 * The caller sets the transform (the Editor fits the image to its window; the export is 1:1). The Box and
 * Callout issues add Boxes, badges and Callouts here, in that order, with the editing-only parts behind
 * `!options.forExport`.
 */
export function renderSnap(ctx: CanvasRenderingContext2D, snap: Snap, options: RenderOptions): void {
  void options;
  ctx.drawImage(snap.image, 0, 0);
}
