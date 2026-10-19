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

/** The Note field's width in CSS pixels: the Callout's width cap as it shows on screen, within 280 to 440. */
export function noteCardWidth(capImagePx: number, cssPerImage: number): number {
  return Math.min(440, Math.max(280, capImagePx * cssPerImage));
}

const CARD_GAP = 10;
const CARD_EDGE = 8;

/**
 * Where the Note field's card goes, in CSS pixels in the stage: under the Box, or above it (clearing its
 * badge) when it wouldn't fit below, and clamped inside the window either way (spec 7.1).
 */
export function placeNoteCard(
  box: { x: number; y: number; w: number; h: number },
  badgeRadius: number,
  card: Size,
  stage: Size,
): { left: number; top: number } {
  let top = box.y + box.h + CARD_GAP;
  if (top + card.height > stage.height - CARD_EDGE) top = box.y - badgeRadius - CARD_GAP - card.height;
  return {
    left: Math.max(CARD_EDGE, Math.min(box.x, stage.width - card.width - CARD_EDGE)),
    top: Math.max(CARD_EDGE, Math.min(top, stage.height - card.height - CARD_EDGE)),
  };
}
