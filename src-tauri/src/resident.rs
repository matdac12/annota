//! Whether this process may become the Resident: Annota's own mutex and the elevation trap (spec §3.2
//! steps 3 and 4), copied from Traccia desktop's `resident.rs`.
//!
//! Hyperkey runs elevated, so an `annota.exe --snap` it starts is elevated too. When no Resident is
//! running, that process must not become it: it starts Annota again through the shell
//! (`explorer.exe <exe>`), which runs it unelevated and with no arguments, and exits. The `--snap` is not
//! forwarded; Caps+S is pressed again once the tray icon is there.
//!
//! "Is one running or starting?" is a named mutex every launch claims after the fast path misses, and the
//! Resident keeps for its whole life. It also covers the gap before the single-instance plugin's window
//! is up, where a second launch can't be forwarded and is dropped instead.

pub use sys::{claim, is_elevated, relaunch_through_shell};

/// What a launch does once the fast path has missed (no plugin window to forward to).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Become the Resident: build Tauri.
    Run,
    /// A Resident is starting (it holds the mutex but its plugin window isn't up): drop this launch.
    Exit,
    /// Elevated with nothing running: start Annota through the shell, unelevated, and exit.
    RelaunchThroughShell,
}

/// `first` is whether this launch got the mutex. Losing it wins over elevation: nothing is relaunched
/// while a Resident is starting.
pub fn start(first: bool, elevated: bool) -> Start {
    if !first {
        Start::Exit
    } else if elevated {
        Start::RelaunchThroughShell
    } else {
        Start::Run
    }
}

/// What waiting for the relaunched Annota found: the shell's own exit status, if it has ended, and whether
/// some process now holds the Resident's mutex.
///
/// The mutex alone is not proof: an `explorer.exe` that exits with an error refused the relaunch, and a
/// mutex someone holds after a refusal was claimed by some other launch.
#[cfg(windows)]
fn relaunch_verdict(shell: Option<std::process::ExitStatus>, claimed: bool) -> std::io::Result<()> {
    if let Some(refused) = shell.filter(|status| !status.success()) {
        return Err(std::io::Error::other(format!(
            "explorer.exe refused the relaunch (exit code {})",
            refused.code().unwrap_or(-1)
        )));
    }
    if !claimed {
        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "the shell did not start Annota"));
    }
    Ok(())
}

#[cfg(windows)]
mod sys {
    use std::io;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{
        CreateMutexW, GetCurrentProcess, OpenMutexW, OpenProcessToken, SYNCHRONIZATION_SYNCHRONIZE,
    };

    /// How long an elevated launch waits for the Annota it started through the shell to appear.
    const RELAUNCH_WAIT: Duration = Duration::from_secs(3);

    /// Per logon session, so another Windows user's Annota doesn't count.
    const NAME: PCWSTR = w!("Local\\annota-resident");

    /// This launch's hold on the mutex. Windows releases it when the process exits, which is exactly
    /// when the process should stop counting as running; `release` lets go earlier.
    pub struct Claim {
        /// Whether this is the first launch: no Resident is running or starting.
        pub first: bool,
        handle: Option<HANDLE>,
    }

    impl Claim {
        /// Stops counting as running, so an Annota this launch starts can take the claim.
        fn release(self) {
            if let Some(handle) = self.handle {
                // SAFETY: a handle this process opened and has not closed.
                let _ = unsafe { CloseHandle(handle) };
            }
        }
    }

    /// Opens the mutex, creating it if no other launch holds it.
    pub fn claim() -> Claim {
        // SAFETY: `NAME` is a NUL-terminated UTF-16 literal.
        let handle = unsafe { CreateMutexW(None, false, NAME) }.ok();
        // If the mutex can't be created at all, run anyway: the single-instance plugin still keeps a
        // second Resident out, and a silent exit would leave Annota never starting.
        // SAFETY: read right after the call that set it.
        let first = handle.is_none() || unsafe { GetLastError() } != ERROR_ALREADY_EXISTS;
        Claim { first, handle }
    }

