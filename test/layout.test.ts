import { expect, it } from "vitest";
import { deviceSize, placeImage } from "../src/layout";

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
