import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { encodeDelta } from "./delta";

/** The Editor page's side of the IPC with Rust (`src-tauri/src/commands.rs`). */
export interface EditorIpc {
  /** Parked until the next Region arrives: raw RGBA, then width and height as little-endian u32. */
  nextSnap(): Promise<ArrayBuffer>;
  /** A frame with the Region on it has been presented: Rust may show the window. */
  painted(): Promise<void>;
  /**
   * The export goes to the clipboard, then Rust closes the Editor. Only what differs from `original`, the
   * Region as Rust sent it, crosses over: the Boxes and Callouts, not the 33 MB of a 4K Region.
   */
  copy(rgba: Uint8ClampedArray, width: number, height: number, original?: Uint8ClampedArray): Promise<void>;
  /** Close the Editor with nothing copied. */
  discard(): Promise<void>;
  /** `--snap` arrived while the Editor is open. */
  onCopyRequested(handler: () => void): void;
  /** A timing mark for the joint test session (ANNOTA-20): only in a build made with `VITE_TIMINGS=1`. */
  mark?(name: string): void;
}

const mark = import.meta.env.VITE_TIMINGS
  ? (name: string) => {
      void invoke("timing_mark", { name });
    }
  : undefined;

function copyRaw(rgba: Uint8ClampedArray, width: number, height: number): Promise<void> {
  return invoke("copy_snap", new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.byteLength), {
    headers: { width: String(width), height: String(height) },
  });
}

/**
 * Sends only the tiles that differ from `original`, which Rust kept (spec §9). If that fails Rust has spent
 * its copy, so the retry is the whole export as the raw body.
 */
async function copyRgba(rgba: Uint8ClampedArray, width: number, height: number, original?: Uint8ClampedArray): Promise<void> {
  if (original) {
    try {
      const payload = encodeDelta(original, rgba, width, height);
      mark?.("page_packed");
      return await invoke("copy_delta", payload);
    } catch (error) {
      console.error("copy by changed tiles failed, sending the whole export", error);
    }
  }
  return copyRaw(rgba, width, height);
}

export const tauriIpc: EditorIpc = {
  nextSnap: () => invoke<ArrayBuffer>("next_snap"),
  painted: () => invoke("editor_painted"),
  copy: copyRgba,
  discard: () => invoke("discard_snap"),
  onCopyRequested: (handler) => {
    void listen("copy-requested", handler);
  },
  // Dead code in a normal build: Vite folds the constant away and Rust has no `timing_mark` command.
  mark,
};
