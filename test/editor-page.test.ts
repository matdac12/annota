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
  measureText(text: string): { width: number } {
    return { width: text.length * 8 };
  }
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

/** Strokes, arcs and text draw nothing the tests read back: every method the fake does not model does nothing. */
function withDrawingNoOps(fake: FakeContext): CanvasRenderingContext2D {
  return new Proxy(fake, {
    get: (target, key) => (key in target ? Reflect.get(target, key) : () => {}),
  }) as unknown as CanvasRenderingContext2D;
}

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(function (this: HTMLCanvasElement) {
    if (!fakes.has(this)) fakes.set(this, new FakeContext(this));
    return withDrawingNoOps(fakes.get(this)!);
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
  vi.spyOn(page.canvas, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: 800, height: 600 } as DOMRect);
  return { doc, page };
}

/** The Editor started against an IPC that logs every call and lets the test answer `next_snap`. */
function startFakeEditor() {
  const { doc, page } = loadPage();
  const calls: string[] = [];
  const parked: Array<(bytes: ArrayBuffer) => void> = [];
  const copied: Array<{ rgba: Uint8ClampedArray; width: number; height: number }> = [];
  let copyRequested = () => {};
  let closeRequested = () => {};
  let copyGate: Promise<void> | null = null;
  let releaseCopy = () => {};
  const ipc: EditorIpc = {
    nextSnap: () => {
      calls.push("nextSnap");
      return new Promise((resolve) => parked.push(resolve));
    },
    painted: async () => void calls.push("painted"),
    copy: async (rgba, width, height) => {
      calls.push("copy");
      copied.push({ rgba, width, height });
      await copyGate;
    },
    discard: async () => void calls.push("discard"),
    onCopyRequested: (handler) => (copyRequested = handler),
    onClosed: (handler) => (closeRequested = handler),
  };
  const frames: Array<() => void> = [];
  const observers: Array<(entries: unknown[]) => void> = [];
  const rescaled: Array<() => void> = [];
  const win = {
    document: doc,
    devicePixelRatio: 1,
    ResizeObserver: class {
      constructor(callback: (entries: unknown[]) => void) {
        observers.push(callback);
      }
      observe(): void {}
    },
    matchMedia: () => ({ addEventListener: (_event: string, handler: () => void) => rescaled.push(handler) }),
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
    /** The title-bar ✕: Rust discards the Snap and hides the Editor, and tells the page it closed. */
    close: () => closeRequested(),
    /** The next copy stays in flight until `releaseCopy`. */
    holdCopy() {
      copyGate = new Promise<void>((resolve) => (releaseCopy = resolve));
    },
    releaseCopy: () => releaseCopy(),
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
    /** A mouse event on the canvas, at a client position (the canvas sits at 0,0 at 1:1). */
    mouse: (type: "pointerdown" | "pointermove" | "pointerup" | "dblclick", x: number, y: number) =>
      page.canvas.dispatchEvent(new MouseEvent(type, { clientX: x, clientY: y, bubbles: true })),
    /** Drags out a Box between two client positions. */
    dragBox(from: [number, number], to: [number, number]) {
      const at = (type: string, [x, y]: [number, number]) =>
        page.canvas.dispatchEvent(new MouseEvent(type, { clientX: x, clientY: y, bubbles: true }));
      at("pointerdown", from);
      at("pointermove", to);
      at("pointerup", to);
    },
    card: () => doc.querySelector<HTMLElement>(".note-card"),
    field: () => doc.querySelector<HTMLTextAreaElement>(".note-card textarea"),
    /** The Editor window is resized: its canvas is now `width`×`height` device pixels. */
    resize(width: number, height: number) {
      const dpr = win.devicePixelRatio;
      const css = { left: 0, top: 0, width: width / dpr, height: height / dpr };
      vi.mocked(page.canvas.getBoundingClientRect).mockReturnValue(css as DOMRect);
      const entry = { devicePixelContentBoxSize: [{ inlineSize: width, blockSize: height }], contentRect: css };
      for (const report of observers) report([entry]);
    },
    /** The Editor moves to a monitor at another scale. */
    rescale(ratio: number) {
      (win as { devicePixelRatio: number }).devicePixelRatio = ratio;
      for (const changed of rescaled.splice(0)) changed();
    },
    typeNote(text: string) {
      const field = doc.querySelector<HTMLTextAreaElement>(".note-card textarea")!;
      field.value = text;
      field.dispatchEvent(new Event("input", { bubbles: true }));
    },
  };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const REGION = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 7, 8, 9, 255];
