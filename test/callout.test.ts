import { describe, expect, it } from "vitest";
import { calloutLayout, calloutLayouts, wrapText, type Measure } from "../src/callout";
import { sizingFor } from "../src/sizing";

/** Every character is 10 px wide, bold or not, so widths read directly. */
const tenPx: Measure = (text) => text.length * 10;

describe("wrapText", () => {
  it("breaks at spaces and keeps a line within the width", () => {
    expect(wrapText(tenPx, "aaa bbb ccc", 70)).toEqual(["aaa bbb", "ccc"]);
  });

  it("fits a line of exactly the width", () => {
    expect(wrapText(tenPx, "aaa bbb", 70)).toEqual(["aaa bbb"]);
  });

  it("breaks a word longer than the width by characters", () => {
    expect(wrapText(tenPx, "abcdefghij", 40)).toEqual(["abcd", "efgh", "ij"]);
  });

  it("starts a long word on its own line, then continues after it", () => {
    expect(wrapText(tenPx, "ab abcdefghij c", 40)).toEqual(["ab", "abcd", "efgh", "ij c"]);
  });

  it("keeps explicit newlines, including blank lines", () => {
    expect(wrapText(tenPx, "one\n\ntwo\r\nthree", 200)).toEqual(["one", "", "two", "three"]);
  });

  it("ignores newlines at the end", () => {
    expect(wrapText(tenPx, "one\n\n", 200)).toEqual(["one"]);
  });

  it("always makes progress, even when one character is wider than the width", () => {
    expect(wrapText(tenPx, "abc", 5)).toEqual(["a", "b", "c"]);
  });

  it("does not split a character outside the basic plane", () => {
    expect(wrapText(tenPx, "😀😀", 10)).toEqual(["😀", "😀"]);
  });
});

/** A box on a 1000×800 Snap: F = 16, S = 2, D = 23, margin 8, gap 6. */
const sizing = sizingFor(1000, 800);
const image = { width: 1000, height: 800 };
const place = (box: { x: number; y: number; w: number; h: number }, note = "note") =>
  calloutLayout(tenPx, { ...box, note }, 1, sizing, image)!;

describe("calloutLayout", () => {
  it("sits under its Box, left-aligned, 0.4F below the stroke", () => {
    const layout = place({ x: 100, y: 100, w: 200, h: 100 });
    expect(layout.side).toBe("below");
    expect(layout.x).toBe(100);
    expect(layout.y).toBe(100 + 100 + sizing.S / 2 + Math.round(0.4 * sizing.F));
  });

  it("flips above the Box near the bottom edge, clearing the badge by D/2", () => {
    const layout = place({ x: 100, y: 600, w: 200, h: 180 });
    expect(layout.side).toBe("above");
    expect(layout.y + layout.h).toBe(600 - sizing.S / 2 - Math.round(0.4 * sizing.F) - sizing.D / 2);
  });

  it("clamps inside the bottom margin when it fits neither below nor above", () => {
    const layout = place({ x: 100, y: 10, w: 200, h: 780 });
    expect(layout.side).toBe("inside");
    expect(layout.y + layout.h).toBe(800 - sizing.margin);
  });

  it("clamps near the right edge, inside the margin", () => {
    const layout = place({ x: 990, y: 100, w: 5, h: 50 });
    expect(layout.x + layout.w).toBe(1000 - sizing.margin);
  });

  it("clamps near the left edge, inside the margin", () => {
    expect(place({ x: 0, y: 100, w: 50, h: 50 }).x).toBe(sizing.margin);
  });

  it("has no Callout for an empty or blank Note", () => {
    expect(calloutLayout(tenPx, { x: 1, y: 1, w: 10, h: 10, note: "" }, 1, sizing, image)).toBeNull();
    expect(calloutLayout(tenPx, { x: 1, y: 1, w: 10, h: 10, note: " \n " }, 1, sizing, image)).toBeNull();
  });

  it("measures its size from the padding, the number, and a line height of round(1.3F)", () => {
    const layout = place({ x: 100, y: 100, w: 50, h: 50 }, "abc");
    const padX = Math.round(0.6 * sizing.F);
    const padY = Math.round(0.45 * sizing.F);
    const lead = 20 + 0.35 * sizing.F; // "1." at 10 px a character
    expect(layout.lead).toBeCloseTo(lead);
    expect(layout.w).toBe(Math.ceil(lead + 30 + 2 * padX));
    expect(layout.h).toBe(Math.round(1.3 * sizing.F) + 2 * padY);
  });

  it("wraps long Notes to the width cap and stays inside the image", () => {
    const layout = place({ x: 100, y: 100, w: 50, h: 50 }, "word ".repeat(200));
    expect(layout.w).toBeLessThanOrEqual(sizing.cap);
    expect(layout.lines.length).toBeGreaterThan(1);
  });
});

describe("sizing a Callout at the four widths", () => {
  const note = "The button is cut off and overlaps the label when the window is narrow";
  // Characters are 0.5 em wide, a fair stand-in for Segoe UI.
  const widths = [
    [640, 360],
    [1920, 1200],
    [2560, 1440],
    [3840, 2160],
  ] as const;

  it.each(widths)("%i wide: within the cap, and readable after the agent's shrink", (width, height) => {
    const s = sizingFor(width, height);
    const em: Measure = (text) => text.length * 0.5 * s.F;
    const layout = calloutLayout(em, { x: width / 4, y: height / 4, w: 100, h: 60, note }, 1, s, { width, height })!;
    expect(layout.w).toBeLessThanOrEqual(s.cap);
    expect(layout.x).toBeGreaterThanOrEqual(s.margin);
    expect(layout.x + layout.w).toBeLessThanOrEqual(width - s.margin);
    // Agents shrink to 1,568 px on the long edge; the text must stay about 16 px or more.
    expect(s.F * Math.min(1, 1568 / Math.max(width, height))).toBeGreaterThanOrEqual(15.9);
  });
});

describe("calloutLayouts", () => {
  const boxes = [
    { x: 10, y: 10, w: 50, h: 50, note: "first" },
    { x: 100, y: 10, w: 50, h: 50, note: "" },
    { x: 200, y: 10, w: 50, h: 50, note: "third" },
  ];

  it("numbers by Box order and skips empty Notes", () => {
    const layouts = calloutLayouts(tenPx, { ...image, boxes }, -1);
    expect(layouts.map((l) => l && l.number)).toEqual([1, null, 3]);
  });

  it("leaves out the Box whose Note field is open", () => {
    const layouts = calloutLayouts(tenPx, { ...image, boxes }, 0);
    expect(layouts[0]).toBeNull();
    expect(layouts[2]).not.toBeNull();
  });
});
