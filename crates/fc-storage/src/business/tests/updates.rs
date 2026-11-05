use super::*;
use fc_domain::time::{LocalDate, LocalDateTime};
use std::sync::atomic::AtomicU64;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Time;
impl Clock for Time {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::new(LocalDate::new(2026, 11, 6).unwrap(), 0).unwrap()
    }
}
fn actor(id: u64) -> ActorContext {
    ActorContext {
        principal_id: PrincipalId::new(id).unwrap(),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(LocalDate::new(2027, 1, 1).unwrap(), 0).unwrap(),
        origin: TrustedOrigin::Local,
    }
}
struct Publisher(Arc<AtomicUsize>);
impl AuthorityPublisher for Publisher {
    type Snapshot = Revision;
    type Update = ();
    type Error = ();
    fn snapshot_revision(&self, snapshot: &Revision) -> Revision {
        *snapshot
    }
    fn begin_update(&mut self) -> Result<(), ()> {
        self.0.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn publish(&mut self, _: (), _: Revision) -> Result<(), ()> {
        Ok(())
    }
    fn rollback(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn fault(&mut self) {}
}
fn fixture() -> (Directory, Arc<AtomicUsize>) {
    ISSUED.with(|issued| issued.borrow_mut().clear());
    let directory = Directory(std::env::temp_dir().join(format!(
        "fc-updates-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir(&directory.0).unwrap();
    // Exclusive, startup-only synthetic fixture setup before a writer exists.
    let mut db = open_connection(&directory.0.join("state.sqlite3")).unwrap();
    let transaction = db.transaction().unwrap();
    let school = SchoolId::new(1).unwrap();
    let identity = IdentityState {
        revision: Revision::new(1),
        installation: Some(Installation {
            school_id: school,
            school_name: "synthetic updates".into(),
            current_class: None,
        }),
        accounts: (1..=2)
            .map(|id| UserAccount {
                principal: Principal {
                    id: PrincipalId::new(id).unwrap(),
                    school_id: school,
                    role: if id == 1 {
                        Role::GlobalAdmin
                    } else {
                        Role::User
                    },
                    enabled: true,
                    auth_epoch: AuthEpoch::new(1),
                },
                login_name: format!("user{id}"),
                display_name: format!("User {id}"),
                must_change_password: false,
                credential_version: 1,
                recovery_version: 1,
            })
            .collect(),
        ..Default::default()
    };
    transaction
        .execute(
            "INSERT INTO documents VALUES(?1,?2,1)",
            params![IDENTITY, codec::bytes(&codec::identity(&identity)).unwrap()],
        )
        .unwrap();
    for id in 1..=2 {
        for kind in ["password", "recovery"] {
            transaction
                .execute(
                    "INSERT INTO documents VALUES(?1,?2,1)",
                    params![
                        credential_key(PrincipalId::new(id).unwrap(), kind, 1),
                        vec![1u8]
                    ],
                )
                .unwrap();
        }
    }
    transaction
        .execute("UPDATE state_meta SET revision=1", [])
        .unwrap();
    transaction.commit().unwrap();
    (directory, Arc::new(AtomicUsize::new(0)))
}
fn open_writer(directory: &Directory, published: &Arc<AtomicUsize>) -> StorageWriter<Revision> {
    StorageWriter::open(
        directory.0.join("state.sqlite3"),
        8,
        Publisher(published.clone()),
    )
    .unwrap()
}
fn repo(writer: &StorageWriter<Revision>) -> BusinessRepository<Revision> {
    BusinessRepository::new(
        writer.handle(),
        Arc::new(Time),
        Arc::new(|_, revision| Ok(revision)),
    )
    .with_session_guard(Arc::new(|_, _| Ok(())))
}
thread_local! {
    static ISSUED: std::cell::RefCell<std::collections::BTreeMap<u64, fc_ports::command_window::IssuedCommand>> = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}
trait IssuedChanges {
    fn change_update_issued(
        &self,
        request: UpdateChangeRequest,
        actor: &ActorContext,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError>;
}
impl IssuedChanges for BusinessRepository<Revision> {
    fn change_update_issued(
        &self,
        request: UpdateChangeRequest,
        actor: &ActorContext,
    ) -> Result<UpdateMaintenanceState, UpdateRepositoryError> {
        let id = request.command_id.get();
        let issued = ISSUED.with(|issued| {
            let mut issued = issued.borrow_mut();
            *issued.entry(id).or_insert_with(|| {
                let mut entropy = [1; 24];
                entropy[16..].copy_from_slice(&id.to_be_bytes());
                self.issue_command_window(crate::command_window::IssueCommand {
                    family: fc_ports::command_window::CommandFamily::Updates,
                    owner: fc_ports::command_window::CommandOwner::authenticated(actor),
                    entropy,
                })
                .unwrap()
            })
        });
        let mut scoped = self.with_command_window(issued);
        UpdateMaintenanceRepository::change_update(&mut scoped, request, actor)
    }
}
fn request(id: u64, revision: u64, mutation: UpdateMutation) -> UpdateChangeRequest {
    UpdateChangeRequest {
        command_id: CommandId::new(id).unwrap(),
        expected_revision: Revision::new(revision),
        mutation,
    }
}
fn joined_policy() -> UpdatePolicy {
    UpdatePolicy {
        group_id: Some([1; 16]),
        enabled: true,
        ..Default::default()
    }
}
fn release(sequence: u64) -> ReleaseIdentity {
    ReleaseIdentity {
        sequence: ReleaseSequence::new(sequence).unwrap(),
        payload_sha256: [sequence as u8; 32],
    }
}

#[test]
fn policy_codec_keeps_existing_mode_numbers_and_roundtrips_immediate_restart() {
    for (number, mode) in [
        (0, UpdateInstallMode::NotifyOnly),
        (1, UpdateInstallMode::ManualInstall),
        (2, UpdateInstallMode::MaintenanceWindow),
        (3, UpdateInstallMode::AutomaticAfterDownload),
    ] {
        let state = UpdateMaintenanceState {
            policy: UpdatePolicy {
                install_mode: mode,
                download: true,
                ..joined_policy()
            },
            ..Default::default()
        };
        let bytes = encode(&state).unwrap();
        let value = codec::value(&bytes).unwrap();
        assert_eq!(value["policy"]["mode"], json!(number));
        assert_eq!(decode(&bytes).unwrap(), state);
    }
    let disabled = UpdateMaintenanceState {
        policy: UpdatePolicy {
            enabled: false,
            download: false,
            share: false,
            install_mode: UpdateInstallMode::NotifyOnly,
            ..joined_policy()
        },
        ..Default::default()
    };
    assert_eq!(decode(&encode(&disabled).unwrap()).unwrap(), disabled);
    assert!(!disabled.policy.participation_enabled());
}

#[test]
fn default_updates_cannot_prepare_a_job_before_group_enrollment() {
    let (directory, published) = fixture();
    let writer = open_writer(&directory, &published);
    let repository = repo(&writer);
    assert!(repository.update_status(&actor(1)).unwrap().policy.enabled);
    assert_eq!(
        repository.change_update_issued(
            request(
                1,
                0,
                UpdateMutation::PrepareJob {
                    release: release(2)
                }
            ),
            &actor(1)
        ),
        Err(UpdateRepositoryError::Invalid)
    );
    assert_eq!(
        repository.update_status(&actor(1)).unwrap(),
        UpdateMaintenanceState::default()
    );
    writer.shutdown().unwrap();
}

#[test]
fn maintenance_cas_replay_and_cold_reopen_leave_teaching_authority_unchanged() {
    let (directory, published) = fixture();
    let writer = open_writer(&directory, &published);
    let repository = repo(&writer);
    assert_eq!(
        repository.update_status(&actor(1)).unwrap(),
        UpdateMaintenanceState::default()
    );
    let set = request(1, 0, UpdateMutation::SetPolicy(joined_policy()));
    let state = repository
        .change_update_issued(set.clone(), &actor(1))
        .unwrap();
    assert_eq!(state.revision, Revision::new(1));
    assert_eq!(
        repository.change_update_issued(set.clone(), &actor(1)),
        Ok(state.clone())
    );
    assert_eq!(
        repository.change_update_issued(
            request(2, 0, UpdateMutation::SetPolicy(joined_policy())),
            &actor(1)
        ),
        Err(UpdateRepositoryError::Stale)
    );
    let mut changed = set;
    if let UpdateMutation::SetPolicy(policy) = &mut changed.mutation {
        policy.channel = "beta".into();
    }
    assert_eq!(
        repository.change_update_issued(changed, &actor(1)),
        Err(UpdateRepositoryError::CommandIdReused)
    );
    let state = repository
        .change_update_issued(
            request(
                3,
                1,
                UpdateMutation::PrepareJob {
                    release: release(2),
                },
            ),
            &actor(1),
        )
        .unwrap();
    assert_eq!(state.jobs.len(), 1);
    assert_eq!(
        writer.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(1)
    );
    assert_eq!(published.load(Ordering::Acquire), 0);
    drop(repository);
    writer.shutdown().unwrap();
    let reopened = open_writer(&directory, &published);
    assert_eq!(repo(&reopened).update_status(&actor(1)), Ok(state));
    reopened.shutdown().unwrap();
}

#[test]
fn global_admin_live_session_and_execution_fences_guard_the_independent_writer_path() {
    let (directory, published) = fixture();
    let writer = open_writer(&directory, &published);
    let repository = repo(&writer);
    assert_eq!(
        repository.update_status(&actor(2)),
        Err(UpdateRepositoryError::Denied)
    );
    assert_eq!(
        repository.change_update_issued(
            request(999, 0, UpdateMutation::SetPolicy(joined_policy())),
            &actor(2)
        ),
        Err(UpdateRepositoryError::Denied)
    );
    let mut stale = actor(1);
    stale.auth_epoch = AuthEpoch::new(2);
    assert_eq!(
        repository.update_status(&stale),
        Err(UpdateRepositoryError::Denied)
    );
    let fenced = repo(&writer).with_execution_fence(Arc::new(|| false));
    assert_eq!(
        fenced.update_status(&actor(1)),
        Err(UpdateRepositoryError::Cancelled)
    );
    let checks = Arc::new(AtomicUsize::new(0));
    let count = checks.clone();
    let revoked = repo(&writer).with_session_guard(Arc::new(move |_, _| {
        if count.fetch_add(1, Ordering::AcqRel) == 0 {
            Ok(())
        } else {
            Err(StorageError::Identity(IdentityError::Permission(
                PermissionError::Expired,
            )))
        }
    }));
    assert_eq!(
        revoked.change_update_issued(
            request(1, 0, UpdateMutation::SetPolicy(joined_policy())),
            &actor(1)
        ),
        Err(UpdateRepositoryError::Denied)
    );
    assert_eq!(
        repository.update_status(&actor(1)).unwrap(),
        UpdateMaintenanceState::default()
    );
    writer.shutdown().unwrap();
}

#[test]
fn bounded_history_keeps_unsettled_jobs_and_evicts_only_settled_records() {
    let (directory, published) = fixture();
    let writer = open_writer(&directory, &published);
    let repository = repo(&writer);
    repository
        .change_update_issued(
            request(1, 0, UpdateMutation::SetPolicy(joined_policy())),
            &actor(1),
        )
        .unwrap();
    for id in 2..=33 {
        repository
            .change_update_issued(
                request(
                    id,
                    id - 1,
                    UpdateMutation::PrepareJob {
                        release: release(id),
                    },
                ),
                &actor(1),
            )
            .unwrap();
    }
    assert_eq!(
        repository.change_update_issued(
            request(
                34,
                33,
                UpdateMutation::PrepareJob {
                    release: release(34)
                }
            ),
            &actor(1)
        ),
        Err(UpdateRepositoryError::Capacity)
    );
    let state = repository
        .change_update_issued(
            request(
                35,
                33,
                UpdateMutation::AdvanceJob {
                    job_id: 1,
                    phase: UpdateJobPhase::Failed,
                },
            ),
            &actor(1),
        )
        .unwrap();
    assert_eq!(state.jobs.len(), 32);
    assert_eq!(
        repository.change_update_issued(
            request(2, 34, UpdateMutation::SetPolicy(state.policy.clone())),
            &actor(1)
        ),
        Err(UpdateRepositoryError::CommandIdReused)
    );
    let state = repository
        .change_update_issued(
            request(
                36,
                34,
                UpdateMutation::PrepareJob {
                    release: release(36),
                },
            ),
            &actor(1),
        )
        .unwrap();
    assert_eq!(state.jobs.len(), 32);
    assert_eq!(state.jobs[0].id, 2);
    assert_eq!(state.receipts.len(), 32);
    assert_eq!(
        writer.handle().try_revision().unwrap().wait().unwrap(),
        Revision::new(1)
    );
    assert_eq!(published.load(Ordering::Acquire), 0);
    writer.shutdown().unwrap();
}

#[test]
fn update_commands_require_actual_issuance_and_pruned_receipts_never_reexecute() {
    let (directory, published) = fixture();
    let writer = open_writer(&directory, &published);
    let mut repository = repo(&writer);
    assert_eq!(
        repository.change_update(
            request(1, 0, UpdateMutation::SetPolicy(joined_policy())),
            &actor(1)
        ),
        Err(UpdateRepositoryError::Invalid)
    );
    let mut state = repository
        .change_update_issued(
            request(1, 0, UpdateMutation::SetPolicy(joined_policy())),
            &actor(1),
        )
        .unwrap();
    for id in 2..=MAX_UPDATE_RECEIPTS as u64 + 2 {
        state = repository
            .change_update_issued(
                request(
                    id,
                    state.revision.get(),
                    UpdateMutation::SetPolicy(state.policy.clone()),
                ),
                &actor(1),
            )
            .unwrap();
    }
    assert!(state
        .receipts
        .iter()
        .all(|r| r.command_id != CommandId::new(1).unwrap()));
    assert_eq!(
        repository.change_update_issued(
            request(1, 0, UpdateMutation::SetPolicy(joined_policy())),
            &actor(1)
        ),
        Err(UpdateRepositoryError::CommandIdReused)
    );
    assert_eq!(repository.update_runtime_state().unwrap(), state);
    assert_eq!(published.load(Ordering::Acquire), 0);
    writer.shutdown().unwrap();
}

#[test]
fn background_updates_keep_release_floor_across_logout_failure_success_and_restart() {
    let (directory, published) = fixture();
    let writer = open_writer(&directory, &published);
    let repository = repo(&writer);
    let mut state = repository
        .change_update_issued(
            request(1, 0, UpdateMutation::SetPolicy(joined_policy())),
            &actor(1),
        )
        .unwrap();
    let mut background = repository.with_session_guard(Arc::new(|_, _| {
        Err(StorageError::Identity(IdentityError::Permission(
            PermissionError::Expired,
        )))
    }));
    state = background
        .accept_update_release(state.policy.revision, release(5))
        .unwrap();
    assert_eq!(state.accepted_release, Some(release(5)));
    assert_eq!(state.installed_release, None);
    let mut equivocation = release(5);
    equivocation.payload_sha256 = [99; 32];
    assert_eq!(
        background.accept_update_release(state.policy.revision, release(4)),
        Err(UpdateRepositoryError::Invalid)
    );
    assert_eq!(
        background.accept_update_release(state.policy.revision, equivocation),
        Err(UpdateRepositoryError::Invalid)
    );
    state = background
        .begin_background_update(state.revision, state.policy.revision, release(5))
        .unwrap();
    let job = state.jobs.last().unwrap().clone();
    state = background
        .advance_background_update(&job, UpdateJobPhase::Failed)
        .unwrap();
    assert_eq!(state.accepted_release, Some(release(5)));
    assert_eq!(state.installed_release, None);
    state = background
        .begin_background_update(state.revision, state.policy.revision, release(5))
        .unwrap();
    for phase in [
        UpdateJobPhase::Ready,
        UpdateJobPhase::Handoff,
        UpdateJobPhase::Installing,
        UpdateJobPhase::Succeeded,
    ] {
        let job = state.jobs.last().unwrap().clone();
        state = background.advance_background_update(&job, phase).unwrap();
    }
    assert_eq!(state.installed_release, Some(release(5)));
    assert_eq!(
        background.begin_background_update(state.revision, state.policy.revision, release(5)),
        Err(UpdateRepositoryError::Invalid)
    );
    drop(background);
    writer.shutdown().unwrap();
    let writer = open_writer(&directory, &published);
    let mut repository = repo(&writer);
    assert_eq!(repository.update_runtime_state().unwrap(), state);
    assert_eq!(
        repository.accept_update_release(state.policy.revision, release(4)),
        Err(UpdateRepositoryError::Invalid)
    );
    assert_eq!(published.load(Ordering::Acquire), 0);
    writer.shutdown().unwrap();
}

#[test]
fn policy_withdrawal_stops_new_progress_and_handoff_serializes_install_intent() {
    let (directory, published) = fixture();
    let writer = open_writer(&directory, &published);
    let mut repository = repo(&writer);
    let mut state = repository
        .change_update_issued(
            request(1, 0, UpdateMutation::SetPolicy(joined_policy())),
            &actor(1),
        )
        .unwrap();
    state = repository
        .begin_background_update(state.revision, state.policy.revision, release(2))
        .unwrap();
    let prepared = state.jobs.last().unwrap().clone();
    let mut disabled = state.policy.clone();
    disabled.enabled = false;
    disabled.download = false;
    disabled.share = false;
    state = repository
        .change_update_issued(
            request(2, state.revision.get(), UpdateMutation::SetPolicy(disabled)),
            &actor(1),
        )
        .unwrap();
    assert!(!state.policy.enabled);
    assert_eq!(
        repository.advance_background_update(&prepared, UpdateJobPhase::Downloading),
        Err(UpdateRepositoryError::Invalid)
    );
    state = repository
        .advance_background_update(&prepared, UpdateJobPhase::Cancelled)
        .unwrap();
    let mut enabled = state.policy.clone();
    enabled.enabled = true;
    enabled.download = true;
    enabled.share = true;
    state = repository
        .change_update_issued(
            request(3, state.revision.get(), UpdateMutation::SetPolicy(enabled)),
            &actor(1),
        )
        .unwrap();
    state = repository
        .begin_background_update(state.revision, state.policy.revision, release(3))
        .unwrap();
    for phase in [UpdateJobPhase::Ready, UpdateJobPhase::Handoff] {
        let job = state.jobs.last().unwrap().clone();
        state = repository.advance_background_update(&job, phase).unwrap();
    }
    for phase in [
        UpdateJobPhase::Handoff,
        UpdateJobPhase::Installing,
        UpdateJobPhase::NeedsReconcile,
    ] {
        if state.jobs.last().unwrap().phase != phase {
            let job = state.jobs.last().unwrap().clone();
            state = repository.advance_background_update(&job, phase).unwrap();
        }
        let mut disabled = state.policy.clone();
        disabled.enabled = false;
        disabled.download = false;
        disabled.share = false;
        assert_eq!(
            repository.change_update_issued(
                request(4, state.revision.get(), UpdateMutation::SetPolicy(disabled)),
                &actor(1)
            ),
            Err(UpdateRepositoryError::Busy)
        );
        assert_eq!(repository.update_runtime_state().unwrap(), state);
    }
    let reconciling = state.jobs.last().unwrap().clone();
    state = repository
        .advance_background_update(&reconciling, UpdateJobPhase::Succeeded)
        .unwrap();
    assert_eq!(state.installed_release, Some(release(3)));
    assert_eq!(state.accepted_release, Some(release(3)));
    let mut disabled = state.policy.clone();
    disabled.enabled = false;
    disabled.download = false;
    disabled.share = false;
    state = repository
        .change_update_issued(
            request(5, state.revision.get(), UpdateMutation::SetPolicy(disabled)),
            &actor(1),
        )
        .unwrap();
    assert!(!state.policy.enabled);
    assert_eq!(published.load(Ordering::Acquire), 0);
    writer.shutdown().unwrap();
}

#[test]
fn recovery_and_update_handoff_are_serialized_by_the_original_writer() {
    // Both orderings use real writer messages/transactions. No installer or
    // restore file replacement is simulated as completed by this test.
    for restore_barrier in [true, false] {
        let (directory, published) = fixture();
        let writer = open_writer(&directory, &published);
        let mut repository = repo(&writer);
        let mut state = repository
            .change_update_issued(
                request(1, 0, UpdateMutation::SetPolicy(joined_policy())),
                &actor(1),
            )
            .unwrap();
        state = repository
            .begin_background_update(state.revision, state.policy.revision, release(2))
            .unwrap();
        for phase in [UpdateJobPhase::Ready, UpdateJobPhase::Handoff] {
            let job = state.jobs.last().unwrap().clone();
            state = repository.advance_background_update(&job, phase).unwrap();
        }
        let barrier = |repository: &BusinessRepository<Revision>, id: u64| {
            let mut entropy = [2; 24];
            entropy[16..].copy_from_slice(&id.to_be_bytes());
            let issued = repository
                .issue_command_window(crate::command_window::IssueCommand {
                    family: fc_ports::command_window::CommandFamily::Generic,
                    owner: fc_ports::command_window::CommandOwner::authenticated(&actor(1)),
                    entropy,
                })
                .unwrap();
            repository
                .with_command_window(issued)
                .try_advance_recovery_barrier(Revision::new(1), &actor(1), Arc::new(|| true))
                .unwrap()
                .wait()
        };
        // Update wins: neither restore nor reset may erase/replace its job.
        assert!(matches!(
            barrier(&repository, 1001),
            Err(StorageError::Busy)
        ));
        assert!(matches!(
            repository.close_update_recovery_admission(),
            Err(StorageError::Busy)
        ));
        assert_eq!(repository.update_runtime_state().unwrap(), state);
        for phase in [UpdateJobPhase::NeedsReconcile, UpdateJobPhase::Failed] {
            let job = state.jobs.last().unwrap().clone();
            state = repository.advance_background_update(&job, phase).unwrap();
        }
        state = repository
            .begin_background_update(state.revision, state.policy.revision, release(3))
            .unwrap();
        let prepared = state.jobs.last().unwrap().clone();
        state = repository
            .advance_background_update(&prepared, UpdateJobPhase::Ready)
            .unwrap();
        let ready = state.jobs.last().unwrap().clone();
        // Recovery wins: a Ready update queued afterwards cannot enter Handoff,
        // even before the caller observes its barrier reply/sets SessionControl.
        if restore_barrier {
            assert!(barrier(&repository, 1002).is_ok());
        } else {
            repository.close_update_recovery_admission().unwrap();
        }
        assert_eq!(
            repository.advance_background_update(&ready, UpdateJobPhase::Handoff),
            Err(UpdateRepositoryError::Busy)
        );
        assert_eq!(
            repository.accept_update_release(state.policy.revision, release(4)),
            Err(UpdateRepositoryError::Busy)
        );
        // Original negative owner return remains possible after admission closes.
        let cancelled = repository
            .advance_background_update(&ready, UpdateJobPhase::Cancelled)
            .unwrap();
        assert_eq!(
            cancelled.jobs.last().unwrap().phase,
            UpdateJobPhase::Cancelled
        );
        writer.shutdown().unwrap();
    }
}
