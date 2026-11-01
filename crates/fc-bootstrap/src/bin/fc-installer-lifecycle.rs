//! GUI lifecycle helper/separately sealed Setup entry. Never starts the desktop,
//! initializes school data, accepts arbitrary paths, or wraps msiexec.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let code = fc_windows::installer_lifecycle::broker::run_entry(&arguments).unwrap_or(1603);
    // Preserve a settled native installation failure (for example1603), and
    // never report rollback/resource settlement as installation success.
    std::process::exit(code as i32);
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The installation lifecycle helper requires Windows.");
    std::process::exit(1603);
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn outer_setup_commands_have_no_lifecycle_authority() {
        for arguments in [
            vec!["--setup".into()],
            vec!["--run".into(), "msiexec.exe".into()],
            vec!["begin".into(), "effects_reconciled=true".into()],
        ] {
            assert!(fc_windows::installer_lifecycle::broker::run_helper(&arguments).is_err());
        }
    }
}
