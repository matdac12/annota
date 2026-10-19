export type EditorKey = "copy" | "discard" | "delete" | "edit" | "close-note";

type Press = Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey"> & { isComposing?: boolean };

/** What the Editor is showing when a key is pressed. */
export interface KeyContext {
  /** A Note field is open (and has focus). */
  noteOpen: boolean;
  /** Text is selected in the open Note field. */
  textSelected: boolean;
  /** A Box is selected. */
  boxSelected: boolean;
}

/**
 * What a key press does in the Editor (spec 10), or null to leave it to the browser: Shift+Enter, typing and
 * Ctrl+C on selected text all stay the textarea's own.
 */
export function actionForKey(press: Press, context: KeyContext): EditorKey | null {
  if (press.isComposing) return null;
  const plainCtrl = press.ctrlKey && !press.altKey && !press.shiftKey && !press.metaKey;
  const copyKey = plainCtrl && press.key.toLowerCase() === "c";

  if (context.noteOpen) {
    // Enter saves and Esc keeps the text: the text is already kept as it is typed, so both just close the field.
    if (press.key === "Escape" || (press.key === "Enter" && !press.shiftKey && !press.ctrlKey && !press.altKey)) {
      return "close-note";
    }
    if (copyKey) return context.textSelected ? null : "copy";
    return null;
  }

  if (press.key === "Escape") return "discard";
  if (copyKey) return "copy";
  if (context.boxSelected) {
    if (press.key === "Delete" || press.key === "Backspace") return "delete";
    if (press.key === "Enter" && !press.shiftKey && !press.ctrlKey && !press.altKey) return "edit";
  }
  return null;
}
