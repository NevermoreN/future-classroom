//! Fixed HKCU Run value, with an explicit observed preimage and real readback.
//! Exact preimage, staged mutation and commit share one native transaction.
//! This is not an MSI/SQLite transaction or a universal registry isolation claim.
use fc_domain::autostart::{AutostartFailure, AutostartObservation};
use fc_ports::autostart::{AutostartSystem, AutostartSystemOutcome};
use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{w, Error, Result, HRESULT, HSTRING},
    Win32::{Foundation::*, System::Registry::*},
};

#[path = "autorun_transaction.rs"]
mod transaction;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
// Includes the terminating UTF-16 NUL. Never allocate from a registry-reported size.
const MAX_COMMAND_UNITS: usize = 260;
static NEXT_SCOPE: AtomicU64 = AtomicU64::new(1);

struct Key(HKEY);
impl Key {
    fn close(self) -> std::result::Result<(), HKEY> {
        let key = std::mem::ManuallyDrop::new(self);
        // SAFETY: transfer this exact owned key to checked close or retention.
        if unsafe { RegCloseKey(key.0) } == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(key.0)
        }
    }
}
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: exclusively owned successful registry open/create result.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

struct Descriptor {
    key: String,
    executable: PathBuf,
    command: String,
    scratch: bool,
}
/// Cloneable scope contains only owned text, never registry handles. Production
/// always addresses this project's fixed current-user Run value. Scratch scopes
/// are generated leaves, never caller-selected registry paths.
#[derive(Clone)]
pub struct RunScope(Arc<Descriptor>);
impl RunScope {
    pub fn current_executable() -> Result<Self> {
        let executable = std::env::current_exe().map_err(|_| Error::from_hresult(E_FAIL))?;
        Self::build(RUN_KEY.into(), executable, false)
    }
    fn build(key: String, executable: PathBuf, scratch: bool) -> Result<Self> {
        let command = command_for(&executable)?;
        Ok(Self(Arc::new(Descriptor {
            key,
            executable,
            command,
            scratch,
        })))
    }
    /// Isolated test scope. It cannot register a real login startup entry.
    /// Explicit cleanup is required after all physical worker calls have returned.
    pub fn scratch() -> Result<Self> {
        let executable = std::env::current_exe().map_err(|_| Error::from_hresult(E_FAIL))?;
        Self::scratch_for(executable)
    }
    fn scratch_for(executable: PathBuf) -> Result<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::from_hresult(E_FAIL))?
            .as_nanos();
        for _ in 0..16 {
            let sequence = NEXT_SCOPE.fetch_add(1, Ordering::Relaxed);
            let scope = Self::build(
                format!(
                    r"Software\FutureClassroom-TestRun-R21-{}-{nonce}-{sequence}",
                    std::process::id()
                ),
                executable.clone(),
                true,
            )?;
            let name = HSTRING::from(scope.0.key.as_str());
            let mut key = HKEY::default();
            let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
            // SAFETY: generated single leaf under HKCU Software. No production key.
            unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &name,
                    None,
                    None,
                    REG_OPTION_NON_VOLATILE,
                    KEY_QUERY_VALUE | KEY_SET_VALUE,
                    None,
                    &mut key,
                    Some(&mut disposition),
                )
                .ok()?;
            }
            drop(Key(key));
            if disposition == REG_CREATED_NEW_KEY {
                return Ok(scope);
            }
            // A collision is somebody else's key: neither use nor remove it.
        }
        Err(Error::from_hresult(E_UNEXPECTED))
    }
    pub fn open(&self) -> Result<RunAtLogin> {
        self.open_until(Instant::now() + Duration::from_secs(40))
    }
    /// The original shared-worker deadline, including time already queued.
    /// Synchronous registry/KTM calls cannot be forcibly interrupted by this fence.
    pub fn open_until(&self, deadline: Instant) -> Result<RunAtLogin> {
        if transaction::remaining_millis(deadline).is_none() {
            return Err(Error::from_hresult(HRESULT::from_win32(ERROR_TIMEOUT.0)));
        }
        Ok(RunAtLogin {
            scope: self.clone(),
            deadline,
            #[cfg(test)]
            fail_transaction_close: false,
            _thread: PhantomData,
        })
    }
    pub fn expected_command(&self) -> &str {
        &self.0.command
    }
    pub fn executable(&self) -> &Path {
        &self.0.executable
    }
    pub fn is_scratch(&self) -> bool {
        self.0.scratch
    }
    /// Never removes production Run keys. Caller must wait for all own workers.
    pub fn cleanup(&self) -> Result<()> {
        if !self.0.scratch {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let name = HSTRING::from(self.0.key.as_str());
        let mut key = HKEY::default();
        // SAFETY: generated test leaf; no arbitrary caller-selected path.
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &name, None, KEY_SET_VALUE, &mut key) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        status.ok()?;
        {
            let key = Key(key);
            // SAFETY: only our fixed value in the owned scratch leaf.
            let status = unsafe { RegDeleteValueW(key.0, w!("FutureClassroom")) };
            if status != ERROR_FILE_NOT_FOUND {
                status.ok()?;
            }
        }
        // SAFETY: RegDeleteKey does not recursively remove unexpected subkeys.
        let status = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &name) };
        if status != ERROR_FILE_NOT_FOUND {
            status.ok()?;
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct AutostartReadback {
    pub present: bool,
    pub matches_expected: bool,
    pub actual_command: Option<String>,
    pub observation: AutostartObservation,
}
/// Constructed on the worker. The Rc marker deliberately prevents Send/Sync;
/// every opened HKEY remains exclusively on the calling worker's stack.
pub struct RunAtLogin {
    scope: RunScope,
    deadline: Instant,
    #[cfg(test)]
    fail_transaction_close: bool,
    _thread: PhantomData<Rc<()>>,
}
impl RunAtLogin {
    pub fn current_executable() -> Result<Self> {
        RunScope::current_executable()?.open()
    }
    pub fn executable(&self) -> &Path {
        self.scope.executable()
    }
    pub fn expected_command(&self) -> &str {
        self.scope.expected_command()
    }
    pub fn read_back(&self) -> Result<AutostartReadback> {
        let observation = self.observe();
        let actual_command = match &observation {
            AutostartObservation::Command(command) => Some(command.clone()),
            _ => None,
        };
        Ok(AutostartReadback {
            present: matches!(
                observation,
                AutostartObservation::Command(_) | AutostartObservation::Opaque { .. }
            ),
            matches_expected: actual_command.as_deref() == Some(self.expected_command()),
            actual_command,
            observation,
        })
    }
    fn open_key(&self, write: bool, create: bool) -> std::result::Result<Option<Key>, WIN32_ERROR> {
        let name = HSTRING::from(self.scope.0.key.as_str());
        let mut key = HKEY::default();
        let access = KEY_QUERY_VALUE
            | if write {
                KEY_SET_VALUE
            } else {
                REG_SAM_FLAGS::default()
            };
        // SAFETY: fixed production or generated scratch path, owned output handle.
        let status = unsafe {
            if create {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &name,
                    None,
                    None,
                    REG_OPTION_NON_VOLATILE,
                    access,
                    None,
                    &mut key,
                    None,
                )
            } else {
                RegOpenKeyExW(HKEY_CURRENT_USER, &name, None, access, &mut key)
            }
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            return Err(status);
        }
        Ok(Some(Key(key)))
    }
    // Called only with our native mutation/observation permit, after any original
    // transaction has physically settled and its handles have actually closed.
    fn observe_owned(&self) -> AutostartObservation {
        match self.open_key(false, false) {
            Ok(None) => AutostartObservation::Absent,
            Ok(Some(key)) => {
                let observation = Self::read_key(&key);
                if let Err(key) = key.close() {
                    transaction::retain_key(key);
                    AutostartObservation::Unavailable(AutostartFailure::Interrupted)
                } else {
                    observation
                }
            }
            Err(error) => AutostartObservation::Unavailable(read_failure(error)),
        }
    }
    fn read_key(key: &Key) -> AutostartObservation {
        Self::read_handle(key.0)
    }
    fn read_handle(key: HKEY) -> AutostartObservation {
        let mut data = [0u8; MAX_COMMAND_UNITS * 2];
        let mut bytes = data.len() as u32;
        let mut kind = REG_VALUE_TYPE::default();
        // SAFETY: bounded stack buffer; never interpret it on ERROR_MORE_DATA.
        let status = unsafe {
            RegQueryValueExW(
                key,
                w!("FutureClassroom"),
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut bytes),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return AutostartObservation::Absent;
        }
        if status == ERROR_MORE_DATA {
            return AutostartObservation::Opaque {
                kind: kind.0,
                bytes,
            };
        }
        if status != ERROR_SUCCESS {
            return AutostartObservation::Unavailable(read_failure(status));
        }
        decode_value(kind.0, bytes, &data)
    }
}
impl AutostartSystem for RunAtLogin {
    fn target_command(&self) -> &str {
        self.expected_command()
    }
    fn observe(&self) -> AutostartObservation {
        // An ordinary observer must not abort another in-flight own transaction.
        let Some(_permit) = transaction::MutationPermit::acquire() else {
            return AutostartObservation::Unavailable(AutostartFailure::Unavailable);
        };
        self.observe_owned()
    }

