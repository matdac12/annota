// No console window in release builds; `tauri dev` keeps one so `start_snap` can log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod capture;
mod commands;
mod editor;
mod geometry;
mod launch;
mod overlay;
mod resident;
mod single_instance;
mod snap;
mod timings;
mod tray;

use launch::Launch;
use resident::Start;

/// The page's commands. A timings build (ANNOTA-20) adds `timing_mark`.
#[cfg(feature = "timings")]
fn with_commands(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        commands::next_snap,
        commands::editor_painted,
        commands::copy_snap,
        commands::discard_snap,
        commands::timing_mark
    ])
}

#[cfg(not(feature = "timings"))]
fn with_commands(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        commands::next_snap,
        commands::editor_painted,
        commands::copy_snap,
        commands::discard_snap
    ])
}

fn main() {
    timings::process_start();
    // 1-2. Fast path: a Resident is running, so hand it our whole command line and exit; it parses the
    // arguments (`Launch::parse`) in the plugin callback below. Nothing runs before this: no logging,
    // no file I/O, no statics.
    if !timings::fast_path_disabled() && single_instance::forward_to_resident() {
        std::process::exit(0);
    }

    // 3. Annota's own mutex, then 4. the elevation trap. (`--no-fast` in a timings build skips both, so
    // the stock plugin does the handoff; only for timing against a running Resident.)
    let claim = resident::claim();
    let start = if timings::fast_path_disabled() { Start::Run } else { resident::start(claim.first, resident::is_elevated()) };
    match start {
        Start::Run => {}
        // A Resident is still starting, so its plugin window isn't up yet: the `--snap` is dropped.
        Start::Exit => std::process::exit(0),
        // Never become the Resident from an elevated process; start it through the shell instead.
        Start::RelaunchThroughShell => {
            std::process::exit(if resident::relaunch_through_shell(claim).is_ok() { 0 } else { 1 })
        }
    }
    // Held until the process exits: Windows releases the mutex then.
    let _claim = claim;

    // 5. Tauri, with the single-instance plugin first.
    with_commands(tauri::Builder::default())
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // Runs inside `WM_COPYDATA` on the main thread: decide and return.
            if timings::callback(&argv) {
                return;
            }
            if Launch::parse(argv).snap {
                snap::request_snap(app);
            }
        }))
        .manage(snap::Resident::new())
        .manage(commands::Handoff::default())
        .setup(|app| {
            tray::build(app)?;
            editor::build(app)?;
            autostart::register_on_first_run();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Annota failed to start");
}
