import { readFileSync } from "node:fs";
import { beforeEach, expect, it } from "vitest";
import { mountEditorPage } from "../src/editor-page";

// The Editor's page as Tauri loads it: the real index.html, not a stand-in.
beforeEach(() => {
  document.documentElement.innerHTML = readFileSync("index.html", "utf8");
});

it("finds the Editor's one canvas in the page", () => {
  const canvas = mountEditorPage(document);
  expect(canvas).toBeInstanceOf(HTMLCanvasElement);
  expect(document.querySelectorAll("canvas")).toHaveLength(1);
});

it("names the missing canvas instead of failing later", () => {
  document.body.innerHTML = "";
  expect(() => mountEditorPage(document)).toThrow(/canvas/);
});
