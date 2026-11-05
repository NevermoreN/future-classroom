//! Same-user one-shot installation. A private DPAPI intent is written by the
//! running product before graceful shutdown. This helper never writes teaching
//! data, kills a process, calls msiexec, or bypasses sealed Setup admission.
use crate::{
    installer_lifecycle::{self as lifecycle, InstallIdentity, LifecycleGate},
    system::{CurrentUserDpapi, SecretBytes},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};
use windows::{
    core::{HSTRING, PWSTR},
    Win32::{
        Foundation::*, Storage::FileSystem::*, System::ApplicationInstallationAndServicing::*,
    },
};

#[derive(Debug)]
pub enum UpdateInstallError {
    Invalid,
    Busy,
    Native(lifecycle::LifecycleError),
    Windows(windows::core::Error),
    Io(std::io::Error),
    SetupFailed(u32),
    ReconcileRequired,
}
impl std::fmt::Display for UpdateInstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "update installation: {self:?}")
    }
}
impl std::error::Error for UpdateInstallError {}
impl From<std::io::Error> for UpdateInstallError {
    fn from(v: std::io::Error) -> Self {
        Self::Io(v)
    }
}
impl From<windows::core::Error> for UpdateInstallError {
    fn from(v: windows::core::Error) -> Self {
        Self::Windows(v)
    }
}
impl From<lifecycle::LifecycleError> for UpdateInstallError {
    fn from(v: lifecycle::LifecycleError) -> Self {
        Self::Native(v)
    }
}
type Result<T> = std::result::Result<T, UpdateInstallError>;

pub struct PreparedInstall {
    pub job_id: [u8; 16],
    pub policy_revision: u64,
    pub release_sequence: u64,
    pub setup_sha256: [u8; 32],
    pub msi_sha256: [u8; 32],
    pub ready_directory: PathBuf,
}
pub struct PreparedHandoff {
    pub job_id: [u8; 16],
    coordinator: PathBuf,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffState {
    Prepared,
    Installing,
    AwaitingProductConfirmation,
    Failed,
    Confirmed,
}
struct Record {
    sid: String,
    request: PreparedInstall,
    product_code: String,
    version: String,
    installed_exe_sha256: [u8; 32],
    state: HandoffState,
}

fn hex(id: [u8; 16]) -> String {
    id.iter().map(|v| format!("{v:02x}")).collect()
}
fn job_id(text: &str) -> Result<[u8; 16]> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    {
        return Err(UpdateInstallError::Invalid);
    }
    let mut result = [0; 16];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| UpdateInstallError::Invalid)?;
    }
    if result == [0; 16] {
        return Err(UpdateInstallError::Invalid);
    }
    Ok(result)
}
fn record_path(identity: &InstallIdentity, id: [u8; 16]) -> Result<PathBuf> {
    Ok(identity
        .update_directory()?
        .join(format!("{}.intent", hex(id))))
}
fn open_record(path: &Path, create: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(create)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    use std::os::windows::io::AsRawHandle;
    unsafe {
        GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info)?;
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0) != 0
        || info.nNumberOfLinks != 1
    {
        return Err(UpdateInstallError::Invalid);
    }
    Ok(file)
}
fn digest_file(path: &Path, expected: [u8; 32], limit: u64) -> Result<File> {
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    use std::os::windows::io::AsRawHandle;
    unsafe {
        GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info)?;
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0) != 0
        || info.nNumberOfLinks != 1
        || file.metadata()?.len() > limit
    {
        return Err(UpdateInstallError::Invalid);
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let got = file.read(&mut buffer)?;
        if got == 0 {
            break;
        }
        hash.update(&buffer[..got]);
    }
    if <[u8; 32]>::from(hash.finalize()) != expected {
        return Err(UpdateInstallError::Invalid);
    }
    Ok(file)
}

