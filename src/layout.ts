/** A size in device pixels: the canvas's backing store, so one canvas pixel is one screen pixel. */
export interface Size {
  width: number;
  height: number;
}

/** Where the image goes on the canvas, in device pixels: `scale` 1 is one image pixel per screen pixel. */
export interface Placed {
  scale: number;
  x: number;
  y: number;
}

/**
 * The image at 100%, centred, and scaled down only when the canvas is too small for it (spec 6.2). Never
 * scaled up. The offset is whole device pixels, so a 100% image lands pixel-for-pixel.
 */
export function placeImage(canvas: Size, image: Size): Placed {
  const scale = Math.min(1, canvas.width / image.width, canvas.height / image.height);
  return {
    scale,
    x: Math.floor((canvas.width - image.width * scale) / 2),
    y: Math.floor((canvas.height - image.height * scale) / 2),
  };
}

/** The slice of a `ResizeObserverEntry` the layout reads. */
export interface ObservedBox {
  devicePixelContentBoxSize?: ReadonlyArray<{ inlineSize: number; blockSize: number }>;
  contentRect: { width: number; height: number };
}

/**
 * The element's content box in device pixels: `device-pixel-content-box` where the WebView reports it,
 * else the CSS box times `devicePixelRatio`.
 */
export function deviceSize(entry: ObservedBox, devicePixelRatio: number): Size {
  const exact = entry.devicePixelContentBoxSize?.[0];
  if (exact) return { width: exact.inlineSize, height: exact.blockSize };
  return {
    width: Math.round(entry.contentRect.width * devicePixelRatio),
    height: Math.round(entry.contentRect.height * devicePixelRatio),
  };
}
