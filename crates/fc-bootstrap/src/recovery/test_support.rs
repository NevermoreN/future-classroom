//! Synthetic production-crypto fixture. No Windows system settings or real data.
use crate::backend::{Backend, ConfirmedRestore, Initialize};
use fc_application::identity::SessionLease;
use fc_domain::{ids::*, recovery::*, time::*};
use fc_ports::{Clock, MonotonicClock};
use fc_security::{CredentialRecordCodec, Password};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 10 * 3600).unwrap()
    }
}
impl MonotonicClock for FixedClock {
    fn monotonic_millis(&self) -> u64 {
        1_000
    }
}
pub(crate) fn password() -> Password {
    Password::new("Synthetic-recovery-R22-9274".into()).unwrap()
}
pub(crate) struct Fixture {
    pub root: PathBuf,
    pub backend: Arc<Mutex<Backend>>,
    pub lease: SessionLease,
    #[cfg(windows)]
    guard: Option<fc_windows::InstanceGuard>,
}
impl Fixture {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Round22-Bootstrap-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        #[cfg(not(windows))]
        std::fs::create_dir_all(root.join("state")).unwrap();
        #[cfg(windows)]
        let guard = {
            let paths = fc_windows::system::DataPaths::isolated_test(root.clone()).unwrap();
            paths
                .prepare_for_current_user()
                .unwrap()
                .read_back()
                .unwrap();
            Some(fc_windows::InstanceGuard::acquire(&root).unwrap())
        };
        let clock = Arc::new(FixedClock);
        let mut backend = Backend::open(
            root.join("state/classroom.sqlite3"),
            CredentialRecordCodec::from_key([119; 32]).unwrap(),
            clock.clone(),
            clock,
        )
        .unwrap();
        backend
            .initialize(
                Initialize {
                    school_name: "Synthetic recovery school".into(),
                    login_name: "admin".into(),
                    display_name: "Admin".into(),
                },
                password(),
            )
            .unwrap();
        let lease = backend.login("admin", password()).unwrap();
        Self {
            root,
            backend: Arc::new(Mutex::new(backend)),
            lease,
            #[cfg(windows)]
            guard,
        }
    }
    pub fn revision(&self) -> Revision {
        self.backend.lock().unwrap().global_revision().unwrap()
    }
    pub fn export(&self) -> (PathBuf, BackupManifest) {
        let revision = self.revision();
        let access = self
            .backend
            .lock()
            .unwrap()
            .ordinary_backup_access(self.lease, revision)
            .unwrap();
        let source = self.root.join("ordinary.sqlite3");
        let receipt = access
            .export_database(source.clone(), Arc::new(|| true))
            .unwrap()
            .wait()
            .unwrap();
        access.check_revision().unwrap();
        let bytes = std::fs::read(&source).unwrap();
        let mut entries = vec![BackupEntry::Database {
            bytes: bytes.len() as u64,
            sha256: fc_security::sha256(&bytes),
        }];
        entries.extend(receipt.media);
        (
            source,
            BackupManifest {
                format: BACKUP_FORMAT,
                school_id: receipt.school_id,
                installation_id: receipt.installation_id,
                revision: receipt.source_revision,
                entries,
            },
        )
    }
    pub fn confirm(&self) -> (PathBuf, ConfirmedRestore) {
        let (source, manifest) = self.export();
        let digest =
            fc_security::sha256(&fc_protocol::recovery::encode_manifest(&manifest).unwrap());
        let revision = self.revision();
        let access = self
            .backend
            .lock()
            .unwrap()
            .ordinary_backup_access(self.lease, revision)
            .unwrap();
        let preview = access
            .preflight_restore(
                source.clone(),
                self.root.join("preview.sqlite3"),
                manifest,
                digest,
                Arc::new(|| true),
            )
            .unwrap();
        let review = self
            .backend
            .lock()
            .unwrap()
            .review_preflighted_restore(self.lease, preview)
            .unwrap();
        (source, review.confirm(Arc::new(|| true)).unwrap())
    }
    #[cfg(windows)]
    pub fn reopen(&self, database: &std::path::Path) -> Backend {
        let clock = Arc::new(FixedClock);
        Backend::open(
            database,
            CredentialRecordCodec::from_key([119; 32]).unwrap(),
            clock.clone(),
            clock,
        )
        .unwrap()
    }
    pub fn prepare(&self) -> super::ColdRestorePermit {
        let (_, confirmed) = self.confirm();
        let pool = fc_runtime::worker::WorkerPool::<QuietTask>::start().unwrap();
        let audio = crate::voice_runtime::VoiceRuntime::new(Arc::new(QuietFactory));
        let mut drain =
            super::ColdShutdown::begin(confirmed, self.backend.clone(), pool, audio).unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            if let Some(drained) = drain.poll().unwrap() {
                return drained
                    .prepare_database(self.root.join("candidate.sqlite3"), Arc::new(|| true))
                    .unwrap();
            }
            assert!(std::time::Instant::now() < until);
            std::thread::yield_now();
        }
    }
}
pub(crate) struct QuietTask;
impl fc_runtime::worker::Task for QuietTask {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(0)
    }
    fn run(self, _: &fc_runtime::worker::JobContext) {}
}
pub(crate) struct QuietFactory;
struct QuietAudio;
impl crate::voice_runtime::RuntimeFactory for QuietFactory {
    fn invalidate_runtime(&self) {}
    fn create(
        &self,
        _: crate::voice_runtime::RuntimeControl,
    ) -> Result<Box<dyn crate::voice_runtime::AudioDomain>, crate::voice_runtime::RuntimeError>
    {
        Ok(Box::new(QuietAudio))
    }
}
impl crate::voice_runtime::AudioDomain for QuietAudio {
    fn interrupt(
        &mut self,
        _: crate::voice_runtime::StopReasons,
    ) -> Result<(), crate::voice_runtime::RuntimeError> {
        Ok(())
    }
    fn step(
        &mut self,
    ) -> Result<crate::voice_runtime::DomainProgress, crate::voice_runtime::RuntimeError> {
        Ok(crate::voice_runtime::DomainProgress::Idle)
    }
    fn has_native_ownership(&self) -> bool {
        false
    }
    fn resume_admission(&mut self) -> Result<(), crate::voice_runtime::RuntimeError> {
        Ok(())
    }
    fn finish(&mut self) -> Result<bool, crate::voice_runtime::RuntimeError> {
        Ok(true)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let closed = self
            .backend
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .close();
        #[cfg(windows)]
        self.guard.take();
        let cleaned = std::fs::remove_dir_all(&self.root);
        if !std::thread::panicking() {
            closed.unwrap();
            cleaned.unwrap();
        }
    }
}
