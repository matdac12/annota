import { expect, it } from "vitest";
import { renderSnap } from "../src/render";
import type { Snap } from "../src/snap";

/** A canvas context that remembers the colours it was set to and the text it was asked to draw. */
function recorder() {
  const colours: string[] = [];
  const texts: string[] = [];
  const ctx = new Proxy(
    {},
    {
      get: (_, key) => (key === "fillText" ? (text: string) => texts.push(text) : () => {}),
      set: (_, key, value) => {
        if (key === "strokeStyle" || key === "fillStyle") colours.push(String(value));
        return true;
      },
    },
  ) as unknown as CanvasRenderingContext2D;
  return { ctx, colours, texts };
}

const snap = (): Snap => ({
  width: 400,
  height: 300,
  image: {} as CanvasImageSource,
  boxes: [
    { x: 10, y: 10, w: 100, h: 60, note: "" },
    { x: 150, y: 100, w: 80, h: 80, note: "second" },
  ],
});

it("numbers the Boxes in order", () => {
  const { ctx, texts } = recorder();
  renderSnap(ctx, snap(), { forExport: true });
  expect(texts).toEqual(["1", "2"]);
});

it("keeps the selection outline and handles out of the export", () => {
  const exported = recorder();
  renderSnap(exported.ctx, snap(), { forExport: true, selected: 0 });
  expect(exported.colours).not.toContain("#0a84ff");

  const editing = recorder();
  renderSnap(editing.ctx, snap(), { forExport: false, selected: 0 });
  expect(editing.colours).toContain("#0a84ff");
});

it("draws the Box being dragged out in the Editor only, numbered next", () => {
  const preview = { x: 200, y: 200, w: 50, h: 50 };
  const exported = recorder();
  renderSnap(exported.ctx, snap(), { forExport: true, preview });
  expect(exported.texts).toEqual(["1", "2"]);

  const editing = recorder();
  renderSnap(editing.ctx, snap(), { forExport: false, preview });
  expect(editing.texts).toEqual(["1", "2", "3"]);
});
