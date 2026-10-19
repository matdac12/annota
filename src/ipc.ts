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
   * The exported RGBA goes to the clipboard, then Rust closes the Editor. `original` is the Region as Rust sent
   * it, for the delta candidate (ANNOTA-23).
   */
  copy(rgba: Uint8ClampedArray, width: number, height: number, original?: Uint8ClampedArray): Promise<void>;
  /** Close the Editor with nothing copied. */
  discard(): Promise<void>;
  /** `--snap` arrived while the Editor is open. */
  onCopyRequested(handler: () => void): void;
  /** A timing mark for the joint test session (ANNOTA-20): only in a build made with `VITE_TIMINGS=1`. */
  mark?(name: string): void;
}

/** How Rust wants the pixels on Ctrl+C (ANNOTA-23); it says so with `transfer-mode` when the Editor shows. */
type Transfer = "raw" | "delta" | "shared";
const TRANSFERS: Transfer[] = ["raw", "delta", "shared"];
let transfer: Transfer = "raw";
void listen<number>("transfer-mode", (event) => {
  transfer = TRANSFERS[event.payload] ?? "raw";
});

/** The WebView2 shared buffer Rust posted for this Snap, once it has arrived. */
let sharedBuffer: ArrayBuffer | null = null;
interface WebViewHost {
  addEventListener(type: "sharedbufferreceived", handler: (event: { getBuffer(): ArrayBuffer }) => void): void;
}
(window as unknown as { chrome?: { webview?: WebViewHost } }).chrome?.webview?.addEventListener(
  "sharedbufferreceived",
  (event) => {
    sharedBuffer = event.getBuffer();
  },
);

const mark = import.meta.env.VITE_TIMINGS
  ? (name: string) => {
      void invoke("timing_mark", { name });
    }
  : undefined;

/** Sends the export the way Rust asked for. Anything not ready falls back to the raw body. */
async function copyRgba(rgba: Uint8ClampedArray, width: number, height: number, original?: Uint8ClampedArray): Promise<void> {
  if (transfer === "delta" && original) {
    const payload = encodeDelta(original, rgba, width, height);
    mark?.("page_packed");
    return invoke("copy_delta", payload);
  }
  // A buffer from an earlier Snap was closed, which detaches it: its length is no longer this Region's.
  if (transfer === "shared" && sharedBuffer && sharedBuffer.byteLength === rgba.byteLength) {
    new Uint8Array(sharedBuffer).set(rgba);
    mark?.("page_packed");
    return invoke("copy_shared");
  }
  return invoke("copy_snap", new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.byteLength), {
    headers: { width: String(width), height: String(height) },
  });
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
