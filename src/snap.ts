/** What the Editor holds for the current Snap: the Region's pixels, which Boxes and Callouts will join. */
export interface Snap {
  /** Image pixels, the unit all Box and Callout geometry is kept in. */
  width: number;
  height: number;
  /** The Region as Rust cropped it, drawn from here every time so the original is never touched. */
  image: CanvasImageSource;
}

/** Bytes after the pixels: width and height as little-endian u32. */
const DIMENSIONS_BYTES = 8;

/**
 * Reads the answer to `next_snap`: top-down RGBA (alpha 255), then width and height. The pixels are a view
 * on `bytes`, not a copy.
 */
export function parseSnapBytes(bytes: ArrayBuffer): { width: number; height: number; pixels: Uint8ClampedArray<ArrayBuffer> } {
  if (bytes.byteLength < DIMENSIONS_BYTES) {
    throw new Error(`a Snap is at least ${DIMENSIONS_BYTES} bytes, got ${bytes.byteLength}`);
  }
  const sizes = new DataView(bytes, bytes.byteLength - DIMENSIONS_BYTES);
  const width = sizes.getUint32(0, true);
  const height = sizes.getUint32(4, true);
  const length = width * height * 4;
  if (width === 0 || height === 0 || length !== bytes.byteLength - DIMENSIONS_BYTES) {
    throw new Error(`${bytes.byteLength} bytes is not a ${width}x${height} RGBA Snap`);
  }
  return { width, height, pixels: new Uint8ClampedArray(bytes, 0, length) };
}
