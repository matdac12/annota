/** The side of the squares the export is compared in. */
export const TILE = 64;

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * What differs between the Region as Rust sent it (`before`) and the export (`after`), as the payload of
 * `copy_delta` (ANNOTA-23): a little-endian u32 count, then `x y w h` as u32s for each rectangle, then each
 * rectangle's RGBA rows. Rust patches the Region it kept (`transfer::apply_rects`).
 *
 * Both are top-down RGBA, `width`×`height`. The rectangles are whole `TILE` squares that hold at least one
 * changed byte (the last ones clipped to the image), joined along a row and then down a column when they
 * line up, so the Boxes and Callouts cost their own area and the rest of a 4K Region costs nothing.
 */
export function encodeDelta(before: Uint8ClampedArray, after: Uint8ClampedArray, width: number, height: number): Uint8Array {
  const rects = changedRects(new Uint32Array(before.buffer, before.byteOffset, width * height), new Uint32Array(after.buffer, after.byteOffset, width * height), width, height);
  let bytes = 4 + rects.length * 16;
  for (const r of rects) bytes += r.w * r.h * 4;
  const out = new Uint8Array(bytes);
  const words = new DataView(out.buffer);
  words.setUint32(0, rects.length, true);
  let pixels = 4 + rects.length * 16;
  rects.forEach((r, i) => {
    [r.x, r.y, r.w, r.h].forEach((value, k) => words.setUint32(4 + i * 16 + k * 4, value, true));
    for (let row = 0; row < r.h; row++) {
      const from = ((r.y + row) * width + r.x) * 4;
      out.set(after.subarray(from, from + r.w * 4), pixels);
      pixels += r.w * 4;
    }
  });
  return out;
}

function changedRects(before: Uint32Array, after: Uint32Array, width: number, height: number): Rect[] {
  const columns = Math.ceil(width / TILE);
  const rects: Rect[] = [];
  /** Rectangles still open at the last tile row, by `x:w`: a run directly below one of the same width extends it. */
  let open = new Map<string, Rect>();
  for (let ty = 0, y0 = 0; y0 < height; ty++, y0 += TILE) {
    const y1 = Math.min(y0 + TILE, height);
    const changed = new Uint8Array(columns);
    for (let y = y0; y < y1; y++) {
      const end = (y + 1) * width;
      for (let i = y * width; i < end; i++) {
        if (before[i] === after[i]) continue;
        const tile = ((i - y * width) / TILE) | 0;
        changed[tile] = 1;
        i = y * width + (tile + 1) * TILE - 1; // the rest of this tile's row can't add anything
      }
    }
    const next = new Map<string, Rect>();
    for (let tx = 0; tx < columns; tx++) {
      if (!changed[tx]) continue;
      let last = tx;
      while (last + 1 < columns && changed[last + 1]) last++;
      const x = tx * TILE;
      const w = Math.min((last + 1) * TILE, width) - x;
      const key = `${x}:${w}`;
      const above = open.get(key);
      if (above) {
        above.h += y1 - y0;
        next.set(key, above);
      } else {
        const rect = { x, y: y0, w, h: y1 - y0 };
        rects.push(rect);
        next.set(key, rect);
      }
      tx = last;
    }
    open = next;
  }
  return rects;
}