    /// Whether some process (the Resident, or one starting) holds the mutex.
    fn someone_holds_it() -> bool {
        // SAFETY: as in `claim`; the handle is closed straight away.
        match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, NAME) } {
            Ok(handle) => {
                let _ = unsafe { CloseHandle(handle) };
                true
            }
            Err(_) => false,
        }
    }

    /// Whether this process runs elevated (an administrator's full token).
    pub fn is_elevated() -> bool {
        let mut token = HANDLE::default();
        // SAFETY: the pseudo-handle needs no closing; `token` is closed below once opened.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        // SAFETY: `elevation` is the struct TokenElevation fills, with its exact size.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            )
        }
        .is_ok();
        let _ = unsafe { CloseHandle(token) };
        ok && elevation.TokenIsElevated != 0
    }

    /// Starts this exe again through the shell, unelevated and with no arguments, and waits for it to
    /// take the claim. Errs when it doesn't appear, or when the shell itself refuses: Explorer isn't
    /// running, or it opened a folder window in a separate process and dropped the request.
    pub fn relaunch_through_shell(claim: Claim) -> io::Result<()> {
        // Let go first, so the new Annota becomes the first launch and not a second one.
        claim.release();
        // The full path, so an elevated launch never runs an `explorer.exe` planted next to the app.
        let windows = std::env::var_os("WINDIR")
            .map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        let mut shell = Command::new(windows.join("explorer.exe")).arg(std::env::current_exe()?).spawn()?;
        let start = Instant::now();
        let mut shell_ended = None;
        while start.elapsed() < RELAUNCH_WAIT {
            if someone_holds_it() {
                return super::relaunch_verdict(shell_ended, true);
            }
            // The shell that took the request leaves at once: its exit status says whether it accepted.
            shell_ended = shell_ended.or(shell.try_wait()?);
            std::thread::sleep(Duration::from_millis(50));
        }
        super::relaunch_verdict(shell_ended, false)
    }
}

#[cfg(not(windows))]
mod sys {
    use std::io;

    pub struct Claim {
        pub first: bool,
    }

    pub fn claim() -> Claim {
        Claim { first: true }
    }

    pub fn is_elevated() -> bool {
        false
    }

    pub fn relaunch_through_shell(_claim: Claim) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_elevated_first_launch_goes_through_the_shell() {
        assert_eq!(start(true, true), Start::RelaunchThroughShell);
    }

    #[test]
    fn an_unelevated_first_launch_becomes_the_resident() {
        assert_eq!(start(true, false), Start::Run);
    }

    #[test]
    fn a_launch_that_lost_the_mutex_is_dropped_elevated_or_not() {
        assert_eq!(start(false, false), Start::Exit);
        assert_eq!(start(false, true), Start::Exit);
    }

    #[cfg(windows)]
    mod relaunch {
        use super::*;
        use std::os::windows::process::ExitStatusExt;
        use std::process::ExitStatus;

        #[test]
        fn a_relaunch_that_is_up_and_whose_shell_did_not_complain_is_success() {
            assert!(relaunch_verdict(None, true).is_ok(), "the shell hands off and leaves");
            assert!(relaunch_verdict(Some(ExitStatus::from_raw(0)), true).is_ok());
        }

        #[test]
        fn a_relaunch_the_shell_refused_is_an_error_even_with_a_mutex_there() {
            let refused = Some(ExitStatus::from_raw(1));
            let error = relaunch_verdict(refused, true).unwrap_err();
            assert!(error.to_string().contains("refused"), "{error}");
            assert!(error.to_string().contains("exit code 1"), "{error}");
            assert!(relaunch_verdict(refused, false).is_err());
        }

        #[test]
        fn a_relaunch_nobody_took_is_a_timeout() {
            let error = relaunch_verdict(None, false).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        }
    }
}
