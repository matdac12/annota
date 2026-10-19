; Hooks for the per-user NSIS installer (tauri.conf.json bundle.windows.nsis.installerHooks).
; Annota registers its own logon autostart (src/autostart.rs); uninstalling removes it, the Task
; Manager on/off switch Windows may have stored for it, and Annota's own registry key (the
; first-run marker).
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Annota"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Annota"
  DeleteRegKey HKCU "Software\Annota"
!macroend
