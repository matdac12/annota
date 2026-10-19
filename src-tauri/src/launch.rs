//! What a command line asks Annota to do (spec §3.2 step 1).

/// The flags this app understands. Anything else, including the exe path, is ignored.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Launch {
    /// `--snap`: start a Snap in the Resident. Caps+S runs this.
    pub snap: bool,
}

impl Launch {
    pub fn parse<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut launch = Launch::default();
        for arg in args {
            if arg.as_ref() == "--snap" {
                launch.snap = true;
            }
        }
        launch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_is_not_a_snap() {
        assert_eq!(Launch::parse([r"C:\Annota\annota.exe"]), Launch { snap: false });
    }

    #[test]
    fn reads_snap_and_ignores_the_rest() {
        assert!(Launch::parse(["annota.exe", "--snap"]).snap);
        assert!(Launch::parse(["--snap"]).snap);
        assert_eq!(Launch::parse(["--verbose", "snap", "-snap", "--snapshot"]), Launch::default());
    }
}