/** A 400×300 Region, which the 800×600 test canvas shows at 100% with its corner at (200, 150). */
const BIG = new Array<number>(400 * 300 * 4).fill(0);

/** The Editor with a 400×300 Region on screen. */
async function editorWithRegion() {
  const editor = startFakeEditor();
  editor.deliver(400, 300, BIG);
  await settle();
  editor.presentFrame();
  await settle();
  return editor;
}

it("finds the Editor's one canvas and a hint line that starts at 0 notes", () => {
  const { doc, page } = loadPage();
  expect(page.canvas).toBeInstanceOf(HTMLCanvasElement);
  expect(doc.querySelectorAll("canvas")).toHaveLength(1);
  expect(page.hint.textContent).toBe("0 notes · Caps+S / Ctrl+C copy · Esc discard");
});

it("names a missing canvas, hint or overlay instead of failing later", () => {
  const doc = document.implementation.createHTMLDocument("Annota");
  expect(() => mountEditorPage(doc)).toThrow(/canvas/);
  doc.body.innerHTML = '<canvas id="snap"></canvas>';
  expect(() => mountEditorPage(doc)).toThrow(/hint/);
  doc.body.innerHTML = '<canvas id="snap"></canvas><div id="hint"></div>';
  expect(() => mountEditorPage(doc)).toThrow(/overlay/);
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

it("opens a focused Note field when a Box is drawn, and the hint counts the Box", async () => {
  const editor = await editorWithRegion();
  // The page's document has no window in jsdom, so nothing can really take focus: watch the call instead.
  const focused = vi.spyOn(HTMLTextAreaElement.prototype, "focus");
  editor.dragBox([250, 200], [350, 260]);

  expect(editor.card()).not.toBeNull();
  expect(editor.card()!.querySelector(".note-num")!.textContent).toBe("1");
  expect(focused.mock.contexts).toEqual([editor.field()]);
  expect(editor.page.hint.textContent).toBe("1 note · Caps+S / Ctrl+C copy · Esc discard");
});

it("draws nothing for a click or a drag under 6 CSS px", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [254, 260]);
  editor.dragBox([300, 200], [300, 200]);
  expect(editor.card()).toBeNull();
  expect(editor.page.hint.textContent).toMatch(/^0 notes/);
});

it("Enter saves and closes the Note field; Esc then discards the Snap", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  editor.typeNote("the save button");
  editor.press("Enter");
  expect(editor.card()).toBeNull();
  expect(editor.calls).not.toContain("discard");

  editor.press("Escape");
  await settle();
  expect(editor.calls.filter((call) => call === "discard")).toHaveLength(1);
});

it("Esc in an open Note field closes it and keeps the text; Esc again discards", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  editor.typeNote("keep me");

  editor.press("Escape");
  await settle();
  expect(editor.card()).toBeNull();
  expect(editor.calls).not.toContain("discard");

  editor.press("Enter");
  expect(editor.field()!.value, "the Box is still selected and its Note is back").toBe("keep me");
  editor.press("Escape");
  editor.press("Escape");
  await settle();
  expect(editor.calls.filter((call) => call === "discard")).toHaveLength(1);
});

it("a click elsewhere on the canvas saves the Note and closes the field", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  editor.typeNote("saved by a click");
  editor.mouse("pointerdown", 600, 500);
  editor.mouse("pointerup", 600, 500);
  expect(editor.card()).toBeNull();

  editor.mouse("dblclick", 300, 230);
  expect(editor.field()!.value).toBe("saved by a click");
});

