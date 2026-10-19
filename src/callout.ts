import type { Box, Rect } from "./boxes";
import { sizingFor, type Sizing } from "./sizing";

export const FAMILY = '"Segoe UI", system-ui, sans-serif';

/** The width of `text` in pixels at the Callout's size `F`, in the bold number's font or the Note's. */
export type Measure = (text: string, bold: boolean) => number;

/** The Callout's fonts, at the Callout text size `F` (spec 8.3). */
export const numberFont = (F: number): string => `700 ${F}px ${FAMILY}`;
export const noteFont = (F: number): string => `500 ${F}px ${FAMILY}`;

/** A `Measure` backed by a canvas context. It sets the context's font. */
export function canvasMeasure(ctx: CanvasRenderingContext2D, F: number): Measure {
  return (text, bold) => {
    ctx.font = bold ? numberFont(F) : noteFont(F);
    return ctx.measureText(text).width;
  };
}

/** A Callout placed in the image, in image pixels. */
export interface CalloutLayout extends Rect {
  /** The Box's number. */
  number: number;
  /** The wrapped Note, one string per line. */
  lines: string[];
  /** Where the Note's text starts, from the Callout's padding: the width of the number and a gap. */
  lead: number;
  side: "below" | "above" | "inside";
}

/**
 * Splits a Note into lines no wider than `maxWidth`: at spaces, then by characters inside a word that is
 * wider than a line on its own. Explicit newlines are kept; newlines at the end are not.
 */
export function wrapText(measure: Measure, text: string, maxWidth: number): string[] {
  const fits = (candidate: string): boolean => measure(candidate, false) <= maxWidth;
  const lines: string[] = [];
  for (const paragraph of text.trimEnd().split(/\r?\n/)) {
    let line = "";
    for (const word of paragraph.split(" ")) {
      const joined = line ? `${line} ${word}` : word;
      if (fits(joined)) {
        line = joined;
        continue;
      }
      if (line) lines.push(line);
      if (fits(word)) {
        line = word;
        continue;
      }
      line = "";
      for (const character of word) {
        if (line && !fits(line + character)) {
          lines.push(line);
          line = character;
        } else {
          line += character;
        }
      }
    }
    lines.push(line);
  }
  return lines;
}

const clamp = (value: number, low: number, high: number): number => Math.max(low, Math.min(high, value));

/**
 * The Callout of one Box (spec 8.3), or null when its Note is empty. Left-aligned with the Box and kept inside
 * the margin; under the Box at 0.4F below its stroke, else above it clearing the badge, else clamped inside the
 * bottom margin. Overlaps with other Callouts are allowed.
 */
export function calloutLayout(
  measure: Measure,
  box: Box,
  number: number,
  sizing: Sizing,
  image: { width: number; height: number },
): CalloutLayout | null {
  if (!box.note.trim()) return null;
  const { F, S, D, margin, cap } = sizing;
  const padX = Math.round(0.6 * F);
  const padY = Math.round(0.45 * F);
  const lineHeight = Math.round(1.3 * F);
  const gap = Math.round(0.4 * F);

  const lead = measure(`${number}.`, true) + 0.35 * F;
  const lines = wrapText(measure, box.note, cap - lead - 2 * padX);
  const textWidth = Math.max(F, ...lines.map((line) => measure(line, false)));
  const w = Math.ceil(lead + textWidth + 2 * padX);
  const h = lines.length * lineHeight + 2 * padY;

  const x = clamp(box.x, margin, Math.max(margin, image.width - w - margin));
  let y = box.y + box.h + S / 2 + gap;
  let side: CalloutLayout["side"] = "below";
  if (y + h > image.height - margin) {
    y = box.y - S / 2 - gap - D / 2 - h;
    side = "above";
    if (y < margin) {
      y = image.height - margin - h;
      side = "inside";
    }
  }
  return { x, y, w, h, number, lines, lead, side };
}

/**
 * The Callouts of a whole Snap, by Box: null for a Box without a Note and for the Box whose Note field is
 * open (`editing`), whose Callout is not drawn meanwhile.
 */
export function calloutLayouts(
  measure: Measure,
  snap: { width: number; height: number; boxes: readonly Box[] },
  editing: number,
): Array<CalloutLayout | null> {
  const sizing = sizingFor(snap.width, snap.height);
  return snap.boxes.map((box, index) =>
    index === editing ? null : calloutLayout(measure, box, index + 1, sizing, snap),
  );
}
