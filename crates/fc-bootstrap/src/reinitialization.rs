//! Physical close proof for explicit deployment reinitialization. This operation
//! grants no access to the old identity and never manufactures a restore barrier.
use crate::{
    backend::{Backend, BackendError},
    voice_runtime::{RuntimePhase, VoiceRuntime},
};
use fc_runtime::worker::{Task, WorkerPool};
#[cfg(any(windows, test))]
use std::sync::OnceLock;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReinitializationError {
    InvalidSequence,
    WrongDeployment,
    AlreadyOpened,
    AlreadyClaimed,
    RegistryFull,
    Unavailable,
    Backend(BackendError),
    Worker,
    Audio,
    AlreadySettled,
}
type Result<T> = std::result::Result<T, ReinitializationError>;
#[derive(Clone, PartialEq, Eq)]
struct RootIdentity {
    #[cfg(windows)]
    native: (u32, u64),
    #[cfg(not(windows))]
    canonical: PathBuf,
}
struct RootPin {
    identity: RootIdentity,
    #[cfg(windows)]
    _pin: fc_windows::materials::files::PinnedDirectory,
}
impl RootPin {
    fn open(root: &Path) -> Result<Self> {
        #[cfg(windows)]
        {
            let pin = fc_windows::materials::files::PinnedDirectory::open(root)
                .map_err(|_| ReinitializationError::Unavailable)?;
            let identity = pin
                .identity()
                .map_err(|_| ReinitializationError::Unavailable)?;
            Ok(Self {
                identity: RootIdentity { native: identity },
                _pin: pin,
            })
        }
        #[cfg(not(windows))]
        {
            let identity = root
                .canonicalize()
                .map_err(|_| ReinitializationError::Unavailable)?;
            if !identity.is_dir() {
                return Err(ReinitializationError::Unavailable);
            }
            Ok(Self {
                identity: RootIdentity {
                    canonical: identity,
                },
            })
        }
    }
}
#[derive(Clone, Copy)]
#[cfg(any(windows, test))]
enum Seen {
    Opened,
    Claimed,
}
#[cfg(any(windows, test))]
const MAX_ROOTS: usize = 4096;
#[cfg(any(windows, test))]
fn registry() -> &'static Mutex<Vec<(RootIdentity, Seen)>> {
    static REGISTRY: OnceLock<Mutex<Vec<(RootIdentity, Seen)>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}
#[cfg(any(windows, test))]
fn register(identity: RootIdentity, claim: bool) -> Result<()> {
    let mut registry = registry()
        .lock()
        .map_err(|_| ReinitializationError::Unavailable)?;
    if let Some((_, seen)) = registry.iter().find(|(key, _)| *key == identity) {
        return match (*seen, claim) {
            (Seen::Opened, false) => Ok(()),
            (Seen::Opened, true) => Err(ReinitializationError::AlreadyOpened),
            (Seen::Claimed, _) => Err(ReinitializationError::AlreadyClaimed),
        };
    }
    if registry.len() == MAX_ROOTS {
        return Err(ReinitializationError::RegistryFull);
    }
    registry.push((identity, if claim { Seen::Claimed } else { Seen::Opened }));
    Ok(())
}
fn database_root(database: &Path) -> Result<&Path> {
    let parent = database
        .parent()
        .ok_or(ReinitializationError::WrongDeployment)?;
    let production_name = database.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case("classroom.sqlite3")
    });
    let state_parent = parent
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("state"));
    if production_name && state_parent {
        parent
            .parent()
            .ok_or(ReinitializationError::WrongDeployment)
    } else {
        Ok(parent)
    }
}
/// Bootstrap's mandatory hook runs BEFORE any SQLite or writer creation. Entries
/// are never recycled; native file-ID reuse can only produce conservative denial.
#[cfg(any(windows, test))]
pub(crate) fn record_backend_open(database: &Path) -> Result<()> {
    let pin = RootPin::open(database_root(database)?)?;
    register(pin.identity.clone(), false)
}

