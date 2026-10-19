import { expect, it } from "vitest";
import { actionForKey } from "../src/keys";

const press = (key: string, mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) =>
  actionForKey({ key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });

it("copies on Ctrl+C and discards on Esc", () => {
  expect(press("c", { ctrlKey: true })).toBe("copy");
  expect(press("C", { ctrlKey: true })).toBe("copy");
  expect(press("Escape")).toBe("discard");
});

it("leaves every other key alone", () => {
  expect(press("c")).toBeNull();
  expect(press("c", { ctrlKey: true, shiftKey: true })).toBeNull();
  expect(press("c", { ctrlKey: true, altKey: true })).toBeNull();
  expect(press("v", { ctrlKey: true })).toBeNull();
  expect(press("Enter")).toBeNull();
});
