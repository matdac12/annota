//! Start with Windows (spec §11.2): `HKCU\…\Run\Annota` = `"<exe>"`, written by Annota itself.
//!
//! Written here rather than through the autostart plugin, which writes the path unquoted (a path with
//! spaces then breaks), tries `HKLM` first, and turns the entry back on in Task Manager at every start.
//! This writes the per-user value only, quoted, and leaves Task Manager's on/off switch to the owner.
//! The installer's uninstaller removes the value (`nsis-hooks.nsh`).

/// The Run value's name. The installer's uninstall hook deletes the same name.
pub const VALUE_NAME: &str = "Annota";

/// The Run value's data: the exe path, quoted, and nothing else (Annota always starts hidden).
pub fn command_line(exe: &str) -> String {
    format!("\"{exe}\"")
}

/// The registry reads and writes autostart needs. The Windows registry implements it; the tests use
/// an in-memory one so the logic runs without touching `HKCU`.
pub trait Registry {
    /// Whether the Run value exists.
    fn run_value_exists(&self) -> bool;
    fn set_run_value(&self, data: &str) -> Result<(), u32>;
    fn delete_run_value(&self);
    /// The raw bytes of the Task Manager on/off entry (`StartupApproved\Run\Annota`), if any.
    fn startup_approved(&self) -> Option<Vec<u8>>;
    fn delete_startup_approved(&self);
    /// Whether `HKCU\Software\Annota` `AutostartInitialized` is set.
    fn initialized(&self) -> bool;
    fn set_initialized(&self) -> Result<(), u32>;
}

/// Task Manager and Settings keep one 12-byte entry per startup item. An even first byte (02, 06)
/// means enabled, an odd one (03, 07) disabled. A missing or empty entry means nobody disabled it.
fn startup_approved_is_disabled(bytes: Option<&[u8]>) -> bool {
    bytes.and_then(|b| b.first()).is_some_and(|b| b % 2 == 1)
}

/// The tray checkbox: the Run value exists and Task Manager hasn't disabled it.
pub fn is_enabled(reg: &dyn Registry) -> bool {
    reg.run_value_exists() && !startup_approved_is_disabled(reg.startup_approved().as_deref())
}

/// Ticks or unticks the checkbox. Unticking deletes the Run value. Ticking writes it and clears a
/// "disabled" Task Manager entry, so the tick takes effect.
pub fn set_enabled(reg: &dyn Registry, exe: &str, enabled: bool) {
    if !enabled {
        reg.delete_run_value();
        return;
    }
    if startup_approved_is_disabled(reg.startup_approved().as_deref()) {
        reg.delete_startup_approved();
    }
    let _ = reg.set_run_value(&command_line(exe));
}

/// First run only: if the marker is absent, writes the Run value and sets the marker, so an untick
/// sticks across updates and reinstalls. The marker is set only once the Run value is written.
pub fn register_first_run(reg: &dyn Registry, exe: &str) {
    if reg.initialized() {
        return;
    }
    if reg.set_run_value(&command_line(exe)).is_ok() {
        let _ = reg.set_initialized();
    }
}

/// Whether the tray checkbox can be changed. A debug build (`tauri dev`) must never write its own
/// exe path over the installed release build's Run value, so there it is read-only.
pub fn can_change() -> bool {
    !cfg!(debug_assertions)
}

/// Runs `f` against the real registry and this exe's path. Does nothing in a debug build, or if the
/// exe path is unknown: the app works without autostart.
fn on_this_exe(f: impl FnOnce(&dyn Registry, &str)) {
    if !can_change() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        f(&sys::HkcuRegistry, &exe.display().to_string());
    }
}

/// Registers this exe at the first release-build run.
pub fn register_on_first_run() {
    on_this_exe(register_first_run);
}

/// The tray checkbox's state, read from the registry every time the menu opens.
pub fn current_state() -> bool {
    is_enabled(&sys::HkcuRegistry)
}

/// Flips the tray checkbox: from what the registry says now, not from what the menu showed.
pub fn toggle() {
    on_this_exe(|reg, exe| set_enabled(reg, exe, !is_enabled(reg)));
}

