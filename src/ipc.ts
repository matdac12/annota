import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** The Editor page's side of the IPC with Rust (`src-tauri/src/commands.rs`). */
export interface EditorIpc {
  /** Parked until the next Region arrives: raw RGBA, then width and height as little-endian u32. */
  nextSnap(): Promise<ArrayBuffer>;
  /** A frame with the Region on it has been presented: Rust may show the window. */
  painted(): Promise<void>;
  /** The exported RGBA goes to the clipboard, then Rust closes the Editor. */
  copy(rgba: Uint8ClampedArray, width: number, height: number): Promise<void>;
  /** Close the Editor with nothing copied. */
  discard(): Promise<void>;
  /** `--snap` arrived while the Editor is open. */
  onCopyRequested(handler: () => void): void;
  /** A timing mark for the joint test session (ANNOTA-20): only in a build made with `VITE_TIMINGS=1`. */
  mark?(name: string): void;
}

export const tauriIpc: EditorIpc = {
  nextSnap: () => invoke<ArrayBuffer>("next_snap"),
  painted: () => invoke("editor_painted"),
  copy: (rgba, width, height) =>
    invoke("copy_snap", new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.byteLength), {
      headers: { width: String(width), height: String(height) },
    }),
  discard: () => invoke("discard_snap"),
  onCopyRequested: (handler) => {
    void listen("copy-requested", handler);
  },
  // Dead code in a normal build: Vite folds the constant away and Rust has no `timing_mark` command.
  mark: import.meta.env.VITE_TIMINGS
    ? (name) => {
        void invoke("timing_mark", { name });
      }
    : undefined,
};
