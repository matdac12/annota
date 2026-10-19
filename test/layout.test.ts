import { expect, it } from "vitest";
import { deviceSize, noteCardWidth, placeImage, placeNoteCard } from "../src/layout";

it("puts an image that fits at 100% in whole device pixels, centred", () => {
  expect(placeImage({ width: 1000, height: 700 }, { width: 800, height: 600 })).toEqual({ scale: 1, x: 100, y: 50 });
  // An odd leftover rounds down, so the image still lands on a pixel boundary.
  expect(placeImage({ width: 801, height: 601 }, { width: 800, height: 600 })).toEqual({ scale: 1, x: 0, y: 0 });
});

it("never scales an image up", () => {
  expect(placeImage({ width: 4000, height: 3000 }, { width: 100, height: 50 }).scale).toBe(1);
});

it("scales down by the tighter side when the canvas is too small", () => {
  expect(placeImage({ width: 1000, height: 1000 }, { width: 2000, height: 1000 })).toEqual({ scale: 0.5, x: 0, y: 250 });
  expect(placeImage({ width: 1000, height: 500 }, { width: 1000, height: 1000 })).toEqual({ scale: 0.5, x: 250, y: 0 });
});

it("takes the canvas size from device-pixel-content-box, so 150% scaling is exact", () => {
  const entry = {
    devicePixelContentBoxSize: [{ inlineSize: 1203, blockSize: 802 }],
    contentRect: { width: 802, height: 534.67 },
  };
  expect(deviceSize(entry, 1.5)).toEqual({ width: 1203, height: 802 });
});

it("falls back to the CSS box times devicePixelRatio when the WebView doesn't report device pixels", () => {
  expect(deviceSize({ contentRect: { width: 800, height: 600.4 } }, 1.5)).toEqual({ width: 1200, height: 901 });
});

it("sizes the Note field from the Callout's width cap on screen, between 280 and 440 CSS px", () => {
  expect(noteCardWidth(600, 1)).toBe(440);
  expect(noteCardWidth(600, 0.5)).toBe(300);
  expect(noteCardWidth(480, 0.4)).toBe(280);
});

it("opens the Note field under the Box, above it when there is no room, and inside the window", () => {
  const stage = { width: 800, height: 600 };
  const card = { width: 300, height: 120 };
  expect(placeNoteCard({ x: 100, y: 100, w: 200, h: 100 }, 11, card, stage)).toEqual({ left: 100, top: 210 });
  expect(placeNoteCard({ x: 100, y: 400, w: 200, h: 150 }, 11, card, stage)).toEqual({ left: 100, top: 259 });
  expect(placeNoteCard({ x: 700, y: 100, w: 50, h: 50 }, 11, card, stage).left).toBe(492);
  expect(placeNoteCard({ x: -20, y: 100, w: 50, h: 50 }, 11, card, stage).left).toBe(8);
});

it("keeps the card inside the window when there is room neither below nor above", () => {
  expect(placeNoteCard({ x: 10, y: 20, w: 300, h: 330 }, 11, { width: 300, height: 120 }, { width: 800, height: 400 })).toEqual({
    left: 10,
    top: 8,
  });
});
