import { expect, it } from "vitest";
import { parseSnapBytes } from "../src/snap";

/** What Rust sends: RGBA, then width and height as little-endian u32 (`geometry::with_dimensions`). */
function wire(width: number, height: number, rgba: number[]): ArrayBuffer {
  const bytes = new Uint8Array(rgba.length + 8);
  bytes.set(rgba);
  new DataView(bytes.buffer).setUint32(rgba.length, width, true);
  new DataView(bytes.buffer).setUint32(rgba.length + 4, height, true);
  return bytes.buffer;
}

it("reads the dimensions from the end and leaves the pixels where they are", () => {
  const rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 9, 9, 9, 255, 1, 2, 3, 255, 4, 5, 6, 255];
  const bytes = wire(3, 2, rgba);
  const snap = parseSnapBytes(bytes);
  expect([snap.width, snap.height]).toEqual([3, 2]);
  expect(Array.from(snap.pixels)).toEqual(rgba);
  expect(snap.pixels.buffer).toBe(bytes);
});

it("refuses bytes that are not the Snap they claim to be", () => {
  expect(() => parseSnapBytes(new ArrayBuffer(4))).toThrow(/at least 8/);
  expect(() => parseSnapBytes(wire(2, 2, [0, 0, 0, 255]))).toThrow(/not a 2x2/);
  expect(() => parseSnapBytes(wire(0, 0, []))).toThrow(/not a 0x0/);
});