    fn apply(
        &mut self,
        expected: &AutostartObservation,
        enabled: bool,
        fresh: &dyn Fn() -> bool,
    ) -> AutostartSystemOutcome {
        let finish = |failure, mutated| AutostartSystemOutcome {
            observation: self.observe(),
            failure,
            mutated,
        };
        if !expected.mutation_eligible() {
            return finish(Some(AutostartFailure::InvalidValue), false);
        }
        let fresh_now = || fresh() && transaction::remaining_millis(self.deadline).is_some();
        if !fresh_now() {
            return finish(Some(AutostartFailure::Cancelled), false);
        }
        // A retained native owner is a permanent negative gate, not a released
        // worker or a new permission. No Backend/SQL lock is held by this call.
        let Some(_mutation) = transaction::MutationPermit::acquire() else {
            return AutostartSystemOutcome {
                observation: AutostartObservation::Unavailable(AutostartFailure::Unavailable),
                failure: Some(AutostartFailure::Unavailable),
                mutated: false,
            };
        };
        let mut native = match transaction::NativeTransaction::begin(&_mutation, self.deadline) {
            Ok(native) => native,
            Err(failure) => {
                return AutostartSystemOutcome {
                    observation: AutostartObservation::Unavailable(failure),
                    failure: Some(failure),
                    mutated: false,
                }
            }
        };
        #[cfg(test)]
        if self.fail_transaction_close {
            native.fail_transaction_close_for_test();
        }
        let staged = (|| {
            native.open_key(&self.scope.0.key, enabled)?;
            let actual = native
                .key()
                .map(Self::read_handle)
                .unwrap_or(AutostartObservation::Absent);
            if &actual != expected {
                return Err(AutostartFailure::PreimageChanged);
            }
            if !fresh_now() {
                return Err(AutostartFailure::Cancelled);
            }
            let status = if enabled {
                let bytes: Vec<u8> = self
                    .expected_command()
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                // SAFETY: bounded REG_SZ in the same transaction-bound key.
                unsafe {
                    RegSetValueExW(
                        native.key().expect("enabled creates key"),
                        w!("FutureClassroom"),
                        None,
                        REG_SZ,
                        Some(&bytes),
                    )
                }
            } else if let Some(key) = native.key() {
                // SAFETY: one fixed value; the original preimage was read through
                // this same transacted HKEY. Never deletes a Run key/other value.
                unsafe { RegDeleteValueW(key, w!("FutureClassroom")) }
            } else {
                ERROR_FILE_NOT_FOUND
            };
            if status != ERROR_SUCCESS && !(status == ERROR_FILE_NOT_FOUND && !enabled) {
                return Err(write_failure(status));
            }
            // Microsoft requires bound handles to be closed before commit.
            // A failed close retains this exact key and transaction fail-closed.
            native.close_key()?;
            if !fresh_now() {
                return Err(AutostartFailure::Cancelled);
            }
            Ok(status == ERROR_SUCCESS)
        })();
        let mut failure = staged.as_ref().err().copied();
        if staged.is_ok() {
            failure = native.commit().err();
        }
        // Close/rollback and determine outcome BEFORE using an ordinary observer:
        // non-transacted access while the transaction is active can abort it.
        let settlement = native.settle();
        if !settlement.released {
            return AutostartSystemOutcome {
                observation: AutostartObservation::Unavailable(AutostartFailure::Interrupted),
                failure: Some(AutostartFailure::Interrupted),
                mutated: settlement.outcome == transaction::Outcome::Committed
                    && staged.unwrap_or(false),
            };
        }
        let committed = settlement.outcome == transaction::Outcome::Committed;
        let mutated = committed && staged.unwrap_or(false);
        if settlement.outcome == transaction::Outcome::Undetermined {
            failure = Some(AutostartFailure::Interrupted);
        } else if !committed && failure.is_none() {
            failure = Some(AutostartFailure::WriteFailed);
        }
        // Only this fourth freshness check is after the actual native commit.
        // Late cancellation never performs a compensating registry write.
        if committed && !fresh_now() {
            failure = Some(AutostartFailure::Cancelled);
        }
        if settlement.outcome == transaction::Outcome::Undetermined {
            // Unknown is not a factual claim of mutation. This observation forces
            // existing storage settlement into NeedsReconcile without inventing it.
            return AutostartSystemOutcome {
                observation: AutostartObservation::Unavailable(AutostartFailure::Interrupted),
                failure,
                mutated: false,
            };
        }
        let observation = self.observe_owned();
        if observation == AutostartObservation::Unavailable(AutostartFailure::Interrupted) {
            failure = Some(AutostartFailure::Interrupted);
        }
        let desired = if enabled {
            AutostartObservation::Command(self.expected_command().into())
        } else {
            AutostartObservation::Absent
        };
        if failure.is_none() && observation != desired {
            failure = Some(AutostartFailure::ReadbackMismatch);
        }
        AutostartSystemOutcome {
            observation,
            failure,
            mutated,
        }
    }
}
fn read_failure(error: WIN32_ERROR) -> AutostartFailure {
    if error == ERROR_ACCESS_DENIED {
        AutostartFailure::AccessDenied
    } else {
        AutostartFailure::ReadFailed
    }
}
fn write_failure(error: WIN32_ERROR) -> AutostartFailure {
    if error == ERROR_ACCESS_DENIED {
        AutostartFailure::AccessDenied
    } else {
        AutostartFailure::WriteFailed
    }
}
fn decode_value(kind: u32, bytes: u32, data: &[u8]) -> AutostartObservation {
    let opaque = || AutostartObservation::Opaque { kind, bytes };
    if kind != REG_SZ.0 || bytes < 2 || bytes as usize > data.len() || !bytes.is_multiple_of(2) {
        return opaque();
    }
    let mut units: Vec<u16> = data[..bytes as usize]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.pop() != Some(0) || units.contains(&0) {
        return opaque();
    }
    match String::from_utf16(&units) {
        Ok(command) => AutostartObservation::Command(command),
        Err(_) => opaque(),
    }
}
fn command_for(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    let raw = text.as_bytes();
    if !path.is_absolute()
        || raw.len() < 4
        || !raw[0].is_ascii_alphabetic()
        || raw[1] != b':'
        || raw[2] != b'\\'
        || text[2..].contains(':')
        || text.contains('/')
        || text.chars().any(|c| c == '"' || c.is_control())
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_none_or(|e| !e.eq_ignore_ascii_case("exe"))
        || text[3..]
            .split('\\')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let command = format!("\"{text}\" --autostart");
    if command.encode_utf16().count() >= MAX_COMMAND_UNITS
        || !fc_domain::autostart::valid_autostart_target(&command)
    {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    Ok(command)
}

#[cfg(test)]
#[path = "autorun_tests.rs"]
mod tests;
