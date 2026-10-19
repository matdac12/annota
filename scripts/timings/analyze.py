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
    ("exported -> changed tiles packed", "page_exported", "page_packed"),
    ("exported -> Rust got pixels (IPC)", "page_exported", "copy_recv"),
    ("Rust got pixels -> Region patched", "copy_recv", "delta_applied"),
    ("Rust got pixels -> clipboard set", "copy_recv", "clipboard_set"),
    ("clipboard set -> Editor closed", "clipboard_set", "closed"),
    ("CTRL+C -> CLIPBOARD SET", "copy_key", "clipboard_set"),
    ("CTRL+C -> CLOSED", "copy_key", "closed"),
    ("Caps+S in Editor -> CLOSED", "copy_requested", "closed"),
]


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
    if blits:
        print("per-monitor blit finish (ms after grab_start, pixels):", ", ".join(f"{m:.1f} ({p/1e6:.1f} MP)" for m, p in blits[:8]), "...")


if __name__ == "__main__":
    main()
