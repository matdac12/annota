// No console window in release builds; `tauri dev` keeps one so `start_snap` can log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod editor;
mod snap;
mod tray;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            tray::build(app)?;
            editor::build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Annota failed to start");
}
