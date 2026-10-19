import { readFileSync } from "node:fs";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { mountEditorPage, startEditor } from "../src/editor-page";
import type { EditorIpc } from "../src/ipc";

// jsdom has no canvas. This one keeps real pixels, so what the page puts on a canvas and reads back is checked
// for real; drawing with a transform (the display canvas) is not modelled.
const fakes = new WeakMap<HTMLCanvasElement, FakeContext>();

class FakeContext {
  pixels = new Uint8ClampedArray(0);
  fillStyle = "";
  imageSmoothingQuality = "";
  constructor(private canvas: HTMLCanvasElement) {}
  private fit(): void {
    const length = this.canvas.width * this.canvas.height * 4;
    if (this.pixels.length !== length) this.pixels = new Uint8ClampedArray(length);
  }
  setTransform(): void {}
  fillRect(): void {}
  putImageData(image: ImageData): void {
    this.fit();
    this.pixels.set(image.data);
  }
  drawImage(source: HTMLCanvasElement): void {
    this.fit();
    this.pixels.set(fakes.get(source)!.pixels.subarray(0, this.pixels.length));
  }
  getImageData(): { data: Uint8ClampedArray } {
    this.fit();
    return { data: this.pixels.slice() };
  }
}

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(function (this: HTMLCanvasElement) {
    if (!fakes.has(this)) fakes.set(this, new FakeContext(this));
    return fakes.get(this) as unknown as CanvasRenderingContext2D;
  } as never);
  vi.stubGlobal(
    "ImageData",
    class {
      constructor(
        public data: Uint8ClampedArray,
        public width: number,
        public height: number,
      ) {}
    },
  );
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** A fresh Editor page, loaded from the real index.html, with a canvas box of 800×600 device pixels. */
function loadPage() {
  const doc = document.implementation.createHTMLDocument("Annota");
  doc.documentElement.innerHTML = readFileSync("index.html", "utf8");
  const page = mountEditorPage(doc);
  vi.spyOn(page.canvas, "getBoundingClientRect").mockReturnValue({ width: 800, height: 600 } as DOMRect);
  return { doc, page };
}

/** The Editor started against an IPC that logs every call and lets the test answer `next_snap`. */
function startFakeEditor() {
  const { doc, page } = loadPage();
  const calls: string[] = [];
  const parked: Array<(bytes: ArrayBuffer) => void> = [];
  const copied: Array<{ rgba: Uint8ClampedArray; width: number; height: number }> = [];
  let copyRequested = () => {};
  const ipc: EditorIpc = {
    nextSnap: () => {
      calls.push("nextSnap");
      return new Promise((resolve) => parked.push(resolve));
    },
    painted: async () => void calls.push("painted"),
    copy: async (rgba, width, height) => {
      calls.push("copy");
      copied.push({ rgba, width, height });
    },
    discard: async () => void calls.push("discard"),
    onCopyRequested: (handler) => (copyRequested = handler),
  };
  const frames: Array<() => void> = [];
  const win = {
    document: doc,
    devicePixelRatio: 1,
    ResizeObserver: class {
      observe(): void {}
    },
    matchMedia: () => ({ addEventListener: () => {} }),
    requestAnimationFrame: (callback: () => void) => frames.push(callback),
    setTimeout: () => 0,
  } as unknown as Window & typeof globalThis;
  startEditor(win, page, ipc);
  return {
    doc,
    page,
    calls,
    copied,
    requestCopy: () => copyRequested(),
    /** Rust answers the parked `next_snap` with a Region of the given RGBA. */
    deliver(width: number, height: number, rgba: number[]) {
      const bytes = new Uint8Array(rgba.length + 8);
      bytes.set(rgba);
      new DataView(bytes.buffer).setUint32(rgba.length, width, true);
      new DataView(bytes.buffer).setUint32(rgba.length + 4, height, true);
      parked.shift()!(bytes.buffer);
    },
    /** The next frame is presented. */
    presentFrame: () => frames.splice(0).forEach((frame) => frame()),
    press: (key: string, ctrlKey = false) =>
      doc.dispatchEvent(new KeyboardEvent("keydown", { key, ctrlKey, cancelable: true })),
  };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const REGION = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 7, 8, 9, 255];

it("finds the Editor's one canvas and a hint line that starts at 0 notes", () => {
  const { doc, page } = loadPage();
  expect(page.canvas).toBeInstanceOf(HTMLCanvasElement);
  expect(doc.querySelectorAll("canvas")).toHaveLength(1);
  expect(page.hint.textContent).toBe("0 notes · Caps+S / Ctrl+C copy · Esc discard");
});

it("names a missing canvas or hint instead of failing later", () => {
  const doc = document.implementation.createHTMLDocument("Annota");
  expect(() => mountEditorPage(doc)).toThrow(/canvas/);
  doc.body.innerHTML = '<canvas id="snap"></canvas>';
  expect(() => mountEditorPage(doc)).toThrow(/hint/);
});

it("parks next_snap from load and says it has painted only on the frame after the Region is drawn", async () => {
  const editor = startFakeEditor();
  await settle();
  expect(editor.calls).toEqual(["nextSnap"]);

  editor.deliver(2, 2, REGION);
  await settle();
  expect(editor.calls, "drawn, but no frame has presented it yet").toEqual(["nextSnap"]);

  editor.presentFrame();
  await settle();
  expect(editor.calls).toEqual(["nextSnap", "painted", "nextSnap"]);
});

it("Ctrl+C sends the Region's own pixels at its native size, once", async () => {
  const editor = startFakeEditor();
  editor.deliver(2, 2, REGION);
  await settle();
  editor.presentFrame();
  await settle();

  editor.press("c", true);
  await settle();
  editor.press("c", true);
  await settle();

  expect(editor.copied).toHaveLength(1);
  expect(editor.copied[0].width).toBe(2);
  expect(editor.copied[0].height).toBe(2);
  expect(Array.from(editor.copied[0].rgba)).toEqual(REGION);
});

it("copies when the Resident asks, as --snap does while the Editor is open, and not before there is a Region", async () => {
  const editor = startFakeEditor();
  editor.requestCopy();
  await settle();
  expect(editor.copied).toHaveLength(0);

  editor.deliver(2, 2, REGION);
  await settle();
  editor.presentFrame();
  await settle();
  editor.requestCopy();
  await settle();
  expect(editor.copied).toHaveLength(1);
});

it("Esc discards once and copies nothing afterwards", async () => {
  const editor = startFakeEditor();
  editor.deliver(2, 2, REGION);
  await settle();
  editor.presentFrame();
  await settle();

  editor.press("Escape");
  editor.press("Escape");
  editor.press("c", true);
  await settle();

  expect(editor.calls.filter((call) => call === "discard")).toHaveLength(1);
  expect(editor.copied).toHaveLength(0);
});

it("takes the next Snap after a copy or a discard", async () => {
  const editor = startFakeEditor();
  editor.deliver(2, 2, REGION);
  await settle();
  editor.presentFrame();
  await settle();
  editor.press("Escape");
  await settle();

  const second = [1, 2, 3, 255];
  editor.deliver(1, 1, second);
  await settle();
  editor.presentFrame();
  await settle();
  editor.press("c", true);
  await settle();

  expect(editor.calls.filter((call) => call === "painted")).toHaveLength(2);
  expect(Array.from(editor.copied[0].rgba)).toEqual(second);
});