pub fn prepare_handoff(request: PreparedInstall) -> Result<PreparedHandoff> {
    if request.job_id == [0; 16] || request.policy_revision == 0 || request.release_sequence == 0 {
        return Err(UpdateInstallError::Invalid);
    }
    let installed = crate::system::DataPaths::installed_bootstrap_for_current_user()?;
    if std::env::current_exe()?.canonicalize()? != installed.canonicalize()? {
        return Err(UpdateInstallError::Invalid);
    }
    let facts = lifecycle::inspect_update_sources(&request.ready_directory)?;
    lifecycle::qualify_update_predecessor(&facts.predecessor_wire, &facts.predecessor)?;
    let _setup = digest_file(
        &request.ready_directory.join("Setup.exe"),
        request.setup_sha256,
        64 * 1024 * 1024,
    )?;
    let _msi = digest_file(
        &request.ready_directory.join("FutureClassroom.msi"),
        request.msi_sha256,
        64 * 1024 * 1024,
    )?;
    let installed_coordinator = installed
        .parent()
        .ok_or(UpdateInstallError::Invalid)?
        .join("fc-update-coordinator.exe");
    if !installed_coordinator.is_file() {
        return Err(UpdateInstallError::Invalid);
    }
    let id = request.job_id;
    let record = Record {
        sid: crate::system::current_user_sid()?,
        request,
        product_code: facts.product_code,
        version: facts.version,
        installed_exe_sha256: facts.installed_exe_sha256,
        state: HandoffState::Prepared,
    };
    let identity = InstallIdentity::for_current_user()?;
    let path = record_path(&identity, id)?;
    // Run a private copy: the installed coordinator itself must not remain an
    // executing image while MSI replaces the product files.
    let coordinator = path.with_extension("coordinator.exe");
    let mut source = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(&installed_coordinator)?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(&coordinator)?;
    let copied = std::io::copy(&mut source, &mut destination).and_then(|_| destination.sync_all());
    drop(destination);
    if let Err(error) = copied {
        let _ = std::fs::remove_file(&coordinator);
        return Err(error.into());
    }
    let mut file = match open_record(&path, true) {
        Ok(file) => file,
        Err(error) => {
            let _ = std::fs::remove_file(&coordinator);
            return Err(error);
        }
    };
    if let Err(error) = write_record(&mut file, &record) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&coordinator);
        return Err(error);
    }
    Ok(PreparedHandoff {
        job_id: id,
        coordinator,
    })
}

pub fn spawn_handoff(prepared: &PreparedHandoff) -> Result<Child> {
    // No paths, SID, result claims or installation switches cross this CLI.
    Ok(Command::new(&prepared.coordinator)
        .arg(hex(prepared.job_id))
        .creation_flags(0x08000000)
        .spawn()?)
}

pub fn run_handoff(text: &str) -> Result<()> {
    let id = job_id(text)?;
    let identity = InstallIdentity::for_current_user()?;
    let mut file = open_record(&record_path(&identity, id)?, false)?;
    let mut record = read_record(&mut file)?;
    if record.request.job_id != id || record.sid != crate::system::current_user_sid()? {
        return Err(UpdateInstallError::Invalid);
    }
    // An interrupted Installing task must be reconciled by the product, never
    // automatically replayed: installer exit/process death is not a receipt.
    if record.state != HandoffState::Prepared {
        return Err(UpdateInstallError::ReconcileRequired);
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match LifecycleGate::probe_quiescent(&identity) {
            Ok(()) => break,
            Err(lifecycle::LifecycleError::Busy) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(error) => return Err(error.into()),
        }
    }
    let result = run_install(&identity, &mut file, &mut record);
    if result.is_err() {
        // Restart only an independently verified intact predecessor after a
        // negative preflight or clean rollback. Pending/unknown effects block.
        if let Ok(facts) = lifecycle::inspect_update_sources(&record.request.ready_directory) {
            if LifecycleGate::probe_quiescent(&identity).is_ok()
                && lifecycle::qualify_update_predecessor(
                    &facts.predecessor_wire,
                    &facts.predecessor,
                )
                .is_ok()
            {
                let _ = restart_product();
            }
        }
    }
    result
}