/// Only the dedicated fresh-process repair bootstrap may claim this proof while
/// it owns InstanceGuard. There is no constructor taking an asserted boolean.
/// Its claim atomically prevents any later queued Backend::open in this process.
pub struct NeverOpenedProof {
    root: PathBuf,
    sequence: u64,
    pin: RootPin,
}
impl NeverOpenedProof {
    #[cfg(windows)]
    pub fn claim_repair_startup(
        paths: &fc_windows::system::DataPaths,
        sequence: u64,
    ) -> Result<Self> {
        paths
            .verify_root_for_current_user()
            .map_err(|_| ReinitializationError::Unavailable)?;
        Self::claim(paths.root(), sequence)
    }
    #[cfg(any(windows, test))]
    fn claim(root: &Path, sequence: u64) -> Result<Self> {
        if sequence == 0 {
            return Err(ReinitializationError::InvalidSequence);
        }
        let pin = RootPin::open(root)?;
        register(pin.identity.clone(), true)?;
        Ok(Self {
            root: root.to_owned(),
            sequence,
            pin,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}
pub enum ReinitializationBackend {
    Live(Arc<Mutex<Backend>>),
    NeverOpened(NeverOpenedProof),
}
/// A distinct capability for the reset adapter. No caller-supplied claim, auth
/// token, restore manifest or completed-cancellation flag can construct it.
pub struct ColdReinitializationPermit {
    root: PathBuf,
    sequence: u64,
    _pin: RootPin,
}
impl ColdReinitializationPermit {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}
pub struct ColdReinitialization<T: Task> {
    root: PathBuf,
    sequence: u64,
    pin: Option<RootPin>,
    backend: Option<ReinitializationBackend>,
    workers: Option<WorkerPool<T>>,
    audio: Option<VoiceRuntime>,
    error: Option<ReinitializationError>,
    retained: bool,
    close_failed: bool,
    settled: bool,
}
impl<T: Task> ColdReinitialization<T> {
    /// Independent final coordinator only. The caller continues holding its root
    /// InstanceGuard, has drained Shell/external IO, and retired every UI preview.
    /// Even an invalid binding retains and settles all supplied physical owners.
    pub fn begin(
        root: PathBuf,
        sequence: u64,
        backend: ReinitializationBackend,
        workers: WorkerPool<T>,
        audio: Option<VoiceRuntime>,
    ) -> Self {
        let pin = RootPin::open(&root);
        let mut error = pin.as_ref().err().cloned();
        if sequence == 0 {
            error = Some(ReinitializationError::InvalidSequence);
        }
        match &backend {
            ReinitializationBackend::NeverOpened(proof) => {
                if proof.sequence != sequence
                    || pin
                        .as_ref()
                        .is_ok_and(|pin| pin.identity != proof.pin.identity)
                {
                    error = Some(ReinitializationError::WrongDeployment);
                }
            }
            ReinitializationBackend::Live(backend) => match backend.lock() {
                Ok(owned) => {
                    let identity =
                        database_root(owned.recovery_database_path()).and_then(RootPin::open);
                    if identity.as_ref().is_err()
                        || pin.as_ref().is_ok_and(|pin| {
                            identity
                                .as_ref()
                                .is_ok_and(|bound| bound.identity != pin.identity)
                        })
                    {
                        error = Some(ReinitializationError::WrongDeployment);
                    }
                    if let Err(admission) = owned.close_reinitialization_admission() {
                        error = Some(ReinitializationError::Backend(admission));
                    }
                }
                Err(_) => error = Some(ReinitializationError::Backend(BackendError::Closed)),
            },
        }
        workers.request_stop();
        if let Some(audio) = &audio {
            audio.shutdown();
        }
        Self {
            root,
            sequence,
            pin: pin.ok(),
            backend: Some(backend),
            workers: Some(workers),
            audio,
            error,
            retained: false,
            close_failed: false,
            settled: false,
        }
    }
    pub fn is_retained(&self) -> bool {
        self.retained
    }
    /// None always retains these SAME owners. A failure is terminal only after
    /// physical worker/audio completion and an explicitly successful writer join.
    pub fn poll(&mut self) -> Result<Option<ColdReinitializationPermit>> {
        if self.settled {
            return Err(ReinitializationError::AlreadySettled);
        }
        if self.close_failed {
            return Ok(None);
        }
        if let Some(audio) = &self.audio {
            if audio.reap_nonblocking().is_err() {
                self.retained = true;
                self.error.get_or_insert(ReinitializationError::Audio);
            }
            let status = match audio.status() {
                Ok(status) => status,
                Err(_) => {
                    self.retained = true;
                    return Ok(None);
                }
            };
            if status.phase != RuntimePhase::Closed
                || status.physical_thread_running
                || status.ack_generation < status.stop_generation
            {
                self.retained |= status.faulted
                    || matches!(status.phase, RuntimePhase::Retained | RuntimePhase::Halted);
                return Ok(None);
            }
        }
        if self
            .workers
            .as_ref()
            .is_some_and(|workers| !workers.is_finished_with(crate::physical_thread::is_finished))
        {
            return Ok(None);
        }
        if let Some(workers) = self.workers.take() {
            if workers.join().is_err() {
                self.error.get_or_insert(ReinitializationError::Worker);
            }
        }
        if let Some(ReinitializationBackend::Live(backend)) = &self.backend {
            let Ok(mut backend) = backend.lock() else {
                self.retained = true;
                return Ok(None);
            };
            if backend.close().is_err() {
                self.retained = true;
                self.close_failed = true;
                return Ok(None);
            }
        }
        self.settled = true;
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        Ok(Some(ColdReinitializationPermit {
            root: self.root.clone(),
            sequence: self.sequence,
            _pin: self.pin.take().ok_or(ReinitializationError::Unavailable)?,
        }))
    }
}
impl<T: Task> Drop for ColdReinitialization<T> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // Abandonment or unwinding is not physical termination. Keep the native
        // handles/writer/pin owned for process lifetime; no caller can reuse an
        // apparently empty task slot and no destructor runs on the UI instead.
        if let Some(workers) = self.workers.take() {
            std::mem::forget(workers);
        }
        if let Some(audio) = self.audio.take() {
            std::mem::forget(audio);
        }
        if let Some(pin) = self.pin.take() {
            std::mem::forget(pin);
        }
        if let Some(backend) = self.backend.take() {
            std::mem::forget(backend);
        }
    }
}

#[cfg(test)]
mod tests;
