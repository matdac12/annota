//! The tray icon: left-click and the Snap item start a Snap; Start with Windows is a checkbox; Quit exits.

use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle};

use crate::{autostart, snap};

const SNAP: &str = "snap";
const AUTOSTART: &str = "autostart";
const QUIT: &str = "quit";

/// What a tray event asks for, decided apart from the app so it can be tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Snap,
    Quit,
}

fn action_for_menu_item(id: &str) -> Option<Action> {
    match id {
        SNAP => Some(Action::Snap),
        QUIT => Some(Action::Quit),
        _ => None,
    }
}

/// A left-click snaps once, on release.
fn action_for_click(button: MouseButton, state: MouseButtonState) -> Option<Action> {
    match (button, state) {
        (MouseButton::Left, MouseButtonState::Up) => Some(Action::Snap),
        _ => None,
    }
}

fn run(app: &AppHandle, action: Action) {
    match action {
        Action::Snap => snap::request_snap(app),
        Action::Quit => app.exit(0),
    }
}

/// Shows what the registry says now, since Task Manager can change it behind the menu's back.
fn refresh_autostart(item: &CheckMenuItem<tauri::Wry>) {
    let _ = item.set_checked(autostart::current_state());
}

/// A right-button release opens the tray menu right after this event, so the checkbox is re-read
/// just before it shows.
fn opens_menu(button: MouseButton, state: MouseButtonState) -> bool {
    matches!((button, state), (MouseButton::Right, MouseButtonState::Up))
}

pub fn build(app: &App) -> tauri::Result<()> {
    let snap_item = MenuItem::with_id(app, SNAP, "Snap", true, None::<&str>)?;
    let autostart_item = CheckMenuItem::with_id(
        app,
        AUTOSTART,
        "Start with Windows",
        autostart::can_change(),
        autostart::current_state(),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&snap_item, &autostart_item, &quit])?;
    #[cfg(feature = "timings")]
    let timing_items = crate::timings::tray_items(app)?;
    #[cfg(feature = "timings")]
    timing_items.append_to(&menu)?;
    let on_click_item = autostart_item.clone();
    let on_menu_item = autostart_item;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Annota")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            #[cfg(feature = "timings")]
            if timing_items.handle(event.id().as_ref()) {
                return;
            }
            if event.id().as_ref() == AUTOSTART {
                // The click already flipped the box; flip the registry, then show the registry's
                // answer rather than assume it worked.
                autostart::toggle();
                refresh_autostart(&on_menu_item);
            } else if let Some(action) = action_for_menu_item(event.id().as_ref()) {
                run(app, action);
            }
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if opens_menu(button, button_state) {
                    refresh_autostart(&on_click_item);
                }
                if let Some(action) = action_for_click(button, button_state) {
                    run(tray.app_handle(), action);
                }
            }
        });
    if let Some(icon) = load_tray_icon(app) {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    Ok(())
}

/// `0` means `GetSystemMetrics` failed. The tray then asks for 16 px, which is
/// the small icon size at 96 DPI.
fn tray_icon_px(metric: i32) -> u32 {
    u32::try_from(metric)
        .ok()
        .filter(|px| *px > 0)
        .unwrap_or(16)
}

/// The notification area draws at the small icon size (16 px at 96 DPI, 20 at
/// 125%, 24 at 150%). The default window icon is the large size, so using it
/// here would shrink the detailed drawing and skip the 16, 20 and 24 entries.
fn load_tray_icon(app: &App) -> Option<tauri::image::Image<'static>> {
    #[cfg(windows)]
    {
        let px = tray_icon_px(small_icon_metric());
        if let Ok(icon) = tauri::image::Image::from_app_icon_resource(px) {
            return Some(icon);
        }
    }
    app.default_window_icon()
        .cloned()
        .map(|icon| icon.to_owned())
}

#[cfg(windows)]
fn small_icon_metric() -> i32 {
    // SAFETY: reads one system metric. No handle, no pointer.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CXSMICON,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snap_item_and_a_left_click_both_start_a_snap() {
        assert_eq!(action_for_menu_item("snap"), Some(Action::Snap));
        assert_eq!(
            action_for_click(MouseButton::Left, MouseButtonState::Up),
            Some(Action::Snap)
        );
    }

    #[test]
    fn the_quit_item_quits() {
        assert_eq!(action_for_menu_item("quit"), Some(Action::Quit));
    }

    #[test]
    fn a_click_snaps_once_on_release_and_only_with_the_left_button() {
        assert_eq!(
            action_for_click(MouseButton::Left, MouseButtonState::Down),
            None
        );
        assert_eq!(
            action_for_click(MouseButton::Right, MouseButtonState::Up),
            None
        );
        assert_eq!(
            action_for_click(MouseButton::Middle, MouseButtonState::Up),
            None
        );
    }

    #[test]
    fn only_releasing_the_right_button_opens_the_menu() {
        assert!(opens_menu(MouseButton::Right, MouseButtonState::Up));
        assert!(!opens_menu(MouseButton::Right, MouseButtonState::Down));
        assert!(!opens_menu(MouseButton::Left, MouseButtonState::Up));
    }

    #[test]
    fn an_unknown_menu_item_does_nothing() {
        assert_eq!(action_for_menu_item("settings"), None);
    }

    #[test]
    fn the_tray_uses_the_small_icon_size_and_16_px_when_the_metric_is_missing() {
        assert_eq!(tray_icon_px(16), 16);
        assert_eq!(tray_icon_px(20), 20);
        assert_eq!(tray_icon_px(24), 24);
        assert_eq!(tray_icon_px(0), 16);
        assert_eq!(tray_icon_px(-1), 16);
    }
}
