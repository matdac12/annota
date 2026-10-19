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
/** The serial of the Snap now open: a shared buffer is this Snap's only if it carries the same one. */
let serial = 0;
let transfer: Transfer = "raw";
void listen<[number, number]>("transfer-mode", (event) => {
  const [mode, snapSerial] = event.payload;
  transfer = TRANSFERS[mode] ?? "raw";
  serial = snapSerial;
});

/** The WebView2 shared buffer Rust posted last, with the serial it was tagged with. Either may arrive first. */
let shared: { buffer: ArrayBuffer; serial: number } | null = null;
interface SharedBufferEvent {
  additionalData?: { serial?: number };
  getBuffer(): ArrayBuffer;
}
interface WebViewHost {
  addEventListener(type: "sharedbufferreceived", handler: (event: SharedBufferEvent) => void): void;
}
(window as unknown as { chrome?: { webview?: WebViewHost } }).chrome?.webview?.addEventListener(
  "sharedbufferreceived",
  (event) => {
    shared = { buffer: event.getBuffer(), serial: event.additionalData?.serial ?? -1 };
  },
);

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
 * Sends the export the way Rust asked for. Anything not ready, or that fails, falls back to the raw body (the
 * delta and shared paths spend what Rust kept for the Snap, so the raw one is the only retry).
 */
async function copyRgba(rgba: Uint8ClampedArray, width: number, height: number, original?: Uint8ClampedArray): Promise<void> {
  try {
    if (transfer === "delta" && original) {
      const payload = encodeDelta(original, rgba, width, height);
      mark?.("page_packed");
      return await invoke("copy_delta", payload);
    }
    if (transfer === "shared" && shared?.serial === serial && shared.buffer.byteLength === rgba.byteLength) {
      new Uint8Array(shared.buffer).set(rgba);
      mark?.("page_packed");
      return await invoke("copy_shared");
    }
  } catch (error) {
    console.error(`copy by ${transfer} failed, sending the raw pixels`, error);
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
