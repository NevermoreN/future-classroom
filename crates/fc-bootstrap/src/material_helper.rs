//! Private self-executable PDF validation protocol. No UI, DB or file writes.
use fc_materials::office::{ContainerError, ValidationControl};
use std::{io::Read, path::Path, time::Duration};
struct Control;
impl ValidationControl for Control {
    fn canceled(&self) -> bool {
        false
    }
    fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
        fc_windows::system::current_thread_cpu_time().map_err(|_| ContainerError::ClockUnavailable)
    }
}
pub fn pdf(path: &Path) -> Result<(), String> {
    // No document access is possible until the parent installs its native Job
    // memory/CPU/descendant limits and explicitly releases this one-byte barrier.
    let mut permit = [0u8; 1];
    std::io::stdin()
        .read_exact(&mut permit)
        .map_err(|_| "helper admission missing")?;
    if permit != *b"G" {
        return Err("helper admission invalid".into());
    }
    let source = fc_windows::materials::files::LockedFile::open_helper(path, 50 * 1024 * 1024)
        .map_err(|_| "helper input unavailable")?;
    let data = source.map().map_err(|_| "helper mapping unavailable")?;
    let report = fc_materials::pdf_validation::validate_pdf(data, &Control)
        .map_err(|_| "PDF validation failed")?;
    let digest: String = report.sha256.iter().map(|v| format!("{v:02x}")).collect();
    println!(
        "PDF_VALIDATED {} {} {} {}",
        report.bytes, digest, report.pages, report.objects
    );
    Ok(())
}

/// Receiver for a temporary, private test ProgID. It has no UI and never opens
/// a third-party program. Production file associations never use this receiver.
pub fn open_probe_receiver(path: &Path) -> Result<(), String> {
    let directory = path.parent().ok_or("probe copy directory missing")?;
    let copies = directory.parent().ok_or("probe pool missing")?;
    let root = copies.parent().ok_or("probe deployment missing")?;
    if copies.file_name().and_then(|v| v.to_str()) != Some("launch-copies")
        || root.parent() != Some(std::env::temp_dir().as_path())
        || root
            .file_name()
            .and_then(|v| v.to_str())
            .is_none_or(|n| !n.starts_with("FutureClassroom-Test-"))
        || directory
            .file_name()
            .and_then(|v| v.to_str())
            .is_none_or(|n| n.is_empty() || !n.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err("probe receiver requires an owned isolated launch copy".into());
    }
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("probe name missing")?;
    fc_domain::materials::validate_material_name(name).map_err(|_| "probe name invalid")?;
    let source = fc_windows::materials::files::LockedFile::open(path, 50 * 1024 * 1024)
        .map_err(|_| "probe copy inaccessible")?;
    let files = fc_windows::materials::files::ControlledFiles::new(root)
        .map_err(|_| "probe deployment unavailable")?;
    let mut receipt = files
        .create_new(&directory.join(".future-classroom-open-receipt"))
        .map_err(|_| "probe receipt already exists or inaccessible")?;
    std::io::Write::write_all(receipt.file_mut(), b"LAUNCH_REQUEST_RECEIVED\n")
        .map_err(|_| "probe receipt failed")?;
    receipt.finish().map_err(|_| "probe receipt flush failed")?;
    drop(source);
    Ok(())
}
