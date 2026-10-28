import {
  hitTest,
  moveRect,
  normalize,
  resizeRect,
  type Box,
  type Corner,
  type Point,
  type Rect,
} from "./boxes";
import type { Size } from "./layout";
import { sizingFor } from "./sizing";

/** A drag shorter than this, in CSS pixels, draws no Box (spec 7). */
export const MIN_DRAW_CSS_PX = 6;
/** How far past a Box's edge, in CSS pixels, a click still hits it. */
const HIT_CSS_PX = 6;

/** What the Editor holds for one Snap: its size and the Boxes drawn on it. */
export interface Scene extends Size {
  boxes: Box[];
}

type Drag =
  | { mode: "draw"; start: Point; rect: Rect }
  | { mode: "move"; index: number; start: Point; original: Rect }
  | { mode: "resize"; index: number; corner: Corner; start: Point; original: Rect };

export type Cursor = "crosshair" | "move" | "pointer" | "nwse-resize" | "nesw-resize";

/**
 * The pointer's half of the Editor (spec 7): draw, select, move, resize, delete, all in image pixels. It
 * keeps which Box is selected and which has its Note field open; the page draws and shows the field.
 */
export class Interaction {
  /** The selected Box, or -1. */
  selected = -1;
  /** The Box whose Note field is open, or -1. */
  editing = -1;
  /** The Box being drawn, while the button is down. */
  preview: Rect | null = null;

  private drag: Drag | null = null;
  private readonly strokeWidth: number;
  /** The Callouts as last laid out, and what they were laid out for: the Boxes, their Notes and `editing`. */
  private drawn: ReadonlyArray<Rect | null> | null = null;
  private drawnFor = "";

  /** @param cssPerImage how many CSS pixels one image pixel takes on screen right now. */
  constructor(
    private readonly scene: Scene,
    private readonly cssPerImage: () => number,
    /** The Callouts as drawn now, by Box (null where there is none), so a click on one can find its Box. */
    private readonly callouts: () => ReadonlyArray<Rect | null> = () => [],
  ) {
    this.strokeWidth = sizingFor(scene.width, scene.height).S;
  }

  /** A button is down on the canvas. */
  get dragging(): boolean {
    return this.drag !== null;
  }

  /** The Callouts as drawn now, laid out again only when a Box, a Note or `editing` has changed since. */
  private drawnCallouts(): ReadonlyArray<Rect | null> {
    const key = JSON.stringify([this.editing, this.scene.boxes.map(({ x, y, w, h, note }) => [x, y, w, h, note])]);
    if (key !== this.drawnFor) {
      this.drawn = this.callouts();
      this.drawnFor = key;
    }
    return this.drawn ?? [];
  }

  private hit(point: Point) {
    const tolerance = HIT_CSS_PX / this.cssPerImage();
    const callouts = this.drawnCallouts();
    return hitTest(this.scene.boxes, this.selected, point, tolerance, tolerance + this.strokeWidth, callouts);
  }

  /** The cursor over a point while no button is down. */
  cursorAt(point: Point): Cursor {
    const hit = this.hit(point);
    if (hit.type === "none") return "crosshair";
    if (hit.type === "box") return "move";
    if (hit.type === "callout") return "pointer";
    return hit.corner === "nw" || hit.corner === "se" ? "nwse-resize" : "nesw-resize";
  }

  pointerDown(point: Point): void {
    const hit = this.hit(point);
    if (hit.type === "handle") {
      this.drag = { mode: "resize", index: hit.index, corner: hit.corner, start: point, original: { ...this.scene.boxes[hit.index] } };
    } else if (hit.type === "callout") {
      // A click on a Callout selects its Box; only the Box itself is dragged.
      this.selected = hit.index;
    } else if (hit.type === "box") {
      this.selected = hit.index;
      this.drag = { mode: "move", index: hit.index, start: point, original: { ...this.scene.boxes[hit.index] } };
    } else {
      this.selected = -1;
      this.drag = { mode: "draw", start: point, rect: { x: point.x, y: point.y, w: 0, h: 0 } };
      this.preview = normalize(this.drag.rect);
    }
  }

  pointerMove(point: Point): void {
    const drag = this.drag;
    if (!drag) return;
    const dx = point.x - drag.start.x;
    const dy = point.y - drag.start.y;
    if (drag.mode === "draw") {
      drag.rect = { x: drag.start.x, y: drag.start.y, w: dx, h: dy };
      this.preview = normalize(drag.rect);
      return;
    }
    const box = this.scene.boxes[drag.index];
    Object.assign(
      box,
      drag.mode === "move"
        ? moveRect(drag.original, dx, dy, this.scene)
        : resizeRect(drag.original, drag.corner, dx, dy, this.scene),
    );
  }

  /** The button is released. Returns the new Box's index when a Box was drawn: its Note field opens. */
  pointerUp(): number | null {
    const drag = this.drag;
    this.drag = null;
    this.preview = null;
    if (!drag) return null;
    const minimum = MIN_DRAW_CSS_PX / this.cssPerImage();
    if (drag.mode === "resize") {
      // A Box dragged down to a sliver would be nearly impossible to grab again: put it back as it was.
      const box = this.scene.boxes[drag.index];
      if (box.w < minimum || box.h < minimum) Object.assign(box, drag.original);
      return null;
    }
    if (drag.mode !== "draw") return null;
    const rect = normalize(drag.rect);
    if (rect.w < minimum || rect.h < minimum) return null;
    this.scene.boxes.push({ ...rect, note: "" });
    this.selected = this.scene.boxes.length - 1;
    return this.selected;
  }

  /** Opens the Note field of the Box under a point (double-click). Returns its index, or null. */
  editAt(point: Point): number | null {
    const hit = this.hit(point);
    if (hit.type === "none") return null;
    return this.startEditing(hit.index);
  }

  /** Opens the Note field of the selected Box (Enter). */
  editSelected(): number | null {
    return this.selected < 0 ? null : this.startEditing(this.selected);
  }

  startEditing(index: number): number {
    this.selected = index;
    this.editing = index;
    return index;
  }

  stopEditing(): void {
    this.editing = -1;
  }

  /** Deletes the selected Box: the Boxes after it move up one number. */
  removeSelected(): boolean {
    if (this.selected < 0) return false;
    this.scene.boxes.splice(this.selected, 1);
    this.selected = -1;
    this.editing = -1;
    return true;
  }
}
