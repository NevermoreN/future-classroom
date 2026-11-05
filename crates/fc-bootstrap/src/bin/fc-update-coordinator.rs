//! One current-user update handoff. Never opens teaching data or accepts paths.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]
#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [job] => fc_windows::update_install::run_handoff(job),
        _ => Err(fc_windows::update_install::UpdateInstallError::Invalid),
    };
    if result.is_err() {
        std::process::exit(1);
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("The update coordinator requires Windows.");
    std::process::exit(1);
}
