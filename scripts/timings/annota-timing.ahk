; --- Annota (ANNOTA-20 TIMING VARIANT: temporary, restored byte-identical after the session) ---
; Same Caps+S as the real binding, but it stamps QueryPerformanceCounter at the hotkey entry and hands
; the stamp to annota.exe as `--qpc=<ticks>` (a normal build ignores it). Modes, Caps+F9 cycles them:
;   fast  - the shipped fast path
;   stock - `--no-fast`: the stock single-instance plugin does the handoff (timings build only)
;   wait  - fast path, but RunWait: logs CreateProcess-to-exit of the second process (AHK side)
; Caps+F10 puts the AHK-side log on the clipboard. Nothing is written to disk.
global AnnotaMode := "fast"
global AnnotaLife := []

CapsLock & s:: {
    DllCall("QueryPerformanceCounter", "Int64*", &t0 := 0)
    exe := '"' EnvGet("LOCALAPPDATA") '\Annota\annota.exe" --snap'
    if (AnnotaMode = "stock") {
        Run exe ' --no-fast --qpc=' t0
    } else if (AnnotaMode = "wait") {
        RunWait exe ' --qpc=' t0
        DllCall("QueryPerformanceCounter", "Int64*", &t1 := 0)
        AnnotaLife.Push(t1 - t0)
    } else {
        Run exe ' --qpc=' t0
    }
    KeyWait "CapsLock", "T0.4"
}

CapsLock & F9:: {
    global AnnotaMode
    AnnotaMode := (AnnotaMode = "fast") ? "stock" : (AnnotaMode = "stock") ? "wait" : "fast"
    ToolTip "Annota timing mode: " AnnotaMode
    SetTimer () => ToolTip(), -1500
}

CapsLock & F10:: {
    global AnnotaLife
    DllCall("QueryPerformanceFrequency", "Int64*", &hz := 0)
    out := "ahk_freq=" hz "`n"
    for ticks in AnnotaLife
        out .= "life " ticks "`n"
    A_Clipboard := out
    AnnotaLife := []
    ToolTip "AHK log copied to the clipboard"
    SetTimer () => ToolTip(), -1500
}