/// Explicit same-user negative reconciliation. Never starts Setup, changes
/// teaching data or infers a successful install from a process disappearing.
pub fn reconcile_failed_handoff(text: &str) -> Result<()> {
    let id = job_id(text)?;
    let identity = InstallIdentity::for_current_user()?;
    let path = record_path(&identity, id)?;
    let mut file = open_record(&path, false)?;
    let mut record = read_record(&mut file)?;
    if record.sid != crate::system::current_user_sid()?
        || record.request.job_id != id
        || record.state != HandoffState::Installing
    {
        return Err(UpdateInstallError::ReconcileRequired);
    }
    let setup = record.request.ready_directory.join("Setup.exe");
    let _setup = digest_file(&setup, record.request.setup_sha256, 64 * 1024 * 1024)?;
    let _msi = digest_file(
        &record.request.ready_directory.join("FutureClassroom.msi"),
        record.request.msi_sha256,
        64 * 1024 * 1024,
    )?;
    let facts = lifecycle::inspect_update_sources(&record.request.ready_directory)?;
    if facts.product_code != record.product_code
        || facts.version != record.version
        || facts.installed_exe_sha256 != record.installed_exe_sha256
    {
        return Err(UpdateInstallError::Invalid);
    }
    lifecycle::reconcile_failed_update(
        &facts.predecessor_wire,
        &facts.predecessor,
        &facts.product_code,
        &setup,
        &path.with_extension("coordinator.exe"),
        || {
            record.state = HandoffState::Failed;
            write_record(&mut file, &record).map_err(|_| lifecycle::LifecycleError::Faulted)
        },
    )?;
    Ok(())
}

fn run_install(identity: &InstallIdentity, file: &mut File, record: &mut Record) -> Result<()> {
    let request = &record.request;
    let facts = lifecycle::inspect_update_sources(&request.ready_directory)?;
    if facts.product_code != record.product_code
        || facts.version != record.version
        || facts.installed_exe_sha256 != record.installed_exe_sha256
    {
        return Err(UpdateInstallError::Invalid);
    }
    lifecycle::qualify_update_predecessor(&facts.predecessor_wire, &facts.predecessor)?;
    let setup = request.ready_directory.join("Setup.exe");
    let _setup_pin = digest_file(&setup, request.setup_sha256, 64 * 1024 * 1024)?;
    let _msi_pin = digest_file(
        &request.ready_directory.join("FutureClassroom.msi"),
        request.msi_sha256,
        64 * 1024 * 1024,
    )?;
    record.state = HandoffState::Installing;
    write_record(file, record)?;
    // Fixed complete official Setup grammar. Native Setup independently checks
    // the exact predecessor, Pending, same SID, source seal and MSI settlement.
    let mut child = match Command::new(&setup)
        .arg("install")
        .current_dir(&record.request.ready_directory)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            // CreateProcess returned no child. This is a known negative result,
            // unlike an error waiting on an already-created original process.
            record.state = HandoffState::Failed;
            write_record(file, record)?;
            return Err(error.into());
        }
    };
    // A failed wait cannot establish installation failure or physical return;
    // preserve Installing for native reconciliation rather than replaying it.
    let status = child.wait()?;
    if !status.success() {
        record.state = HandoffState::Failed;
        write_record(file, record)?;
        return Err(UpdateInstallError::SetupFailed(
            status.code().unwrap_or(1603) as u32,
        ));
    }
    LifecycleGate::probe_quiescent(identity)?;
    verify_installed(record)?;
    record.state = HandoffState::AwaitingProductConfirmation;
    write_record(file, record)?;
    // The relaunched product owns DB migration/readback. This is deliberately
    // not Succeeded: its startup/version/class/data checks must confirm later.
    restart_product()
}

fn restart_product() -> Result<()> {
    let executable = crate::system::DataPaths::installed_bootstrap_for_current_user()?;
    Command::new(&executable)
        .arg("--autostart")
        .current_dir(executable.parent().ok_or(UpdateInstallError::Invalid)?)
        .creation_flags(0x08000000)
        .spawn()?;
    Ok(())
}

