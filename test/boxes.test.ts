import { expect, it } from "vitest";
import { badgeFor, hitTest, moveRect, normalize, resizeRect, type Box } from "../src/boxes";
import { sizingFor } from "../src/sizing";

const BOUNDS = { width: 400, height: 300 };
const box = (x: number, y: number, w: number, h: number): Box => ({ x, y, w, h, note: "" });

it("normalises a rectangle dragged up and to the left", () => {
  expect(normalize({ x: 50, y: 60, w: -30, h: -20 })).toEqual({ x: 20, y: 40, w: 30, h: 20 });
});

it("moves a rectangle but never out of the image", () => {
  const rect = { x: 100, y: 100, w: 50, h: 40 };
  expect(moveRect(rect, 20, -10, BOUNDS)).toEqual({ x: 120, y: 90, w: 50, h: 40 });
  expect(moveRect(rect, -500, -500, BOUNDS)).toEqual({ x: 0, y: 0, w: 50, h: 40 });
  expect(moveRect(rect, 500, 500, BOUNDS)).toEqual({ x: 350, y: 260, w: 50, h: 40 });
});

it("resizes from a corner while the opposite corner stays put", () => {
  const rect = { x: 100, y: 100, w: 50, h: 40 };
  expect(resizeRect(rect, "se", 10, 5, BOUNDS)).toEqual({ x: 100, y: 100, w: 60, h: 45 });
  expect(resizeRect(rect, "nw", 10, 5, BOUNDS)).toEqual({ x: 110, y: 105, w: 40, h: 35 });
});

it("normalises a resize dragged across its opposite corner", () => {
  const rect = { x: 100, y: 100, w: 50, h: 40 };
  expect(resizeRect(rect, "se", -70, -50, BOUNDS)).toEqual({ x: 80, y: 90, w: 20, h: 10 });
  expect(resizeRect(rect, "nw", 70, 50, BOUNDS)).toEqual({ x: 150, y: 140, w: 20, h: 10 });
});

it("keeps a resized corner inside the image", () => {
  expect(resizeRect({ x: 350, y: 250, w: 20, h: 20 }, "se", 100, 100, BOUNDS)).toEqual({ x: 350, y: 250, w: 50, h: 50 });
  expect(resizeRect({ x: 10, y: 10, w: 20, h: 20 }, "nw", -100, -100, BOUNDS)).toEqual({ x: 0, y: 0, w: 30, h: 30 });
});

it("hits a corner handle of the selected Box before anything else", () => {
  const boxes = [box(100, 100, 100, 100), box(150, 150, 100, 100)];
  expect(hitTest(boxes, 0, { x: 200, y: 200 }, 6, 6)).toEqual({ type: "handle", index: 0, corner: "se" });
  expect(hitTest(boxes, 0, { x: 100, y: 100 }, 6, 6)).toEqual({ type: "handle", index: 0, corner: "nw" });
});

it("ignores the handles of a Box that is not selected", () => {
  const boxes = [box(100, 100, 100, 100)];
  expect(hitTest(boxes, -1, { x: 100, y: 100 }, 6, 6)).toEqual({ type: "box", index: 0 });
});

it("hits the topmost Box where Boxes overlap, and the tolerance around an edge", () => {
  const boxes = [box(100, 100, 100, 100), box(150, 150, 100, 100)];
  expect(hitTest(boxes, -1, { x: 170, y: 170 }, 6, 6)).toEqual({ type: "box", index: 1 });
  expect(hitTest(boxes, -1, { x: 120, y: 120 }, 6, 6)).toEqual({ type: "box", index: 0 });
  expect(hitTest(boxes, -1, { x: 95, y: 150 }, 6, 6)).toEqual({ type: "box", index: 0 });
  expect(hitTest(boxes, -1, { x: 90, y: 150 }, 6, 6)).toEqual({ type: "none" });
});

it("lets the whole badge hit its Box, and no more than the badge", () => {
  const sizing = sizingFor(400, 300); // badge 23 px across: 11.5 px from the corner
  const boxes = [box(100, 100, 50, 50)];
  const tolerance = 6;
  const handleTolerance = tolerance + sizing.S;
  // 11 px up and left of the corner: on the badge, and beyond the Box's tolerance.
  expect(hitTest(boxes, -1, { x: 89, y: 100 }, tolerance, handleTolerance)).toEqual({ type: "box", index: 0 });
  expect(hitTest(boxes, -1, { x: 87, y: 100 }, tolerance, handleTolerance)).toEqual({ type: "none" });
});

it("hits nothing on empty canvas", () => {
  expect(hitTest([], -1, { x: 5, y: 5 }, 6, 6)).toEqual({ type: "none" });
});

it("centres the badge on the top-left corner and clamps it inside the image", () => {
  const sizing = sizingFor(400, 300);
  const r = sizing.D / 2;
  expect(badgeFor(box(100, 80, 50, 50), sizing, BOUNDS)).toEqual({ cx: 100, cy: 80, r });
  expect(badgeFor(box(0, 0, 50, 50), sizing, BOUNDS)).toEqual({ cx: r, cy: r, r });
});

it("lets a Callout over empty canvas take the click, and picks the topmost Callout", () => {
  const boxes = [box(10, 10, 20, 20), box(300, 10, 20, 20)];
  const callouts = [
    { x: 400, y: 400, w: 80, h: 30 },
    { x: 420, y: 410, w: 80, h: 30 },
  ];
  expect(hitTest(boxes, -1, { x: 430, y: 415 }, 6, 6, callouts)).toEqual({ type: "callout", index: 1 });
  expect(hitTest(boxes, -1, { x: 405, y: 405 }, 6, 6, callouts)).toEqual({ type: "callout", index: 0 });
});

it("gives a Box's edge the click where a Callout overlaps it", () => {
  const boxes = [box(300, 20, 50, 50), box(105, 160, 200, 100)];
  const callouts = [null, { x: 95, y: 150, w: 100, h: 60 }];
  expect(hitTest(boxes, -1, { x: 160, y: 160 }, 6, 6, callouts)).toEqual({ type: "box", index: 1 });
});

it("gives a Box's badge the click where a Callout overlaps it", () => {
  const boxes = [box(300, 20, 50, 50), box(105, 160, 200, 100)];
  const callouts = [null, { x: 95, y: 150, w: 100, h: 60 }];
  const tolerance = 6;
  const handleTolerance = tolerance + sizingFor(400, 300).S;
  expect(hitTest(boxes, -1, { x: 100, y: 152 }, tolerance, handleTolerance, callouts)).toEqual({ type: "box", index: 1 });
});

it("lets a Callout over a Box's interior take the click, so a Box inside another keeps its Note", () => {
  const boxes = [box(50, 50, 400, 300), box(100, 100, 40, 40)];
  const callouts = [null, { x: 100, y: 150, w: 80, h: 30 }];
  expect(hitTest(boxes, -1, { x: 110, y: 160 }, 6, 6, callouts)).toEqual({ type: "callout", index: 1 });
});
