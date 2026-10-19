import { renderHint } from "./hint";
import { calloutLayouts, canvasMeasure } from "./callout";
import { Interaction } from "./interaction";
import type { EditorIpc } from "./ipc";
import { actionForKey } from "./keys";
import {
  deviceSize,
  noteCardWidth,
  placeImage,
  placeNoteCard,
  type ObservedBox,
  type Placed,
  type Size,
} from "./layout";
import { openNoteField, type NoteField } from "./note-field";
import { renderSnap } from "./render";
import { sizingFor } from "./sizing";
import { parseSnapBytes, type Snap } from "./snap";

/** The page's own background (`index.html`): what the canvas shows around a Region that doesn't fill it. */
const BACKGROUND = "#2a2a2f";

/** How long to wait before parking `next_snap` again after a failure. */
const RETRY_MS = 250;

export interface EditorPage {
  canvas: HTMLCanvasElement;
  hint: HTMLElement;
  /** Holds the Note field's card above the canvas. */
  overlay: HTMLElement;
}

/** The Editor's page as `index.html` lays it out. */
export function mountEditorPage(doc: Document): EditorPage {
  const canvas = doc.querySelector<HTMLCanvasElement>("canvas#snap");
  if (!canvas) throw new Error("the Editor page has no canvas#snap");
  const hint = doc.querySelector<HTMLElement>("#hint");
  if (!hint) throw new Error("the Editor page has no #hint");
  const overlay = doc.querySelector<HTMLElement>("#overlay");
  if (!overlay) throw new Error("the Editor page has no #overlay");
  renderHint(hint, 0);
  return { canvas, hint, overlay };
}

/**
 * Runs the Editor: parks `next_snap`, puts each Region on the canvas, tells Rust once a frame has
 * presented it, lets the user draw, edit and delete Boxes with Notes, and copies or discards on Ctrl+C and Esc.
 */
