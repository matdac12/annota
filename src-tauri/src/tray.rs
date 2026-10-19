//! The tray icon: left-click and the Snap item start a Snap; Quit exits.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle};

use crate::snap;

const SNAP: &str = "snap";
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
        Action::Snap => snap::start_snap(),
        Action::Quit => app.exit(0),
    }
}

pub fn build(app: &App) -> tauri::Result<()> {
    let snap_item = MenuItem::with_id(app, SNAP, "Snap", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&snap_item, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Annota")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if let Some(action) = action_for_menu_item(event.id().as_ref()) {
                run(app, action);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if let Some(action) = action_for_click(button, button_state) {
                    run(tray.app_handle(), action);
                }
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
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
    fn an_unknown_menu_item_does_nothing() {
        assert_eq!(action_for_menu_item("settings"), None);
    }
}