it("Ctrl+C with a Note field open closes it and copies the PNG", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  editor.typeNote("copy me");
  editor.press("c", true);
  await settle();
  expect(editor.card()).toBeNull();
  expect(editor.copied).toHaveLength(1);
  expect(editor.copied[0].width).toBe(400);
});

it("--snap while a Note field is open saves it and copies", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  editor.typeNote("dictated");
  editor.requestCopy();
  await settle();
  expect(editor.card()).toBeNull();
  expect(editor.copied).toHaveLength(1);
});

it("deletes the selected Box with Delete and the hint follows", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  editor.press("Enter");
  editor.dragBox([250, 300], [350, 340]);
  editor.press("Enter");
  expect(editor.page.hint.textContent).toMatch(/^2 notes/);

  editor.mouse("pointerdown", 300, 320);
  editor.mouse("pointerup", 300, 320);
  editor.press("Delete");
  expect(editor.page.hint.textContent).toMatch(/^1 note ·/);
});

it("starts the next Snap with no Boxes", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  editor.press("Enter");
  editor.press("Escape");
  await settle();
  editor.deliver(400, 300, BIG);
  await settle();
  expect(editor.page.hint.textContent).toMatch(/^0 notes/);
});

it("clears the Region from the canvas when the Editor closes, so nothing stale is left to copy", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  editor.press("Enter");
  expect(editor.page.canvas.width, "the Region is on the canvas").toBe(800);

  editor.close();
  expect(editor.page.canvas.width).toBe(0);
  expect(editor.page.canvas.height).toBe(0);

  editor.press("c", true);
  await settle();
  expect(editor.copied, "there is no Snap to copy").toHaveLength(0);
});

it("keeps the Note card's width in step with a window resize", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  expect(editor.card()!.style.width, "the Region at 100%, its Callout cap as it shows").toBe("384px");

  editor.resize(200, 150);
  expect(editor.card()!.style.width, "the Region now fits at 50%").toBe("280px");
});

it("keeps the Note card's width in step with a move to a monitor at another scale", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  expect(editor.card()!.style.width).toBe("384px");

  editor.rescale(2);
  expect(editor.card()!.style.width, "the same pixels at twice the scale are half as wide on screen").toBe("280px");
});

it("fits the Note field's text into the card again when the card narrows", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  // jsdom lays nothing out: the textarea reports the height its text needs at the card's width, as a browser
  // would (16 px a character, 20 px a line).
  vi.spyOn(HTMLTextAreaElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLTextAreaElement) {
    const width = parseInt((this.closest(".note-card") as HTMLElement).style.width, 10);
    return Math.ceil((this.value.length * 16) / width) * 20;
  });
  // 71 characters: 3 lines in a 384 px card, 5 in the 280 px one it becomes at 50%.
  editor.typeNote("one two three four five six seven eight nine ten eleven twelve thirteen");
  expect(editor.field()!.style.height, "at 384 px").toBe("60px");

  editor.resize(200, 150);
  expect(editor.field()!.style.height, "the narrower card wraps the text onto more lines").toBe("100px");
});

it("does nothing on Ctrl+C in the middle of a drag", async () => {
  const editor = await editorWithRegion();
  editor.mouse("pointerdown", 250, 200);
  editor.mouse("pointermove", 350, 260);
  editor.press("c", true);
  await settle();
  expect(editor.copied, "the drag is not exported halfway").toHaveLength(0);

  editor.mouse("pointerup", 350, 260);
  expect(editor.card(), "the Box is drawn and its Note field opens").not.toBeNull();
});

it("ignores the pointer while a copy is in flight", async () => {
  const editor = await editorWithRegion();
  editor.dragBox([250, 200], [350, 260]);
  editor.press("Enter");
  editor.holdCopy();
  editor.press("c", true);
  await settle();
  expect(editor.calls).toContain("copy");

  editor.mouse("pointermove", 300, 230);
  expect(editor.page.canvas.style.cursor, "the pointer is ignored until the copy is over").toBe("");

  editor.releaseCopy();
  await settle();
});