mod sys {
    use super::{Registry, VALUE_NAME};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ,
        RRF_RT_ANY,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const APP_KEY: &str = r"Software\Annota";
    const AUTOSTART_INITIALIZED: &str = "AutostartInitialized";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// The raw bytes of `HKCU\<key>` value `name`, or `None` if it is absent.
    fn read(key: &str, name: &str) -> Option<Vec<u8>> {
        let (key, name) = (wide(key), wide(name));
        let mut size = 0u32;
        // SAFETY: both are NUL-terminated UTF-16 buffers that outlive the call; a null data pointer
        // asks only for the size.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_ANY,
                null_mut(),
                null_mut(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        // SAFETY: as above; `data` has `size` writable bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_ANY,
                null_mut(),
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        data.truncate(size as usize);
        Some(data)
    }

    fn write(key: &str, name: &str, kind: u32, data: &[u8]) -> Result<(), u32> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: NUL-terminated UTF-16 key and name; `data` is `data.len()` readable bytes, which
        // is what the size argument says.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                kind,
                data.as_ptr().cast(),
                data.len() as u32,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(status)
        }
    }

    /// Deletes exactly one value; the key stays.
    fn delete(key: &str, name: &str) {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: NUL-terminated UTF-16 buffers that outlive the call. A missing value is not an error
        // worth reporting here.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
    }

    pub struct HkcuRegistry;

    impl Registry for HkcuRegistry {
        fn run_value_exists(&self) -> bool {
            read(RUN_KEY, VALUE_NAME).is_some()
        }
        fn set_run_value(&self, data: &str) -> Result<(), u32> {
            let bytes: Vec<u8> = wide(data).iter().flat_map(|u| u.to_le_bytes()).collect();
            write(RUN_KEY, VALUE_NAME, REG_SZ, &bytes)
        }
        fn delete_run_value(&self) {
            delete(RUN_KEY, VALUE_NAME);
        }
        fn startup_approved(&self) -> Option<Vec<u8>> {
            read(APPROVED_KEY, VALUE_NAME)
        }
        fn delete_startup_approved(&self) {
            delete(APPROVED_KEY, VALUE_NAME);
        }
        fn initialized(&self) -> bool {
            read(APP_KEY, AUTOSTART_INITIALIZED).is_some()
        }
        fn set_initialized(&self) -> Result<(), u32> {
            write(
                APP_KEY,
                AUTOSTART_INITIALIZED,
                REG_DWORD,
                &1u32.to_le_bytes(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// In-memory stand-in for the registry keys autostart touches.
    #[derive(Default)]
    struct FakeRegistry {
        run: RefCell<Option<String>>,
        approved: RefCell<Option<Vec<u8>>>,
        initialized: RefCell<bool>,
        /// Makes `set_run_value` fail, as when the key is locked down.
        fail_writes: Cell<bool>,
    }

    impl Registry for FakeRegistry {
        fn run_value_exists(&self) -> bool {
            self.run.borrow().is_some()
        }
        fn set_run_value(&self, data: &str) -> Result<(), u32> {
            if self.fail_writes.get() {
                return Err(5);
            }
            *self.run.borrow_mut() = Some(data.to_string());
            Ok(())
        }
        fn delete_run_value(&self) {
            *self.run.borrow_mut() = None;
        }
        fn startup_approved(&self) -> Option<Vec<u8>> {
            self.approved.borrow().clone()
        }
        fn delete_startup_approved(&self) {
            *self.approved.borrow_mut() = None;
        }
        fn initialized(&self) -> bool {
            *self.initialized.borrow()
        }
        fn set_initialized(&self) -> Result<(), u32> {
            *self.initialized.borrow_mut() = true;
            Ok(())
        }
    }

    const ENABLED: [u8; 12] = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const ENABLED_BY_SETTINGS: [u8; 12] = [0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const DISABLED: [u8; 12] = [0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const DISABLED_BY_SETTINGS: [u8; 12] = [0x07, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    fn registry(run: bool, approved: Option<&[u8]>) -> FakeRegistry {
        let reg = FakeRegistry::default();
        if run {
            reg.set_run_value("\"x\"").unwrap();
        }
        *reg.approved.borrow_mut() = approved.map(<[u8]>::to_vec);
        reg
    }

    #[test]
    fn the_checkbox_is_unticked_without_a_run_value_whatever_task_manager_says() {
        for approved in [None, Some(&ENABLED[..]), Some(&DISABLED[..])] {
            assert!(!is_enabled(&registry(false, approved)));
        }
    }

    #[test]
    fn the_checkbox_is_ticked_with_a_run_value_task_manager_has_not_disabled() {
        for approved in [
            None,
            Some(&ENABLED[..]),
            Some(&ENABLED_BY_SETTINGS[..]),
            Some(&[][..]),
        ] {
            assert!(is_enabled(&registry(true, approved)));
        }
    }

    #[test]
    fn unticking_deletes_the_run_value() {
        let reg = registry(true, Some(&ENABLED));
        set_enabled(&reg, r"C:\Annota\annota.exe", false);
        assert!(!reg.run_value_exists());
        assert!(!is_enabled(&reg));
    }

    #[test]
    fn ticking_writes_the_quoted_run_value() {
        let reg = registry(false, None);
        set_enabled(&reg, r"C:\Jane Doe\Annota\annota.exe", true);
        assert_eq!(
            reg.run.borrow().as_deref(),
            Some(r#""C:\Jane Doe\Annota\annota.exe""#)
        );
        assert!(is_enabled(&reg));
    }

    #[test]
    fn ticking_clears_a_disabled_task_manager_entry() {
        let reg = registry(true, Some(&DISABLED));
        assert!(!is_enabled(&reg));
        set_enabled(&reg, r"C:\Annota\annota.exe", true);
        assert!(is_enabled(&reg));
    }

    #[test]
    fn ticking_leaves_an_enabled_task_manager_entry_alone() {
        let reg = registry(false, Some(&ENABLED_BY_SETTINGS));
        set_enabled(&reg, r"C:\Annota\annota.exe", true);
        assert_eq!(
            reg.startup_approved().as_deref(),
            Some(&ENABLED_BY_SETTINGS[..])
        );
    }

    #[test]
    fn the_first_run_registers_and_sets_the_marker() {
        let reg = registry(false, None);
        register_first_run(&reg, r"C:\Annota\annota.exe");
        assert!(is_enabled(&reg));
        assert!(reg.initialized());
    }

    #[test]
    fn after_the_first_run_an_untick_sticks() {
        let reg = registry(false, None);
        register_first_run(&reg, r"C:\Annota\annota.exe");
        set_enabled(&reg, r"C:\Annota\annota.exe", false);
        register_first_run(&reg, r"C:\Annota\annota.exe");
        assert!(!reg.run_value_exists());
    }

    #[test]
    fn a_reinstall_with_the_marker_present_does_not_register() {
        let reg = registry(false, None);
        reg.set_initialized().unwrap();
        register_first_run(&reg, r"C:\Annota\annota.exe");
        assert!(!reg.run_value_exists());
    }

    #[test]
    fn the_first_run_leaves_the_marker_unset_if_the_run_value_could_not_be_written() {
        let reg = FakeRegistry::default();
        reg.fail_writes.set(true);
        register_first_run(&reg, r"C:\Annota\annota.exe");
        assert!(!reg.initialized());
    }

    #[test]
    fn the_checkbox_is_unticked_when_task_manager_has_disabled_the_run_value() {
        for approved in [&DISABLED[..], &DISABLED_BY_SETTINGS[..]] {
            assert!(!is_enabled(&registry(true, Some(approved))));
        }
    }

    #[test]
    fn quotes_the_path_so_spaces_survive_and_adds_no_arguments() {
        assert_eq!(
            command_line(r"C:\Users\Jane Doe\AppData\Local\Annota\annota.exe"),
            r#""C:\Users\Jane Doe\AppData\Local\Annota\annota.exe""#
        );
    }
}
