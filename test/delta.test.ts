import { expect, it } from "vitest";
import { encodeDelta, TILE } from "../src/delta";

/** An opaque image whose pixel `i` is `i` in every colour byte, so any patch shows where it came from. */
function ramp(width: number, height: number): Uint8ClampedArray {
  const rgba = new Uint8ClampedArray(width * height * 4);
  for (let i = 0; i < width * height; i++) rgba.set([i % 251, (i * 3) % 251, (i * 7) % 251, 255], i * 4);
  return rgba;
}

/** Independent reader of the wire format (`delta::apply_rects` in Rust is the real one). */
function apply(base: Uint8ClampedArray, width: number, payload: Uint8Array): Uint8ClampedArray {
  const out = base.slice();
  const words = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  const count = words.getUint32(0, true);
  let pixels = 4 + count * 16;
  for (let r = 0; r < count; r++) {
    const [x, y, w, h] = [4, 8, 12, 16].map((o) => words.getUint32(4 + r * 16 - 4 + o, true));
    for (let row = 0; row < h; row++) {
      out.set(payload.subarray(pixels, pixels + w * 4), ((y + row) * width + x) * 4);
      pixels += w * 4;
    }
  }
  expect(pixels).toBe(payload.length);
  return out;
}

it("sends nothing for an image that did not change", () => {
  const image = ramp(10, 10);
  expect(Array.from(encodeDelta(image, image.slice(), 10, 10))).toEqual([0, 0, 0, 0]);
});

it("writes the format Rust reads: a count, a table, then the pixels of each rectangle", () => {
  // The same bytes `delta::tests::the_payload_the_page_test_pins_applies` patches in: a 2×1 image is one
  // tile, so one changed pixel sends the whole 2×1 rectangle at (0, 0).
  const before = new Uint8ClampedArray([1, 2, 3, 255, 4, 5, 6, 255]);
  const after = new Uint8ClampedArray([1, 2, 3, 255, 40, 50, 60, 255]);
  expect(Array.from(encodeDelta(before, after, 2, 1))).toEqual([
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 1, 2, 3, 255, 40, 50, 60, 255,
  ]);
});

it("cuts the image into tiles and sends only those that changed", () => {
  const width = TILE * 3 + 5;
  const height = TILE * 2 + 5;
  const before = ramp(width, height);
  const after = before.slice();
  after[(TILE * width + TILE * 2) * 4] ^= 1; // one byte, in the tile at column 2, row 1
  const payload = encodeDelta(before, after, width, height);
  const words = new DataView(payload.buffer, payload.byteOffset);
  expect(words.getUint32(0, true)).toBe(1);
  expect([4, 8, 12, 16].map((o) => words.getUint32(o, true))).toEqual([TILE * 2, TILE, TILE, TILE]);
  expect(payload.length).toBe(4 + 16 + TILE * TILE * 4);
  expect(apply(before, width, payload)).toEqual(after);
});

it("merges neighbouring tiles into runs and runs of the same width into taller rectangles", () => {
  const width = TILE * 4;
  const height = TILE * 3;
  const before = ramp(width, height);
  const after = before.slice();
  // Change something in tiles (1, 0), (2, 0), (1, 1), (2, 1): a 2×2-tile block, and one in (0, 2).
  for (const [tx, ty] of [[1, 0], [2, 0], [1, 1], [2, 1], [0, 2]]) after[((ty * TILE) * width + tx * TILE) * 4] ^= 1;
  const payload = encodeDelta(before, after, width, height);
  const words = new DataView(payload.buffer, payload.byteOffset);
  expect(words.getUint32(0, true)).toBe(2);
  expect([4, 8, 12, 16].map((o) => words.getUint32(o, true))).toEqual([TILE, 0, TILE * 2, TILE * 2]);
  expect([20, 24, 28, 32].map((o) => words.getUint32(o, true))).toEqual([0, TILE * 2, TILE, TILE]);
  expect(apply(before, width, payload)).toEqual(after);
});

it("clips the last tiles to the image edge", () => {
  const before = ramp(70, 70);
  const after = before.slice();
  after[(69 * 70 + 69) * 4 + 1] ^= 1;
  const payload = encodeDelta(before, after, 70, 70);
  const words = new DataView(payload.buffer, payload.byteOffset);
  expect([4, 8, 12, 16].map((o) => words.getUint32(o, true))).toEqual([64, 64, 6, 6]);
  expect(apply(before, 70, payload)).toEqual(after);
});

it("reproduces any set of changes exactly", () => {
  const width = 150;
  const height = 130;
  const before = ramp(width, height);
  const after = before.slice();
  let seed = 12345;
  const next = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff);
  for (let n = 0; n < 40; n++) after[next() % after.length] ^= 1 + (next() % 255);
  for (let n = 0; n < 2000; n++) after[(next() % (60 * width)) * 4 + 2] = 99; // a band along the top
  expect(apply(before, width, encodeDelta(before, after, width, height))).toEqual(after);
});
