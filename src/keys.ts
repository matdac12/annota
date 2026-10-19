export type EditorKey = "copy" | "discard";

type Press = Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

/** What a key press does in the Editor (spec 10, the rows without Boxes). */
export function actionForKey(press: Press): EditorKey | null {
  if (press.key === "Escape") return "discard";
  const plainCtrl = press.ctrlKey && !press.altKey && !press.shiftKey && !press.metaKey;
  if (plainCtrl && press.key.toLowerCase() === "c") return "copy";
  return null;
}
