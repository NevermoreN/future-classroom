//! Sealed Setup retains native MSI authority. Its outer maintenance UI can
//! restart the product after success; no arbitrary paths or MSI switches enter.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = verify_source(&arguments) {
        let _ = fc_windows::startup::show_startup_failure(&format!(
            "无法验证未来教室官方安装源。\n错误：{error}\n请重新获取完整官方安装包；已有设置和资料不会因此删除。"
        ));
        std::process::exit(1603);
    }
    let code =
        fc_windows::installer_lifecycle::broker::run_entry(&arguments).unwrap_or_else(|error| {
            use fc_windows::installer_lifecycle::LifecycleError;
            if arguments.is_empty()
                || matches!(arguments.as_slice(), [one] if one == "uninstall" || one == "maintenance")
            {
                let _ = fc_windows::startup::show_startup_failure(&format!(
                    "无法打开未来教室安装与维护界面。\n错误：{error:?}\n请保留此信息以便排查；已有设置和资料不会因此删除。"
                ));
            }
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

#[cfg(windows)]
fn verify_source(arguments: &[String]) -> Result<(), String> {
    use fc_windows::installer_lifecycle;
    let map = |error| format!("{error:?}");
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let outer = installer_lifecycle::standalone_archive()
        .map_err(map)?
        .is_some();
    let operation = arguments.is_empty()
        || matches!(arguments, [one] if matches!(one.as_str(), "install" | "repair" | "uninstall" | "maintenance"));
    if !outer && !operation {
        // Original native begin/broker/guardian roles retain their closed
        // pinned-image grammar and are never authorized by public metadata.
        return Ok(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs() as i64;
    let version = installer_lifecycle::maintenance_version().map_err(map)?;
    let directory = executable.parent().ok_or("安装源目录无效。")?;
    let archive = if outer {
        executable.clone()
    } else {
        directory.join("OriginalInstaller.exe")
    };
    if !outer && !archive.try_exists().map_err(|error| error.to_string())? {
        // The already released four-member coordinator cannot provide a v4
        // carrier. It retains its original compiled/native sealed-source path;
        // this does not admit a new unsealed source or a network executable.
        return Ok(());
    }
    let msi = directory.join("FutureClassroom.msi");
    fc_bootstrap::update_packages::verify_installer_distribution(
        &archive,
        include_bytes!("../../../../packaging/update-root.json"),
        now,
        version.as_deref(),
        (!outer).then_some((executable.as_path(), msi.as_path())),
    )
    .map_err(|error| error.to_string())?;
    Ok(())
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