export function startEditor(win: Window & typeof globalThis, page: EditorPage, ipc: EditorIpc): void {
  const { canvas } = page;
  const doc = win.document;
  const ctx = canvas.getContext("2d", { alpha: false });
  if (!ctx) throw new Error("the Editor's canvas has no 2d context");

  let snap: Snap | null = null;
  let interaction: Interaction | null = null;
  /** The open Note field, if any. Its text goes into its Box as it is typed, so closing it never loses text. */
  let note: NoteField | null = null;
  /** Where the image sits on the canvas, in device pixels. */
  let placed: Placed = { scale: 1, x: 0, y: 0 };
  /** A copy or a discard is in flight: Rust is closing the Editor, so a second key press does nothing. */
  let closing = false;
  /** Set once the observer has reported the canvas's exact device box, which `measure()` only estimates. */
  let observed = false;
  /** The canvas's backing store: its content box in device pixels. */
  let box: Size = { width: 0, height: 0 };
  /** Measures Callout text for clicks, apart from the visible canvas. */
  const measuring = doc.createElement("canvas").getContext("2d") as CanvasRenderingContext2D;

  /** How many CSS pixels one image pixel takes on screen now. */
  function cssPerImage(): number {
    return placed.scale / win.devicePixelRatio;
  }

  /** The Region at 100% when it fits, fitted when it doesn't, 1:1 in device pixels either way. */
  function paint(): void {
    if (canvas.width !== box.width || canvas.height !== box.height) {
      canvas.width = box.width;
      canvas.height = box.height;
    }
    ctx!.setTransform(1, 0, 0, 1, 0, 0);
    ctx!.fillStyle = BACKGROUND;
    ctx!.fillRect(0, 0, box.width, box.height);
    if (!snap || !interaction) return;
    placed = placeImage(box, snap);
    ctx!.setTransform(placed.scale, 0, 0, placed.scale, placed.x, placed.y);
    ctx!.imageSmoothingQuality = "high";
    renderSnap(ctx!, snap, {
      forExport: false,
      editing: interaction.editing,
      selected: interaction.selected,
      preview: interaction.preview,
      imagePerCss: 1 / cssPerImage(),
    });
    positionNote();
  }

  /** Paint, and count the Boxes in the hint line. */
  function changed(): void {
    paint();
    renderHint(page.hint, snap?.boxes.length ?? 0);
  }

  /** The Note field under its Box (above it if there is no room), in CSS pixels in the stage. */
  function positionNote(): void {
    if (!snap || !note || !interaction || interaction.editing < 0) return;
    const editing = snap.boxes[interaction.editing];
    const k = cssPerImage();
    const dpr = win.devicePixelRatio;
    const stage = canvas.getBoundingClientRect();
    const at = placeNoteCard(
      { x: placed.x / dpr + editing.x * k, y: placed.y / dpr + editing.y * k, w: editing.w * k, h: editing.h * k },
      (sizingFor(snap.width, snap.height).D / 2) * k,
      note.size(),
      { width: stage.width, height: stage.height },
    );
    note.place(at.left, at.top);
  }

  function openNote(index: number): void {
    if (!snap || !interaction) return;
    closeNote();
    const target = snap.boxes[interaction.startEditing(index)];
    note = openNoteField(doc, page.overlay, {
      number: index + 1,
      text: target.note,
      width: noteCardWidth(sizingFor(snap.width, snap.height).cap, cssPerImage()),
      onInput: (text) => (target.note = text),
      onResize: positionNote,
    });
    changed();
  }

  /** Enter, Esc, a click elsewhere and a copy all end here: the text is already in its Box. */
  function closeNote(): void {
    if (!note) return;
    note.close();
    note = null;
    interaction?.stopEditing();
  }

  function measure(): Size {
    const rect = canvas.getBoundingClientRect();
    return {
      width: Math.round(rect.width * win.devicePixelRatio),
      height: Math.round(rect.height * win.devicePixelRatio),
    };
  }

  function resized(size: Size): void {
    if (size.width === box.width && size.height === box.height) return;
    box = size;
    paint();
  }

  new win.ResizeObserver((entries) => {
    const entry = entries[entries.length - 1] as ObservedBox | undefined;
    if (!entry) return;
    observed = true;
    resized(deviceSize(entry, win.devicePixelRatio));
  }).observe(canvas, { box: "device-pixel-content-box" });

  // The Editor dragged to a monitor with another scale: redo the layout so the image isn't resampled.
  function watchScale(): void {
    win.matchMedia(`(resolution: ${win.devicePixelRatio}dppx)`).addEventListener(
      "change",
      () => {
        box = { width: -1, height: -1 };
        resized(measure());
        watchScale();
      },
      { once: true },
    );
  }
  watchScale();

  /** A pointer position in image pixels, kept inside the image. */
  function toImage(event: MouseEvent): { x: number; y: number } {
    const rect = canvas.getBoundingClientRect();
    const dpr = win.devicePixelRatio;
    const x = ((event.clientX - rect.left) * dpr - placed.x) / placed.scale;
    const y = ((event.clientY - rect.top) * dpr - placed.y) / placed.scale;
    return { x: Math.max(0, Math.min(snap!.width, x)), y: Math.max(0, Math.min(snap!.height, y)) };
  }

  canvas.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || !snap || !interaction || closing) return;
    closeNote();
    interaction.pointerDown(toImage(event));
    canvas.setPointerCapture?.(event.pointerId);
    changed();
  });
  canvas.addEventListener("pointermove", (event) => {
    if (!snap || !interaction) return;
    const at = toImage(event);
    if (interaction.dragging) {
      interaction.pointerMove(at);
      paint();
    } else {
      canvas.style.cursor = interaction.cursorAt(at);
    }
  });
  function released(): void {
    if (!interaction?.dragging) return;
    const drawn = interaction.pointerUp();
    if (drawn === null) changed();
    else openNote(drawn);
  }
  canvas.addEventListener("pointerup", released);
  canvas.addEventListener("pointercancel", released);
  canvas.addEventListener("dblclick", (event) => {
    if (!snap || !interaction || closing) return;
    closeNote();
    const index = interaction.editAt(toImage(event));
    if (index !== null) openNote(index);
  });

  /** What Ctrl+C copies: the Region at its native size, drawn by the same function as the canvas. */
  function exportSnap(current: Snap): Uint8ClampedArray {
    const out = doc.createElement("canvas");
    out.width = current.width;
    out.height = current.height;
    const outCtx = out.getContext("2d");
    if (!outCtx) throw new Error("the export canvas has no 2d context");
    renderSnap(outCtx, current, { forExport: true });
    return outCtx.getImageData(0, 0, current.width, current.height).data;
  }

  async function copy(): Promise<void> {
    if (!snap || closing) return;
    // An open Note is already in its Box, so closing its field is all the saving there is.
    closeNote();
    closing = true;
    try {
      const rgba = exportSnap(snap);
      ipc.mark?.("page_exported");
      await ipc.copy(rgba, snap.width, snap.height, snap.pixels);
      forget();
    } catch (error) {
      console.error("copy failed", error);
      changed();
    } finally {
      closing = false;
    }
  }

  /** The Snap is over: don't keep its pixels or its Boxes on the hidden canvas. */
  function forget(): void {
    closeNote();
    snap = null;
    interaction = null;
    changed();
  }

  async function discard(): Promise<void> {
    if (!snap || closing) return;
    closeNote();
    closing = true;
    try {
      await ipc.discard();
      forget();
    } catch (error) {
      console.error("discard failed", error);
      changed();
    } finally {
      closing = false;
    }
  }

  doc.addEventListener("keydown", (event) => {
    const action = actionForKey(event, {
      noteOpen: note !== null,
      textSelected: note?.hasSelection() ?? false,
      boxSelected: (interaction?.selected ?? -1) >= 0,
    });
    if (!action) return;
    event.preventDefault();
    if (event.repeat || closing) return;
    switch (action) {
      case "copy":
        ipc.mark?.("copy_key");
        void copy();
        break;
      case "discard":
        void discard();
        break;
      case "close-note":
        closeNote();
        changed();
        break;
      case "edit": {
        const index = interaction?.editSelected();
        if (index !== null && index !== undefined) openNote(index);
        break;
      }
      case "delete":
        if (interaction?.removeSelected()) changed();
        break;
    }
  });
  ipc.onCopyRequested(() => void copy());

  function nextFrame(): Promise<void> {
    return new Promise((resolve) => win.requestAnimationFrame(() => resolve()));
  }

  /** One Snap at a time: park, paint, say so on the next frame, park again. */
  async function run(): Promise<void> {
    for (;;) {
      try {
        const { width, height, pixels } = parseSnapBytes(await ipc.nextSnap());
        ipc.mark?.("page_bytes");
        const image = doc.createElement("canvas");
        image.width = width;
        image.height = height;
        image.getContext("2d")!.putImageData(new ImageData(pixels, width, height), 0, 0);
        ipc.mark?.("page_drawn");
        closeNote();
        snap = { width, height, image, boxes: [], pixels };
        const current = snap;
        interaction = new Interaction(current, cssPerImage, () =>
          calloutLayouts(canvasMeasure(measuring, sizingFor(width, height).F), current, interaction?.editing ?? -1),
        );
        if (!observed) box = measure();
        changed();
        await nextFrame();
        ipc.mark?.("page_frame");
        // Rust sized the window before it sent the pixels; if the observer hasn't spoken yet, look again.
        if (!observed) resized(measure());
        await ipc.painted();
      } catch (error) {
        console.error("snap failed", error);
        await new Promise((resolve) => win.setTimeout(resolve, RETRY_MS));
      }
    }
  }

  box = measure();
  paint();
  void run();
}