fn verify_installed(record: &Record) -> Result<()> {
    let product = HSTRING::from(format!("{{{}}}", record.product_code));
    let property = |name: &str| -> Result<String> {
        let mut bytes = [0u16; 1024];
        let mut len = bytes.len() as u32;
        let rc = unsafe {
            MsiGetProductInfoExW(
                &product,
                None,
                MSIINSTALLCONTEXT_USERUNMANAGED,
                &HSTRING::from(name),
                Some(PWSTR(bytes.as_mut_ptr())),
                Some(&mut len),
            )
        };
        if rc != 0 || len as usize >= bytes.len() {
            return Err(UpdateInstallError::Invalid);
        }
        String::from_utf16(&bytes[..len as usize]).map_err(|_| UpdateInstallError::Invalid)
    };
    if property("State")? != "5"
        || property("VersionString")? != record.version
        || !property("Transforms")?.is_empty()
    {
        return Err(UpdateInstallError::ReconcileRequired);
    }
    let exe = crate::system::DataPaths::installed_bootstrap_for_current_user()?;
    let _pin = digest_file(&exe, record.installed_exe_sha256, 64 * 1024 * 1024)?;
    Ok(())
}

fn write_record(file: &mut File, record: &Record) -> Result<()> {
    let mut bytes = b"FCUPI1\0\0".to_vec();
    bytes.push(match record.state {
        HandoffState::Prepared => 0,
        HandoffState::Installing => 1,
        HandoffState::AwaitingProductConfirmation => 2,
        HandoffState::Failed => 3,
        HandoffState::Confirmed => 4,
    });
    bytes.extend(record.request.job_id);
    bytes.extend(record.request.policy_revision.to_le_bytes());
    bytes.extend(record.request.release_sequence.to_le_bytes());
    bytes.extend(record.request.setup_sha256);
    bytes.extend(record.request.msi_sha256);
    bytes.extend(record.installed_exe_sha256);
    for text in [
        &record.sid,
        &record.product_code,
        &record.version,
        record
            .request
            .ready_directory
            .to_str()
            .ok_or(UpdateInstallError::Invalid)?,
    ] {
        if text.is_empty() || text.len() > 4096 || text.contains('\0') {
            return Err(UpdateInstallError::Invalid);
        }
        bytes.extend((text.len() as u32).to_le_bytes());
        bytes.extend(text.as_bytes());
    }
    let ciphertext = CurrentUserDpapi::default().protect(&SecretBytes::new(bytes)?)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&ciphertext)?;
    file.set_len(ciphertext.len() as u64)?;
    file.sync_all()?;
    Ok(())
}
fn read_record(file: &mut File) -> Result<Record> {
    if file.metadata()?.len() > 65536 {
        return Err(UpdateInstallError::Invalid);
    }
    let mut encrypted = Vec::new();
    file.read_to_end(&mut encrypted)?;
    let secret = CurrentUserDpapi::default().unprotect(&encrypted)?;
    let mut bytes = secret.expose();
    fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
        if bytes.len() < len {
            return Err(UpdateInstallError::Invalid);
        }
        let (head, tail) = bytes.split_at(len);
        *bytes = tail;
        Ok(head)
    }
    fn text(bytes: &mut &[u8]) -> Result<String> {
        let len = u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
        if len == 0 || len > 4096 {
            return Err(UpdateInstallError::Invalid);
        }
        String::from_utf8(take(bytes, len)?.to_vec()).map_err(|_| UpdateInstallError::Invalid)
    }
    if take(&mut bytes, 8)? != b"FCUPI1\0\0" {
        return Err(UpdateInstallError::Invalid);
    }
    let state = match take(&mut bytes, 1)?[0] {
        0 => HandoffState::Prepared,
        1 => HandoffState::Installing,
        2 => HandoffState::AwaitingProductConfirmation,
        3 => HandoffState::Failed,
        4 => HandoffState::Confirmed,
        _ => return Err(UpdateInstallError::Invalid),
    };
    let job_id = take(&mut bytes, 16)?.try_into().unwrap();
    let policy_revision = u64::from_le_bytes(take(&mut bytes, 8)?.try_into().unwrap());
    let release_sequence = u64::from_le_bytes(take(&mut bytes, 8)?.try_into().unwrap());
    let setup_sha256 = take(&mut bytes, 32)?.try_into().unwrap();
    let msi_sha256 = take(&mut bytes, 32)?.try_into().unwrap();
    let installed_exe_sha256 = take(&mut bytes, 32)?.try_into().unwrap();
    let sid = text(&mut bytes)?;
    let product_code = text(&mut bytes)?;
    let version = text(&mut bytes)?;
    let ready_directory = PathBuf::from(text(&mut bytes)?);
    if !bytes.is_empty()
        || policy_revision == 0
        || release_sequence == 0
        || !ready_directory.is_absolute()
    {
        return Err(UpdateInstallError::Invalid);
    }
    Ok(Record {
        sid,
        request: PreparedInstall {
            job_id,
            policy_revision,
            release_sequence,
            setup_sha256,
            msi_sha256,
            ready_directory,
        },
        product_code,
        version,
        installed_exe_sha256,
        state,
    })
}

