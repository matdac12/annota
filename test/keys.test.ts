import { expect, it } from "vitest";
import { actionForKey, type KeyContext } from "../src/keys";

const IDLE: KeyContext = { noteOpen: false, textSelected: false, boxSelected: false };
const NOTE: KeyContext = { noteOpen: true, textSelected: false, boxSelected: true };

const press = (
  key: string,
  context: KeyContext = IDLE,
  mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "isComposing", boolean>> = {},
) => actionForKey({ key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods }, context);

it("copies on Ctrl+C and discards on Esc when no Note field is open", () => {
  expect(press("c", IDLE, { ctrlKey: true })).toBe("copy");
  expect(press("C", IDLE, { ctrlKey: true })).toBe("copy");
  expect(press("Escape")).toBe("discard");
});

it("leaves every other key alone", () => {
  expect(press("c")).toBeNull();
  expect(press("c", IDLE, { ctrlKey: true, shiftKey: true })).toBeNull();
  expect(press("c", IDLE, { ctrlKey: true, altKey: true })).toBeNull();
  expect(press("v", IDLE, { ctrlKey: true })).toBeNull();
  expect(press("Enter")).toBeNull();
  expect(press("Delete")).toBeNull();
});

it("deletes or edits the selected Box, and only a selected one", () => {
  const selected = { ...IDLE, boxSelected: true };
  expect(press("Delete", selected)).toBe("delete");
  expect(press("Backspace", selected)).toBe("delete");
  expect(press("Enter", selected)).toBe("edit");
  expect(press("Backspace", IDLE)).toBeNull();
});

it("Enter and Esc close an open Note field, keeping the text; Esc does not discard the Snap", () => {
  expect(press("Enter", NOTE)).toBe("close-note");
  expect(press("Escape", NOTE)).toBe("close-note");
});

it("Shift+Enter adds a new line, and typing (Backspace, Delete) stays the Note field's own", () => {
  expect(press("Enter", NOTE, { shiftKey: true })).toBeNull();
  expect(press("Backspace", NOTE)).toBeNull();
  expect(press("Delete", NOTE)).toBeNull();
});

it("does not take the Enter that confirms an input method's composition", () => {
  expect(press("Enter", NOTE, { isComposing: true })).toBeNull();
});

it("Ctrl+C in a Note field copies selected text, and otherwise copies the PNG", () => {
  expect(press("c", { ...NOTE, textSelected: true }, { ctrlKey: true })).toBeNull();
  expect(press("c", NOTE, { ctrlKey: true })).toBe("copy");
});

it("opens the selected Box's Note on a plain Enter only", () => {
  const selected = { ...IDLE, boxSelected: true };
  expect(press("Enter", selected, { shiftKey: true })).toBeNull();
  expect(press("Enter", selected, { ctrlKey: true })).toBeNull();
});
