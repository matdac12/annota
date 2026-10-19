/** Everything the Boxes (and later the Callouts) size from the capture, in image pixels (spec 8.1). */
export interface Sizing {
  /** The long edge, `L`. */
  longEdge: number;
  /** Callout text size, `F`. */
  F: number;
  /** Box stroke width, `S`. */
  S: number;
  /** Badge diameter, `D`. */
  D: number;
  /** Space kept inside the image edge. */
  margin: number;
  /** Width cap of a Callout, which the Note field's width follows. */
  cap: number;
}

/**
 * Sizes that grow with the capture, so they survive an agent shrinking the image to 1,568 px on the long edge:
 * `F = max(16, round(16 × L / 1568))`, `S = max(2, round(F / 7))`, `D = round(1.45 × F)`.
 */
export function sizingFor(width: number, height: number): Sizing {
  const longEdge = Math.max(width, height);
  const F = Math.max(16, Math.round((16 * longEdge) / 1568));
  const margin = Math.round(F * 0.5);
  return {
    longEdge,
    F,
    S: Math.max(2, Math.round(F / 7)),
    D: Math.round(F * 1.45),
    margin,
    cap: Math.min(width - 2 * margin, 30 * F),
  };
}