#[derive(Debug)]
pub struct HandoffReceipt {
    pub job_id: [u8; 16],
    pub policy_revision: u64,
    pub release_sequence: u64,
    pub version: String,
    pub state: HandoffState,
}

/// The caller checks its healthy teaching-data/class startup before invoking
/// confirmation. This adapter independently rechecks the actual installed image
/// and MSI registration; it never opens or migrates the school database.
pub fn confirm_handoff(id: [u8; 16], sequence: u64, policy_revision: u64) -> Result<()> {
    let identity = InstallIdentity::for_current_user()?;
    let path = record_path(&identity, id)?;
    let mut file = open_record(&path, false)?;
    let mut record = read_record(&mut file)?;
    if record.sid != crate::system::current_user_sid()?
        || record.request.job_id != id
        || record.request.release_sequence != sequence
        || record.request.policy_revision != policy_revision
        || record.state != HandoffState::AwaitingProductConfirmation
    {
        return Err(UpdateInstallError::ReconcileRequired);
    }
    let installed = crate::system::DataPaths::installed_bootstrap_for_current_user()?;
    if std::env::current_exe()?.canonicalize()? != installed.canonicalize()? {
        return Err(UpdateInstallError::Invalid);
    }
    verify_installed(&record)?;
    record.state = HandoffState::Confirmed;
    write_record(&mut file, &record)?;
    // The copied helper normally exited before the new product initialized.
    // If it is still executing, leave its own bounded file for the next cleanup.
    let _ = std::fs::remove_file(path.with_extension("coordinator.exe"));
    Ok(())
}

/// Private maintenance receipts only. An in-use record remains owned by its
/// original coordinator and is omitted until that owner actually returns.
pub fn handoff_receipts() -> Result<Vec<HandoffReceipt>> {
    let identity = InstallIdentity::for_current_user()?;
    let directory = identity.update_directory()?;
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_some_and(|v| v == "intent") {
            if paths.len() == 32 {
                return Err(UpdateInstallError::Busy);
            }
            paths.push(path);
        }
    }
    paths.sort();
    let sid = crate::system::current_user_sid()?;
    let mut receipts = Vec::new();
    for path in paths {
        let Some(stem) = path.file_stem().and_then(|v| v.to_str()) else {
            return Err(UpdateInstallError::Invalid);
        };
        let id = job_id(stem)?;
        let mut file = match open_record(&path, false) {
            Ok(file) => file,
            Err(UpdateInstallError::Io(error))
                if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION.0 as i32) =>
            {
                continue
            }
            Err(error) => return Err(error),
        };
        let record = read_record(&mut file)?;
        if record.sid != sid || record.request.job_id != id {
            return Err(UpdateInstallError::Invalid);
        }
        receipts.push(HandoffReceipt {
            job_id: id,
            policy_revision: record.request.policy_revision,
            release_sequence: record.request.release_sequence,
            version: record.version,
            state: record.state,
        });
    }
    Ok(receipts)
}

