import type { Size } from "./layout";
import type { Sizing } from "./sizing";

export interface Point {
  x: number;
  y: number;
}

export interface Rect extends Point {
  w: number;
  h: number;
}

/** A Box in image pixels. Its number is its place in the Snap's list, so deleting one renumbers the rest. */
export interface Box extends Rect {
  /** The Note, which may be empty. */
  note: string;
}

export type Corner = "nw" | "ne" | "sw" | "se";

export type Hit =
  | { type: "handle"; index: number; corner: Corner }
  | { type: "box"; index: number }
  | { type: "callout"; index: number }
  | { type: "none" };

const clamp = (value: number, low: number, high: number): number => Math.max(low, Math.min(high, value));

/** The same rectangle with a positive width and height, whichever way it was dragged. */
export function normalize(rect: Rect): Rect {
  return {
    x: Math.min(rect.x, rect.x + rect.w),
    y: Math.min(rect.y, rect.y + rect.h),
    w: Math.abs(rect.w),
    h: Math.abs(rect.h),
  };
}

/** The rectangle moved by (dx, dy), kept inside the image. */
export function moveRect(rect: Rect, dx: number, dy: number, bounds: Size): Rect {
  return {
    x: clamp(rect.x + dx, 0, bounds.width - rect.w),
    y: clamp(rect.y + dy, 0, bounds.height - rect.h),
    w: rect.w,
    h: rect.h,
  };
}

/**
 * The rectangle with one corner dragged by (dx, dy), the opposite corner fixed, kept inside the image and
 * normalised so a drag across the opposite corner flips the rectangle instead of inverting it.
 */
export function resizeRect(rect: Rect, corner: Corner, dx: number, dy: number, bounds: Size): Rect {
  const west = corner.includes("w");
  const north = corner.includes("n");
  const x1 = west ? clamp(rect.x + dx, 0, bounds.width) : rect.x;
  const x2 = west ? rect.x + rect.w : clamp(rect.x + rect.w + dx, 0, bounds.width);
  const y1 = north ? clamp(rect.y + dy, 0, bounds.height) : rect.y;
  const y2 = north ? rect.y + rect.h : clamp(rect.y + rect.h + dy, 0, bounds.height);
  return normalize({ x: x1, y: y1, w: x2 - x1, h: y2 - y1 });
}

export function cornerPoints(rect: Rect): Array<[Corner, Point]> {
  return [
    ["nw", { x: rect.x, y: rect.y }],
    ["ne", { x: rect.x + rect.w, y: rect.y }],
    ["sw", { x: rect.x, y: rect.y + rect.h }],
    ["se", { x: rect.x + rect.w, y: rect.y + rect.h }],
  ];
}

/**
 * What a point lands on: a handle of the selected Box first, then the topmost Callout (they draw over the
 * Boxes), then the topmost Box (later Boxes draw over earlier ones). `tolerance` widens a Box's edge and
 * `handleTolerance` a handle, both in image pixels. `callouts` are the drawn Callouts by Box, null for none.
 */
export function hitTest(
  boxes: readonly Box[],
  selected: number,
  point: Point,
  tolerance: number,
  handleTolerance: number,
  callouts: ReadonlyArray<Rect | null> = [],
): Hit {
  const chosen = boxes[selected];
  if (chosen) {
    for (const [corner, at] of cornerPoints(chosen)) {
      if (Math.abs(point.x - at.x) <= handleTolerance && Math.abs(point.y - at.y) <= handleTolerance) {
        return { type: "handle", index: selected, corner };
      }
    }
  }
  for (let index = callouts.length - 1; index >= 0; index--) {
    const callout = callouts[index];
    if (
      callout &&
      point.x >= callout.x &&
      point.x <= callout.x + callout.w &&
      point.y >= callout.y &&
      point.y <= callout.y + callout.h
    ) {
      return { type: "callout", index };
    }
  }
  for (let index = boxes.length - 1; index >= 0; index--) {
    const box = boxes[index];
    if (
      point.x >= box.x - tolerance &&
      point.x <= box.x + box.w + tolerance &&
      point.y >= box.y - tolerance &&
      point.y <= box.y + box.h + tolerance
    ) {
      return { type: "box", index };
    }
  }
  return { type: "none" };
}

/** The number badge: centred on the Box's top-left corner, pulled inside the image. */
export function badgeFor(box: Rect, sizing: Sizing, image: Size): { cx: number; cy: number; r: number } {
  const r = sizing.D / 2;
  return {
    cx: clamp(box.x, r, image.width - r),
    cy: clamp(box.y, r, image.height - r),
    r,
  };
}
