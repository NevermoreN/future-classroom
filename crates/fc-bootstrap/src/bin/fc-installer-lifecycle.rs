//! Sealed Setup retains native MSI authority. Its outer maintenance UI can
//! restart the product after success; no arbitrary paths or MSI switches enter.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let code =
        fc_windows::installer_lifecycle::broker::run_entry(&arguments).unwrap_or_else(|error| {
            use fc_windows::installer_lifecycle::LifecycleError;
            match error {
                LifecycleError::Busy | LifecycleError::GuardianRunning => 1618,
                LifecycleError::Pending | LifecycleError::Unsettled => 1604,
                LifecycleError::UpgradeProtocolUnsupported => 1638,
                LifecycleError::ShellIntegration => 5104,
                _ => 1603,
            }
        });
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
