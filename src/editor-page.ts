import { renderHint } from "./hint";
import type { EditorIpc } from "./ipc";
import { actionForKey } from "./keys";
import { deviceSize, placeImage, type ObservedBox, type Size } from "./layout";
import { renderSnap } from "./render";
import { parseSnapBytes, type Snap } from "./snap";

/** The page's own background (`index.html`): what the canvas shows around a Region that doesn't fill it. */
const BACKGROUND = "#2a2a2f";

/** How long to wait before parking `next_snap` again after a failure. */
const RETRY_MS = 250;

export interface EditorPage {
  canvas: HTMLCanvasElement;
  hint: HTMLElement;
}

/** The Editor's page as `index.html` lays it out. */
export function mountEditorPage(doc: Document): EditorPage {
  const canvas = doc.querySelector<HTMLCanvasElement>("canvas#snap");
  if (!canvas) throw new Error("the Editor page has no canvas#snap");
  const hint = doc.querySelector<HTMLElement>("#hint");
  if (!hint) throw new Error("the Editor page has no #hint");
  renderHint(hint, 0);
  return { canvas, hint };
}

/**
 * Runs the Editor: parks `next_snap`, puts each Region on the canvas, tells Rust once a frame has
 * presented it, and copies or discards on Ctrl+C and Esc.
 */
export function startEditor(win: Window & typeof globalThis, page: EditorPage, ipc: EditorIpc): void {
  const { canvas } = page;
  const doc = win.document;
  const ctx = canvas.getContext("2d", { alpha: false });
  if (!ctx) throw new Error("the Editor's canvas has no 2d context");

  let snap: Snap | null = null;
  /** A copy or a discard is in flight: Rust is closing the Editor, so a second key press does nothing. */
  let closing = false;
  /** Set once the observer has reported the canvas's exact device box, which `measure()` only estimates. */
  let observed = false;
  /** The canvas's backing store: its content box in device pixels. */
  let box: Size = { width: 0, height: 0 };

  /** The Region at 100% when it fits, fitted when it doesn't, 1:1 in device pixels either way. */
  function paint(): void {
    if (canvas.width !== box.width || canvas.height !== box.height) {
      canvas.width = box.width;
      canvas.height = box.height;
    }
    ctx!.setTransform(1, 0, 0, 1, 0, 0);
    ctx!.fillStyle = BACKGROUND;
    ctx!.fillRect(0, 0, box.width, box.height);
    if (!snap) return;
    const { scale, x, y } = placeImage(box, snap);
    ctx!.setTransform(scale, 0, 0, scale, x, y);
    ctx!.imageSmoothingQuality = "high";
    renderSnap(ctx!, snap, { forExport: false });
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
    closing = true;
    try {
      await ipc.copy(exportSnap(snap), snap.width, snap.height);
      forget();
    } catch (error) {
      console.error("copy failed", error);
    } finally {
      closing = false;
    }
  }

  /** The Snap is over: don't keep its pixels on the hidden canvas. */
  function forget(): void {
    snap = null;
    paint();
  }

  async function discard(): Promise<void> {
    if (!snap || closing) return;
    closing = true;
    try {
      await ipc.discard();
      forget();
    } catch (error) {
      console.error("discard failed", error);
    } finally {
      closing = false;
    }
  }

  doc.addEventListener("keydown", (event) => {
    const action = actionForKey(event);
    if (!action || event.repeat) return;
    event.preventDefault();
    void (action === "copy" ? copy() : discard());
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
        const image = doc.createElement("canvas");
        image.width = width;
        image.height = height;
        image.getContext("2d")!.putImageData(new ImageData(pixels, width, height), 0, 0);
        snap = { width, height, image };
        if (!observed) box = measure();
        paint();
        await nextFrame();
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
