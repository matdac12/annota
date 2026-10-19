"""ANNOTA-20: turns the tray's "Dump timings" text (stdin) into per-interval stats. Reads stdin, prints stdout;
writes nothing. Dumps may be concatenated; `freq=` lines reset nothing, only the last one counts.

    python -I scripts/timings/analyze.py < dump.txt

Percentiles use linear interpolation between ranks (the numpy default). Times are milliseconds.
"""
import sys
from collections import defaultdict

# metric name -> (from mark, to mark). The last `from` before each `to` within one snap is used.
INTERVALS = [
    ("key -> callback (A: hyper.ahk QPC to callback top)", "ahk_key", "callback"),
    ("  key -> second process main()", "ahk_key", "proc_main"),
    ("  main() -> just before WM_COPYDATA", "proc_main", "proc_sent"),
    ("  send -> callback top", "proc_sent", "callback"),
    ("callback -> all monitors captured", "grab_start", "grab_done"),
    ("callback top -> grab start", "callback", "grab_start"),
    ("grab done -> Overlay up (dim copies + show)", "grab_done", "overlay_up"),
    ("mouse-up -> Region cropped", "mouse_up", "region"),
    ("Region -> pixels delivered (place Editor)", "region", "deliver"),
    ("delivered -> page has bytes", "deliver", "page_bytes"),
    ("page bytes -> putImageData done", "page_bytes", "page_drawn"),
    ("putImageData done -> frame presented", "page_drawn", "page_frame"),
    ("deliver -> Rust got editor_painted (warm show -> first paint)", "deliver", "painted"),
    ("painted -> Editor shown", "painted", "shown"),
    ("MOUSE-UP -> EDITOR SHOWN", "mouse_up", "shown"),
    ("Ctrl+C -> page exported (render + getImageData)", "copy_key", "page_exported"),
    ("exported -> Rust got pixels (IPC)", "page_exported", "copy_recv"),
    ("Rust got pixels -> clipboard set", "copy_recv", "clipboard_set"),
    ("clipboard set -> Editor closed", "clipboard_set", "closed"),
    ("CTRL+C -> CLIPBOARD SET", "copy_key", "clipboard_set"),
    ("CTRL+C -> CLOSED", "copy_key", "closed"),
    ("Caps+S in Editor -> CLOSED", "copy_requested", "closed"),
]


# ANNOTA-23: per copy candidate (`copy_mode` = clipboard writer * 10 + transfer).
CLIP_NAMES = {0: "arboard", 1: "own + fast PNG", 2: "own + stored PNG"}
TRANSFER_NAMES = {0: "raw", 1: "delta", 2: "shared"}
COPY_INTERVALS = [
    ("Ctrl+C -> page packed (delta encode / shared fill)", "copy_key", "page_packed"),
    ("page exported -> Rust got pixels", "page_exported", "copy_recv"),
    ("Rust got pixels -> PNG encoded (own writer)", "copy_recv", "png_done"),
    ("Rust got pixels -> DIBV5 filled (own writer)", "copy_recv", "dib_done"),
    ("Rust got pixels -> clipboard set", "copy_recv", "clipboard_set"),
    ("CTRL+C -> CLIPBOARD SET", "copy_key", "clipboard_set"),
    ("CTRL+C -> CLOSED", "copy_key", "closed"),
]


def copy_table(marks, hz):
    """Prints the copy intervals once per candidate, from the snaps whose `copy_mode` mark names it."""
    by_mode = defaultdict(list)
    for snap, rows in sorted(marks.items()):
        mode = next((d for n, _, d in reversed(rows) if n == "copy_mode"), None)
        if mode is not None:
            by_mode[mode].append(rows)
    for mode, snaps in sorted(by_mode.items()):
        print()
        print(f"copy candidate: clipboard={CLIP_NAMES.get(mode // 10, mode // 10)}, transfer={TRANSFER_NAMES.get(mode % 10, mode % 10)}")
        print(f"{'interval':64} {'n':>3} {'min':>8} {'p50':>8} {'p95':>8} {'max':>8}")
        for label, a, b in COPY_INTERVALS:
            v = []
            for rows in snaps:
                end = next((t for n, t, _ in reversed(rows) if n == b), None)
                start = next((t for n, t, _ in reversed(rows) if n == a and end is not None and t <= end), None)
                if end is not None and start is not None:
                    v.append((end - start) * 1000.0 / hz)
            if v:
                v.sort()
                print(f"{label:64} {len(v):3d} {v[0]:8.1f} {percentile(v, .5):8.1f} {percentile(v, .95):8.1f} {v[-1]:8.1f}")


def percentile(sorted_values, q):
    if len(sorted_values) == 1:
        return sorted_values[0]
    pos = (len(sorted_values) - 1) * q
    lo = int(pos)
    hi = min(lo + 1, len(sorted_values) - 1)
    return sorted_values[lo] + (sorted_values[hi] - sorted_values[lo]) * (pos - lo)


def main():
    freq = None
    marks = defaultdict(list)  # snap -> [(name, ticks, detail)]
    life = []
    ahk_freq = None
    for line in sys.stdin:
        parts = line.split()
        if not parts:
            continue
        if parts[0].startswith("freq="):
            freq = int(parts[0][5:])
        elif parts[0].startswith("ahk_freq="):
            ahk_freq = int(parts[0][9:])
        elif parts[0] == "life":
            life.append(int(parts[1]))
        elif len(parts) == 4 and parts[0].isdigit():
            marks[int(parts[0])].append((parts[1], int(parts[2]), int(parts[3])))
    hz = freq or ahk_freq or 10_000_000
    results = defaultdict(list)
    blits = []
    for snap, rows in sorted(marks.items()):
        for label, a, b in INTERVALS:
            end = next((t for n, t, _ in reversed(rows) if n == b), None)
            if end is None:
                continue
            start = next((t for n, t, _ in reversed(rows) if n == a and t <= end), None)
            if start is None:
                continue
            results[label].append((end - start) * 1000.0 / hz)
        grab_start = next((t for n, t, _ in rows if n == "grab_start"), None)
        for n, t, d in rows:
            if n == "blit" and grab_start is not None:
                blits.append(((t - grab_start) * 1000.0 / hz, d))
    if life:
        results["second process lifetime (AHK RunWait, CreateProcess -> exit)"] = [t * 1000.0 / hz for t in life]
    print(f"QPC {hz} Hz; {len(marks)} snap rows")
    print(f"{'interval':64} {'n':>3} {'min':>8} {'p50':>8} {'p95':>8} {'max':>8}")
    for label, _, _ in INTERVALS:
        if label in results:
            v = sorted(results[label])
            print(f"{label:64} {len(v):3d} {v[0]:8.1f} {percentile(v, .5):8.1f} {percentile(v, .95):8.1f} {v[-1]:8.1f}")
    for label in results:
        if label.startswith("second process"):
            v = sorted(results[label])
            print(f"{label:64} {len(v):3d} {v[0]:8.1f} {percentile(v, .5):8.1f} {percentile(v, .95):8.1f} {v[-1]:8.1f}")
    copy_table(marks, hz)
    if blits:
        print("per-monitor blit finish (ms after grab_start, pixels):", ", ".join(f"{m:.1f} ({p/1e6:.1f} MP)" for m, p in blits[:8]), "...")


if __name__ == "__main__":
    main()
