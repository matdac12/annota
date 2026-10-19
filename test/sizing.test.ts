import { expect, it } from "vitest";
import { sizingFor } from "../src/sizing";

// Spec 8.1: F = max(16, round(16 × L / 1568)), stroke max(2, round(F/7)), badge round(1.45F).
it.each([
  { width: 640, height: 360, F: 16, S: 2, D: 23, margin: 8, cap: 480 },
  { width: 1920, height: 1200, F: 20, S: 3, D: 29, margin: 10, cap: 600 },
  { width: 2560, height: 1440, F: 26, S: 4, D: 38, margin: 13, cap: 780 },
  { width: 3840, height: 2160, F: 39, S: 6, D: 57, margin: 20, cap: 1170 },
])("sizes a $width×$height capture", ({ width, height, ...expected }) => {
  expect(sizingFor(width, height)).toMatchObject(expected);
});

it("scales from the long edge, so a tall capture sizes like a wide one", () => {
  expect(sizingFor(1200, 1920).F).toBe(sizingFor(1920, 1200).F);
});

it("never lets the width cap pass the image's own margins", () => {
  expect(sizingFor(100, 100).cap).toBe(100 - 2 * 8);
});