/// Called only after the normal maintenance writer durably records success.
/// Unresolved/failed tasks cannot be evicted to make room for another attempt.
pub fn retire_confirmed_handoff(id: [u8; 16]) -> Result<()> {
    let identity = InstallIdentity::for_current_user()?;
    let path = record_path(&identity, id)?;
    let mut file = open_record(&path, false)?;
    let record = read_record(&mut file)?;
    if record.sid != crate::system::current_user_sid()?
        || record.request.job_id != id
        || record.state != HandoffState::Confirmed
    {
        return Err(UpdateInstallError::ReconcileRequired);
    }
    drop(file);
    match std::fs::remove_file(path.with_extension("coordinator.exe")) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    std::fs::remove_file(path)?;
    Ok(())
}

/// Cancel only before the one-shot owner has changed the durable state. A
/// running coordinator holds the record exclusively and cannot be cancelled by
/// deleting its intent underneath the original operation.
pub fn cancel_prepared_handoff(id: [u8; 16]) -> Result<()> {
    let identity = InstallIdentity::for_current_user()?;
    let path = record_path(&identity, id)?;
    let mut file = open_record(&path, false)?;
    let record = read_record(&mut file)?;
    if record.sid != crate::system::current_user_sid()?
        || record.request.job_id != id
        || record.state != HandoffState::Prepared
    {
        return Err(UpdateInstallError::ReconcileRequired);
    }
    drop(file);
    std::fs::remove_file(path.with_extension("coordinator.exe"))?;
    std::fs::remove_file(path)?;
    Ok(())
}

/// Retire a known failed helper receipt after the ordinary writer recorded its
/// outcome. A native Pending transaction still blocks retirement, and the
/// original coordinator's exclusive task lock must have physically returned.
pub fn retire_failed_handoff(id: [u8; 16]) -> Result<()> {
    let identity = InstallIdentity::for_current_user()?;
    let mut startup = lifecycle::LifecycleGate::acquire_startup(&identity)?;
    startup.try_release()?;
    let path = record_path(&identity, id)?;
    let mut file = open_record(&path, false)?;
    let record = read_record(&mut file)?;
    if record.sid != crate::system::current_user_sid()?
        || record.request.job_id != id
        || record.state != HandoffState::Failed
    {
        return Err(UpdateInstallError::ReconcileRequired);
    }
    drop(file);
    match std::fs::remove_file(path.with_extension("coordinator.exe")) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_sid_intent_is_dpapi_bound_and_exclusive() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let identity = InstallIdentity::for_current_user().unwrap();
        let mut id = [0x91; 16];
        id[..4].copy_from_slice(&std::process::id().to_le_bytes());
        let path = record_path(&identity, id).unwrap();
        struct Remove(PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _cleanup = Remove(path.clone());
        let record = Record {
            sid: crate::system::current_user_sid().unwrap(),
            request: PreparedInstall {
                job_id: id,
                policy_revision: 9,
                release_sequence: 45,
                setup_sha256: [1; 32],
                msi_sha256: [2; 32],
                ready_directory: PathBuf::from(r"C:\Fixture-Not-An-Installation"),
            },
            product_code: "99378807-098D-5F04-8D4D-F181DFA218D6".into(),
            version: "0.2.0".into(),
            installed_exe_sha256: [3; 32],
            state: HandoffState::Installing,
        };
        let mut file = open_record(&path, true).unwrap();
        write_record(&mut file, &record).unwrap();
        assert!(
            open_record(&path, false).is_err(),
            "original process retains exclusive record ownership"
        );
        file.seek(SeekFrom::Start(0)).unwrap();
        let read = read_record(&mut file).unwrap();
        assert_eq!(read.state, HandoffState::Installing);
        assert_eq!(read.request.job_id, id);
        assert_eq!(read.request.release_sequence, 45);
        file.seek(SeekFrom::Start(20)).unwrap();
        file.write_all(&[0x7d]).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        assert!(
            read_record(&mut file).is_err(),
            "modified DPAPI task cannot authorize a replay"
        );
        drop(file);
    }
}
