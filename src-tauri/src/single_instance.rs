//! What the `--snap` fast path borrows from `tauri-plugin-single-instance` 2.5.4 (spec §3.2).
//!
//! The plugin's Windows handoff is a hidden top-level window plus `WM_COPYDATA` (see its
//! `platform_impl/windows.rs`). Its class and title are `{identifier}-sic` / `{identifier}-siw`, and the
//! payload is `dwData = 1543` over UTF-8 `cwd NUL arg0 NUL arg1 …` with no terminator. They are private, so
//! the crate is pinned `=2.5.4` and everything the fast path assumes lives here, pinned by a test.

/// Bundle identifier, `identifier` in `tauri.conf.json`. The plugin derives its names from it; the test
/// checks that the names below still follow it.
#[cfg(test)]
const IDENTIFIER: &str = "app.annota.desktop";

/// The class of the plugin's hidden window: `{identifier}-sic`.
pub const WINDOW_CLASS: &str = "app.annota.desktop-sic";
/// The title of the plugin's hidden window: `{identifier}-siw`.
pub const WINDOW_TITLE: &str = "app.annota.desktop-siw";

/// `dwData` of the plugin's current payload: cwd and arguments separated by NUL.
pub const COPYDATA_KIND: usize = 1543;

/// How long the second process waits for the Resident to take the payload (the plugin's own value).
pub const FORWARD_TIMEOUT_MS: u32 = 10_000;

/// The plugin's payload: `cwd`, then every argument (the exe path first), each after a NUL.
pub fn encode_payload<S: AsRef<str>>(cwd: &str, args: &[S]) -> Vec<u8> {
    let mut data = String::from(cwd);
    for arg in args {
        data.push('\0');
        data.push_str(arg.as_ref());
    }
    data.into_bytes()
}

/// The fast path (spec §3.2 step 2): if the Resident's plugin window exists, hand it this process's
/// command line the way the plugin itself would, and report that it was handed over (the caller then
/// exits). Does nothing else first: no logging, no file I/O.
#[cfg(windows)]
pub fn forward_to_resident() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, SendMessageTimeoutW,
        SMTO_NORMAL, WM_COPYDATA,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let (class, title) = (wide(WINDOW_CLASS), wide(WINDOW_TITLE));
    // SAFETY: both are NUL-terminated UTF-16 that outlive the call.
    let Ok(hwnd) = (unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR(title.as_ptr())) }) else {
        return false;
    };

    // Windows lets this process bring a window to the front, but not the Resident. Hand that right over
    // before exiting, so the Overlay can take focus.
    let mut pid = 0u32;
    // SAFETY: `hwnd` was just found; `pid` is a live out-pointer.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != 0 {
        // SAFETY: a plain Win32 call with no pointers.
        let _ = unsafe { AllowSetForegroundWindow(pid) };
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let args: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
    let bytes = encode_payload(&cwd.to_string_lossy(), &args);
    let data = COPYDATASTRUCT {
        dwData: COPYDATA_KIND,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr() as *mut _,
    };
    // SAFETY: `data` and `bytes` outlive the call, which returns only after the Resident's window
    // procedure has handled the message or the timeout has passed.
    unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&data as *const COPYDATASTRUCT as isize),
            SMTO_NORMAL,
            FORWARD_TIMEOUT_MS,
            None,
        );
    }
    // Like the plugin, exit even when the send timed out: the Resident is there, a second one is not wanted.
    true
}

/// No plugin window to find off Windows.
#[cfg(not(windows))]
pub fn forward_to_resident() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_the_plugins_for_our_bundle_identifier() {
        assert_eq!(WINDOW_CLASS, format!("{IDENTIFIER}-sic"));
        assert_eq!(WINDOW_TITLE, format!("{IDENTIFIER}-siw"));
        assert_eq!(WINDOW_CLASS, "app.annota.desktop-sic");
        assert_eq!(WINDOW_TITLE, "app.annota.desktop-siw");
        assert_eq!(COPYDATA_KIND, 1543);
        assert_eq!(FORWARD_TIMEOUT_MS, 10_000);
    }

    #[test]
    fn the_plugin_is_pinned_exactly_and_without_its_semver_feature() {
        // With `semver` the plugin's names become `{identifier}_{version}-sic` and the fast path would miss.
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains("tauri-plugin-single-instance = \"=2.5.4\""));
    }

    #[test]
    fn the_identifier_is_the_one_in_tauri_conf() {
        let conf = include_str!("../tauri.conf.json");
        assert!(conf.contains(&format!("\"identifier\": \"{IDENTIFIER}\"")));
    }

    #[test]
    fn the_payload_is_cwd_then_each_argument_after_a_nul() {
        let bytes = encode_payload(r"C:\Users\m", &[r"C:\Annota\annota.exe", "--snap"]);
        assert_eq!(bytes, b"C:\\Users\\m\0C:\\Annota\\annota.exe\0--snap".to_vec());
    }

    #[test]
    fn the_payload_has_no_terminator_and_keeps_an_empty_last_argument() {
        assert_eq!(encode_payload(r"C:\", &["a.exe"]), b"C:\\\0a.exe".to_vec());
        assert_eq!(encode_payload(r"C:\", &["a.exe", ""]), b"C:\\\0a.exe\0".to_vec());
    }

    #[test]
    fn the_payload_is_utf8() {
        assert_eq!(encode_payload(r"C:\è", &["a.exe"]), "C:\\è\0a.exe".as_bytes().to_vec());
    }
}
