import { badgeFor, cornerPoints, type Rect } from "./boxes";
import { sizingFor, type Sizing } from "./sizing";
import type { Snap } from "./snap";

export interface RenderOptions {
  /**
   * True for the PNG that Ctrl+C copies: only what the agent should see. False for the Editor's canvas,
   * which also shows what is only for editing (a selected Box's outline and handles, the Box being drawn).
   */
  forExport: boolean;
  /** The selected Box, or -1. Editing only. */
  selected?: number;
  /** The Box being dragged out, drawn dashed. Editing only. */
  preview?: Rect | null;
  /** Image pixels per CSS pixel, so the outline and handles keep their on-screen size. Editing only. */
  imagePerCss?: number;
}

const FAMILY = '"Segoe UI", system-ui, sans-serif';
const RED = "#e5484d";
const BLUE = "#0a84ff";

/**
 * The one function that draws a Snap, for the Editor's canvas and for the export alike, in image pixels.
 * The caller sets the transform (the Editor fits the image to its window; the export is 1:1). The Callout
 * issue adds Callouts after the Boxes, with the editing-only parts behind `!options.forExport`.
 */
export function renderSnap(ctx: CanvasRenderingContext2D, snap: Snap, options: RenderOptions): void {
  ctx.drawImage(snap.image, 0, 0);
  const sizing = sizingFor(snap.width, snap.height);
  snap.boxes.forEach((box, index) => drawBox(ctx, box, index + 1, sizing, snap));
  if (options.forExport) return;
  if (options.preview) drawBox(ctx, options.preview, snap.boxes.length + 1, sizing, snap, true);
  const selected = snap.boxes[options.selected ?? -1];
  if (selected) drawSelection(ctx, selected, sizing, options.imagePerCss ?? 1);
}

/** A Box (spec 8.2): faint fill, red stroke, then its badge on the top-left corner. */
function drawBox(ctx: CanvasRenderingContext2D, box: Rect, number: number, sizing: Sizing, image: Snap, preview = false): void {
  ctx.save();
  if (preview) ctx.setLineDash([sizing.S * 3, sizing.S * 2]);
  ctx.fillStyle = "rgba(229,72,77,0.07)";
  ctx.fillRect(box.x, box.y, box.w, box.h);
  ctx.lineWidth = sizing.S;
  ctx.strokeStyle = RED;
  ctx.beginPath();
  ctx.roundRect(box.x, box.y, box.w, box.h, sizing.S);
  ctx.stroke();
  ctx.setLineDash([]);

  const { cx, cy, r } = badgeFor(box, sizing, image);
  ctx.beginPath();
  ctx.arc(cx, cy, r, 0, Math.PI * 2);
  ctx.fillStyle = RED;
  ctx.fill();
  ctx.fillStyle = "#fff";
  ctx.font = `700 ${Math.round(sizing.D * 0.58)}px ${FAMILY}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(String(number), cx, cy + r * 0.05);
  ctx.restore();
}

/** The selected Box in the Editor: a dashed blue outline and four white corner handles, sized in CSS pixels. */
function drawSelection(ctx: CanvasRenderingContext2D, box: Rect, sizing: Sizing, unit: number): void {
  const offset = sizing.S / 2 + 3 * unit;
  const handle = 9 * unit;
  ctx.save();
  ctx.lineWidth = 1.5 * unit;
  ctx.strokeStyle = BLUE;
  ctx.setLineDash([5 * unit, 4 * unit]);
  ctx.strokeRect(box.x - offset, box.y - offset, box.w + 2 * offset, box.h + 2 * offset);
  ctx.setLineDash([]);
  ctx.fillStyle = "#fff";
  for (const [, at] of cornerPoints(box)) {
    ctx.fillRect(at.x - handle / 2, at.y - handle / 2, handle, handle);
    ctx.strokeRect(at.x - handle / 2, at.y - handle / 2, handle, handle);
  }
  ctx.restore();
}
