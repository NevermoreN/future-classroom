//! Real package/SQLite capabilities through the same physical workflow slot.
//! No production proof, native picker or credential state is fabricated.
use super::*;
use fc_domain::{identity::IdentityCommand, recovery::MigrationMapping};
use fc_storage::StorageError;
use std::sync::mpsc;

struct Pair {
    _source: Fixture,
    target: Fixture,
    workflow: RecoveryWorkflow,
    package: PathBuf,
}
fn access(fixture: &Fixture) -> OrdinaryBackupAccess {
    let revision = fixture.revision();
    fixture
        .backend
        .lock()
        .unwrap()
        .ordinary_backup_access(fixture.lease, revision)
        .unwrap()
}
impl Pair {
    fn new() -> Self {
        let source = Fixture::new();
        let target = Fixture::new();
        for fixture in [&source, &target] {
            let files =
                crate::profile_content_files::ControlledContentFiles::new(&fixture.root).unwrap();
            crate::profile_content_startup::reconcile_after_restart(
                &fixture.backend.lock().unwrap(),
                &files,
            )
            .unwrap();
        }
        let source_paths = DataPaths::isolated_test(source.root.clone()).unwrap();
        let source_workflow = RecoveryWorkflow::new(source_paths);
        let exported = source_workflow
            .export(access(&source), Arc::new(|| true))
            .unwrap();
        source_workflow.request_close();
        let package = target
            .root
            .join("quarantine")
            .join("migration-workflow.fcbak");
        std::fs::copy(exported.path, &package).unwrap();
        let workflow =
            RecoveryWorkflow::new(DataPaths::isolated_test(target.root.clone()).unwrap());
        Self {
            _source: source,
            target,
            workflow,
            package,
        }
    }
    fn inspect(&self) -> Result<MigrationBundle> {
        self.workflow.inspect_migration(
            SelectedFile::isolated_probe(&self.target.root, "migration-workflow.fcbak").unwrap(),
            access(&self.target),
            Arc::new(|| true),
        )
    }
    fn package_hash(&self) -> [u8; 32] {
        fc_security::sha256(&std::fs::read(&self.package).unwrap())
    }
}
impl Drop for Pair {
    fn drop(&mut self) {
        // Failures must not strand a self-retained slot with a heavy owner while
        // Fixture removes its synthetic directory. This destructor is test-only.
        self.workflow.request_close();
        if self.workflow.retirement_pending() {
            let workflow = self.workflow.clone();
            let _ = thread::spawn(move || workflow.retire_pending_on_worker()).join();
        }
    }
}
fn mapping(bundle: &MigrationBundle) -> MigrationMapping {
    let details = bundle.details();
    MigrationMapping {
        source_school_id: details.source_catalog.school_id,
        target_school_id: details
            .target_identity
            .installation
            .as_ref()
            .unwrap()
            .school_id,
        classes: details
            .source_catalog
            .classes
            .iter()
            .map(|v| (v.id, v.id))
            .collect(),
        subjects: details
            .source_catalog
            .subjects
            .iter()
            .map(|v| (v.id, v.id))
            .collect(),
        principals: details
            .source_catalog
            .principals
            .iter()
            .map(|v| (v.id, v.id))
            .collect(),
    }
}
fn retire(workflow: &RecoveryWorkflow) {
    let worker = workflow.clone();
    assert!(thread::spawn(move || worker.retire_pending_on_worker())
        .join()
        .unwrap());
}
fn assert_not_recovering(pair: &Pair) {
    assert!(!pair
        .target
        .backend
        .lock()
        .unwrap()
        .session_control()
        .is_recovering());
    assert!(pair.target.backend.lock().unwrap().state().is_ok());
}
struct MappingWorker {
    release: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<Result<ReviewBundle>>>,
}
impl MappingWorker {
    fn finish(mut self) -> Result<ReviewBundle> {
        self.release.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap()
    }
}
impl Drop for MappingWorker {
    fn drop(&mut self) {
        // A failed test assertion must still release and physically join the
        // held worker before Pair/Fixture attempts to remove its pinned root.
        self.release.store(true, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn migration_bundle_drop_only_retires_exact_source_and_blocks_reentry_until_worker_cleanup() {
    let pair = Pair::new();
    let original_package = pair.package_hash();
    let bundle = pair.inspect().unwrap();
    assert_ne!(
        bundle.details().source_installation,
        bundle.details().target_installation
    );
    let operation = operation_root(&pair.workflow);
    let ordinary = operation.join("ordinary.sqlite3");
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(&ordinary)
        .is_err());
    assert!(matches!(pair.inspect(), Err(WorkflowError::Busy)));
    drop(bundle); // Only a lightweight notification is dropped on this thread.
    assert!(operation.exists());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(&ordinary)
        .is_err());
    assert!(pair.workflow.retirement_pending());
    assert!(!pair.workflow.is_idle());
    assert!(matches!(pair.inspect(), Err(WorkflowError::Busy)));
    retire(&pair.workflow);
    assert!(!operation.exists());
    assert!(pair.workflow.is_idle());
    let next = pair.inspect().unwrap();
    assert_ne!(operation_root(&pair.workflow), operation);
    drop(next);
    retire(&pair.workflow);
    assert_eq!(pair.package_hash(), original_package);
    assert_not_recovering(&pair);
}

#[test]
fn changed_target_revision_cannot_rebless_old_mapping_but_fresh_review_can_be_retired() {
    let pair = Pair::new();
    let original_package = pair.package_hash();
    let bundle = pair.inspect().unwrap();
    let map = mapping(&bundle);
    let operation = operation_root(&pair.workflow);
    let old_revision = bundle.details().target_global_revision;
    pair.target
        .backend
        .lock()
        .unwrap()
        .identity(
            pair.target.lease,
            IdentityCommand::RenameSchool {
                name: "Synthetic target changed after migration inspection".into(),
            },
        )
        .unwrap();
    let changed_revision = pair.target.revision();
    assert!(changed_revision > old_revision);
    let result =
        pair.workflow
            .preflight_mapping(bundle, map, access(&pair.target), Arc::new(|| true));
    assert!(matches!(
        result,
        Err(WorkflowError::Backend(BackendError::Storage(
            StorageError::RecoveryConflict
        )))
    ));
    assert!(!operation.exists());
    assert!(pair.workflow.is_idle());
    assert_eq!(pair.target.revision(), changed_revision);
    assert_not_recovering(&pair);
    let fresh = pair.inspect().unwrap();
    let source_operation = operation_root(&pair.workflow);
    let map = mapping(&fresh);
    let review = pair
        .workflow
        .preflight_mapping(fresh, map, access(&pair.target), Arc::new(|| true))
        .unwrap();
    assert_eq!(review.summary().current_revision, changed_revision);
    assert!(review.summary().document_count > 0);
    let mapped_operation = operation_root(&pair.workflow);
    assert_ne!(source_operation, mapped_operation);
    assert!(source_operation.exists() && mapped_operation.exists());
    assert!(matches!(pair.inspect(), Err(WorkflowError::Busy)));
    drop(review);
    assert!(source_operation.exists() && mapped_operation.exists());
    assert!(pair.workflow.retirement_pending());
    retire(&pair.workflow);
    assert!(!source_operation.exists() && !mapped_operation.exists());
    assert_eq!(pair.target.revision(), changed_revision);
    assert_eq!(pair.package_hash(), original_package);
    assert_not_recovering(&pair);
}

#[test]
fn foreign_workflow_cannot_take_mapping_capability_or_release_its_owner_slot() {
    let pair = Pair::new();
    let bundle = pair.inspect().unwrap();
    let map = mapping(&bundle);
    let operation = operation_root(&pair.workflow);
    let other = RecoveryWorkflow::new(DataPaths::isolated_test(pair.target.root.clone()).unwrap());
    let result = other.preflight_mapping(bundle, map, access(&pair.target), Arc::new(|| true));
    assert!(matches!(result, Err(WorkflowError::Busy)));
    assert!(other.is_idle());
    assert!(!pair.workflow.is_idle());
    assert!(pair.workflow.retirement_pending());
    assert_eq!(operation_root(&pair.workflow), operation);
    assert!(operation.exists());
    assert!(matches!(pair.inspect(), Err(WorkflowError::Busy)));
    assert!(other.retire_pending_on_worker()); // Its empty slot owns no source.
    assert!(operation.exists());
    other.request_close();
    retire(&pair.workflow);
    assert!(!operation.exists());
    assert_not_recovering(&pair);
}

#[test]
fn closing_during_real_mapping_worker_prevents_late_publication_and_slot_reentry() {
    let pair = Pair::new();
    let bundle = pair.inspect().unwrap();
    let map = mapping(&bundle);
    let operation = operation_root(&pair.workflow);
    let worker_workflow = pair.workflow.clone();
    let access = access(&pair.target);
    let (worker_entered, entered) = mpsc::channel();
    let release = Arc::new(AtomicBool::new(false));
    let worker_release = release.clone();
    let once = AtomicBool::new(false);
    let fence: ExecutionFence = Arc::new(move || {
        if !once.swap(true, Ordering::AcqRel) {
            let _ = worker_entered.send(());
            while !worker_release.load(Ordering::Acquire) {
                thread::park_timeout(Duration::from_millis(2));
            }
        }
        true
    });
    let worker = MappingWorker {
        release,
        thread: Some(thread::spawn(move || {
            worker_workflow.preflight_mapping(bundle, map, access, fence)
        })),
    };
    entered.recv_timeout(Duration::from_secs(15)).unwrap(); // Worker owns payload.
    pair.workflow.request_close();
    assert!(!pair.workflow.is_idle());
    assert!(!pair.workflow.retirement_pending());
    assert!(!pair.workflow.retire_pending_on_worker());
    assert!(operation.exists());
    assert!(std::fs::OpenOptions::new()
        .write(true)
        .open(operation.join("ordinary.sqlite3"))
        .is_err());
    assert!(matches!(pair.inspect(), Err(WorkflowError::Busy)));
    assert!(matches!(
        worker.finish(),
        Err(WorkflowError::Files(RecoveryFileError::Cancelled)) | Err(WorkflowError::Cancelled)
    ));
    assert!(pair.workflow.is_idle());
    assert!(!pair.workflow.retirement_pending());
    assert!(!operation.exists());
    assert!(matches!(pair.inspect(), Err(WorkflowError::Busy))); // Closing is sticky.
    assert_not_recovering(&pair);
}
