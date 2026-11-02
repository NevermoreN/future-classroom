//! Real SQLite Writer transactions and actual server-issued pairs. No mock
//! batch repository, direct table write or permission fallback is used.
use super::*;
use fc_ports::{
    command_window::{CommandWindowError, IssuedCommand, COMMAND_WINDOW_MILLIS},
    prepare_material_batch_append, MaterialBatchOutcomeRepository, MaterialBatchRepository,
    PreparedMaterialBatchAppend,
};
struct ResourcePreparation;
impl MaterialPreparation for ResourcePreparation {
    type Input = MaterialResource;
    fn prepare(&mut self, input: Self::Input) -> Result<MaterialResource, MaterialRepositoryError> {
        Ok(input)
    }
}
fn issue(f: &Fixture, label: &str, actor: &ActorContext) -> IssuedCommand {
    f.commands
        .issue(
            &f.handle(),
            CommandFamily::Materials,
            label,
            CommandOwner::authenticated(actor),
        )
        .unwrap()
}
fn proof(issued: IssuedCommand, scope: MaterialScope, ids: &[u64]) -> PreparedMaterialBatchAppend {
    proof_with_bytes(issued, scope, ids, 7)
}
fn proof_with_bytes(
    issued: IssuedCommand,
    scope: MaterialScope,
    ids: &[u64],
    bytes: u64,
) -> PreparedMaterialBatchAppend {
    prepare_material_batch_append(
        ids.iter()
            .map(|id| {
                fc_ports::prepare_material_append(
                    &mut ResourcePreparation,
                    MaterialResource {
                        version_id: ResourceVersionId::new(1000 + id).unwrap(),
                        sha256: [*id as u8; 32],
                        kind: MaterialKind::Txt,
                        bytes,
                    },
                    MaterialAppendMetadata {
                        command_id: issued.command_id,
                        scope,
                        material_id: MaterialId::new(*id).unwrap(),
                        display_name: format!("synthetic-{id}.txt"),
                        allowed_class_ids: vec![ClassId::new(1).unwrap()],
                    },
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
}
fn append_batch(
    f: &Fixture,
    issued: IssuedCommand,
    a: &ActorContext,
    ids: &[u64],
) -> Result<Vec<MaterialRecord>, MaterialRepositoryError> {
    f.repo(a.clone())
        .with_command_window(issued)
        .append_material_batch(proof(issued, scope(), ids), a, &Time)
}
fn batch_outcome(
    f: &Fixture,
    issued: IssuedCommand,
    a: &ActorContext,
) -> Result<Option<fc_ports::MaterialBatchCommandOutcome>, MaterialRepositoryError> {
    f.repo(a.clone())
        .with_command_window(issued)
        .material_batch_outcome(issued.command_id, a)
}
fn assert_no_batch_documents(f: &Fixture, issued: IssuedCommand, ids: &[u64]) {
    assert!(f
        .value(f.receipt_key(&key("batch-receipts", issued.command_id.get())))
        .is_none());
    for id in ids {
        assert!(f.value(key("items", *id)).is_none());
        assert!(f.value(key("resources", 1000 + id)).is_none());
    }
}
#[test]
fn twenty_confirmed_files_publish_once_in_order_and_replay_original_history_after_delete_restart() {
    let mut f = Fixture::new();
    let a = actor(2, TrustedOrigin::Local);
    let issued = issue(&f, "batch-twenty", &a);
    let ids = (1..=20).rev().collect::<Vec<_>>();
    let before = f.revision();
    let results = append_batch(&f, issued, &a, &ids).unwrap();
    assert_eq!(f.revision(), before.next().unwrap());
    assert_eq!(results.iter().map(|r| r.id.get()).collect::<Vec<_>>(), ids);
    assert_eq!(
        results.iter().map(|r| r.added_sequence).collect::<Vec<_>>(),
        (1..=20).collect::<Vec<_>>()
    );
    let folder =
        dto::read_folder(&f.value(key("folders", results[0].folder_id.get())).unwrap()).unwrap();
    assert_eq!(folder.revision, Revision::new(1));
    assert_eq!(folder.last_added_sequence, 20);
    assert_eq!(f.value(CATALOG.into()).unwrap()["bytes"], json!(140));
    let committed = f.revision();
    assert_eq!(append_batch(&f, issued, &a, &ids).unwrap(), results);
    assert_eq!(f.revision(), committed);
    let delete = issue(&f, "delete-from-batch", &a);
    f.repo(a.clone())
        .with_command_window(delete)
        .delete_material(
            MaterialDeleteRequest {
                command_id: delete.command_id,
                scope: scope(),
                material_id: results[0].id,
                expected_material_revision: results[0].revision,
            },
            &a,
            &Time,
        )
        .unwrap();
    let deleted_revision = f.revision();
    assert_eq!(
        batch_outcome(&f, issued, &a).unwrap().unwrap().records,
        results
    );
    assert_eq!(append_batch(&f, issued, &a, &ids).unwrap(), results);
    assert_eq!(f.revision(), deleted_revision);
    f.reopen();
    let mut renewed = a.clone();
    renewed.expires_at = LocalDateTime::new(now().date, 10800).unwrap();
    assert_eq!(
        batch_outcome(&f, issued, &renewed)
            .unwrap()
            .unwrap()
            .records,
        results
    );
    assert_eq!(append_batch(&f, issued, &renewed, &ids).unwrap(), results);
    assert_eq!(f.revision(), deleted_revision);
}
#[test]
fn late_item_collision_and_precommit_revocation_leave_all_batch_documents_absent() {
    let f = Fixture::new();
    let a = actor(2, TrustedOrigin::Web);
    let original = issue(&f, "existing", &a);
    append_batch(&f, original, &a, &[3]).unwrap();
    let collision = issue(&f, "late-collision", &a);
    let before = f.revision();
    assert_eq!(
        append_batch(&f, collision, &a, &[1, 2, 3]),
        Err(MaterialRepositoryError::Domain(
            MaterialError::AlreadyExists
        ))
    );
    assert_eq!(f.revision(), before);
    assert_no_batch_documents(&f, collision, &[1, 2]);
    let cancelled = issue(&f, "revoked-before-commit", &a);
    f.revoke.store(true, Ordering::Release);
    assert!(append_batch(&f, cancelled, &a, &[4, 5]).is_err());
    assert_eq!(f.revision(), before);
    assert_no_batch_documents(&f, cancelled, &[4, 5]);
    f.live.store(true, Ordering::Release);
    assert_eq!(batch_outcome(&f, cancelled, &a).unwrap(), None);
}
#[test]
fn batch_cannot_reuse_original_pair_for_reordered_payload_or_another_scope() {
    let f = Fixture::new();
    let a = actor(2, TrustedOrigin::Web);
    let issued = issue(&f, "ordered", &a);
    let results = append_batch(&f, issued, &a, &[1, 2]).unwrap();
    let before = f.revision();
    assert_eq!(
        append_batch(&f, issued, &a, &[2, 1]),
        Err(MaterialRepositoryError::IdempotencyConflict)
    );
    let other_scope = MaterialScope {
        teacher_id: principal(3),
        ..scope()
    };
    assert!(f
        .repo(a.clone())
        .with_command_window(issued)
        .append_material_batch(proof(issued, other_scope, &[3]), &a, &Time)
        .is_err());
    assert_eq!(f.revision(), before);
    assert_eq!(
        batch_outcome(&f, issued, &a).unwrap().unwrap().records,
        results
    );
    let other = issue(&f, "other-pair", &a);
    assert_eq!(
        f.repo(a.clone())
            .with_command_window(other)
            .material_batch_outcome(issued.command_id, &a),
        Err(MaterialRepositoryError::CommandWindow(
            CommandWindowError::NotIssued
        ))
    );
    let foreign = actor(3, TrustedOrigin::Web);
    assert_eq!(
        batch_outcome(&f, issued, &foreign),
        Err(MaterialRepositoryError::CommandWindow(
            CommandWindowError::NotIssued
        ))
    );
    let local = actor(2, TrustedOrigin::Local);
    assert_eq!(
        batch_outcome(&f, issued, &local),
        Err(MaterialRepositoryError::CommandWindow(
            CommandWindowError::NotIssued
        ))
    );
}
#[test]
fn single_and_batch_receipts_reject_same_pair_cross_kind_in_both_directions() {
    let f = Fixture::new();
    let a = actor(2, TrustedOrigin::Web);
    let batch = issue(&f, "first-batch", &a);
    let records = append_batch(&f, batch, &a, &[1, 2]).unwrap();
    let single_request = || {
        fc_ports::prepare_material_append(
            &mut Preparation,
            (),
            MaterialAppendMetadata {
                command_id: batch.command_id,
                scope: scope(),
                material_id: MaterialId::new(20).unwrap(),
                display_name: "synthetic.txt".into(),
                allowed_class_ids: vec![ClassId::new(1).unwrap()],
            },
        )
        .unwrap()
    };
    let before = f.revision();
    assert_eq!(
        f.repo(a.clone())
            .with_command_window(batch)
            .append_material(single_request(), &a, &Time),
        Err(MaterialRepositoryError::IdempotencyConflict)
    );
    assert_eq!(
        f.repo(a.clone())
            .with_command_window(batch)
            .material_outcome(batch.command_id, &a),
        Err(MaterialRepositoryError::IdempotencyConflict)
    );
    assert_eq!(f.revision(), before);
    assert_eq!(
        batch_outcome(&f, batch, &a).unwrap().unwrap().records,
        records
    );
    let single = f.command_id(CommandFamily::Materials, 900);
    super::append(&f, 900);
    let issued = f.commands.original(single).unwrap();
    let before = f.revision();
    assert_eq!(
        append_batch(&f, issued, &a, &[4, 5]),
        Err(MaterialRepositoryError::IdempotencyConflict)
    );
    assert_eq!(
        batch_outcome(&f, issued, &a),
        Err(MaterialRepositoryError::IdempotencyConflict)
    );
    assert_eq!(f.revision(), before);
    assert!(f.value(key("items", 4)).is_none());
}
#[test]
fn postcommit_publication_failure_preserves_full_batch_history_on_reopen() {
    let mut f = Fixture::new();
    let a = actor(2, TrustedOrigin::Web);
    let issued = issue(&f, "unknown-batch", &a);
    f.fail.store(true, Ordering::Release);
    assert_eq!(
        append_batch(&f, issued, &a, &[1, 2]),
        Err(MaterialRepositoryError::Faulted)
    );
    assert!(batch_outcome(&f, issued, &a).is_err());
    f.reopen();
    let result = batch_outcome(&f, issued, &a).unwrap().unwrap();
    let before = f.revision();
    assert_eq!(
        result
            .records
            .iter()
            .map(|r| r.id.get())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        append_batch(&f, issued, &a, &[1, 2]).unwrap(),
        result.records
    );
    assert_eq!(f.revision(), before);
}
struct WindowTime {
    wall: AtomicU64,
    mono: AtomicU64,
}
impl crate::command_window::WindowClock for WindowTime {
    fn wall_millis(&self) -> Result<u64, StorageError> {
        Ok(self.wall.load(Ordering::SeqCst))
    }
    fn monotonic_millis(&self) -> u64 {
        self.mono.load(Ordering::SeqCst)
    }
}
impl fc_ports::MonotonicClock for WindowTime {
    fn monotonic_millis(&self) -> u64 {
        self.mono.load(Ordering::SeqCst)
    }
}
#[test]
fn original_batch_expiry_cannot_execute_again_or_disclose_history() {
    let mut f = Fixture::new();
    f.writer.take().unwrap().shutdown().unwrap();
    let clock = Arc::new(WindowTime {
        wall: AtomicU64::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
                + 1000,
        ),
        mono: AtomicU64::new(0),
    });
    f.writer = Some(
        StorageWriter::open_with_clocks(
            f.directory.join("state.sqlite3"),
            8,
            Publisher {
                fail: f.fail.clone(),
                revoke: f.revoke.clone(),
                live: f.live.clone(),
            },
            clock.clone(),
            clock.clone(),
        )
        .unwrap(),
    );
    let a = actor(2, TrustedOrigin::Web);
    let issued = issue(&f, "expiry", &a);
    let results = append_batch(&f, issued, &a, &[1, 2]).unwrap();
    let before = f.revision();
    clock
        .wall
        .fetch_add(COMMAND_WINDOW_MILLIS + 1, Ordering::SeqCst);
    clock
        .mono
        .fetch_add(COMMAND_WINDOW_MILLIS + 1, Ordering::SeqCst);
    assert_eq!(
        batch_outcome(&f, issued, &a),
        Err(MaterialRepositoryError::CommandWindow(
            CommandWindowError::Expired
        ))
    );
    assert_eq!(
        append_batch(&f, issued, &a, &[1, 2]),
        Err(MaterialRepositoryError::CommandWindow(
            CommandWindowError::Expired
        ))
    );
    assert_eq!(f.revision(), before);
    assert_eq!(
        dto::read_record(&f.value(key("items", 1)).unwrap()).unwrap(),
        results[0]
    );
}
#[test]
fn ordinary_backup_omits_batch_receipts_and_restores_no_replay_authority() {
    let f = Fixture::new();
    let a = actor(2, TrustedOrigin::Local);
    let issued = issue(&f, "backup-batch", &a);
    let records = append_batch(&f, issued, &a, &[1, 2]).unwrap();
    let admin = actor(1, TrustedOrigin::Local);
    let repo = f.repo(admin.clone());
    repo.ensure_recovery_installation([9; 32]).unwrap();
    let destination = f.directory.join("ordinary.sqlite3");
    let backup = repo
        .try_export_ordinary(destination.clone(), &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(backup.media.len(), 2);
    let output = rusqlite::Connection::open(&destination).unwrap();
    let count: u64 = output
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE key LIKE 'business/material/v1/batch-receipts/%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    for record in records {
        let bytes: Vec<u8> = output
            .query_row(
                "SELECT value FROM documents WHERE key=?1",
                [key("items", record.id.get())],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            dto::read_record(&codec::value(&bytes).unwrap()).unwrap(),
            record
        );
    }
    drop(output);
    let before = f.revision();
    let barrier_pair = f
        .commands
        .issue(
            &f.handle(),
            CommandFamily::Generic,
            "backup-barrier",
            CommandOwner::authenticated(&admin),
        )
        .unwrap();
    let barrier = repo
        .with_command_window(barrier_pair)
        .try_advance_recovery_barrier(before, &admin, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let candidate = f.directory.join("candidate.sqlite3");
    repo.try_prepare_ordinary_restore(destination, candidate.clone(), barrier, Arc::new(|| true))
        .unwrap()
        .wait()
        .unwrap();
    let db = rusqlite::Connection::open(candidate).unwrap();
    assert_eq!(
        crate::business::command_window::owned(
            f.receipt_key(&key("batch-receipts", issued.command_id.get()))
                .as_str()
        )
        .map(|(_, ticket)| ticket),
        Some(issued.ticket)
    );
    assert!(crate::business::recovery::known_omitted(
        "business/material/v1/batch-receipts/00000000000000000001"
    ));
    assert!(crate::recovery::is_receipt(
        "business/material/v1/batch-receipts/00000000000000000001"
    ));
    let count: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE key LIKE 'business/material/v1/batch-receipts/%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(db);
    // The original live database's barrier also retired the old pair; no new
    // command is issued to reinterpret its old historical result as permission.
    assert_eq!(
        f.handle().try_check_command(issued).unwrap().wait(),
        Err(StorageError::CommandExpired)
    );
}

#[test]
fn full_batch_quota_refusal_does_not_admit_a_fitting_first_file() {
    let f = Fixture::new();
    let a = actor(2, TrustedOrigin::Local);
    for (label, ids) in [
        ("quota-first", (1..=20).collect::<Vec<_>>()),
        ("quota-second", (21..=39).collect::<Vec<_>>()),
    ] {
        let issued = issue(&f, label, &a);
        f.repo(a.clone())
            .with_command_window(issued)
            .append_material_batch(
                proof_with_bytes(issued, scope(), &ids, MAX_MATERIAL_BYTES),
                &a,
                &Time,
            )
            .unwrap();
    }
    assert_eq!(
        f.value(CATALOG.into()).unwrap()["bytes"],
        json!(39 * MAX_MATERIAL_BYTES)
    );
    let issued = issue(&f, "quota-rejected", &a);
    let before = f.revision();
    // 1950 MiB + the first 50 MiB fits, but both files exceed 2 GiB.
    // No member may enter any resource/folder/index/receipt namespace.
    assert_eq!(
        f.repo(a.clone())
            .with_command_window(issued)
            .append_material_batch(
                proof_with_bytes(issued, scope(), &[41, 42], MAX_MATERIAL_BYTES),
                &a,
                &Time
            ),
        Err(MaterialRepositoryError::QuotaExceeded)
    );
    assert_eq!(f.revision(), before);
    assert_no_batch_documents(&f, issued, &[41, 42]);
    assert_eq!(f.value(CATALOG.into()).unwrap()["records"], json!(39));
}

#[test]
fn low_global_revision_twenty_file_batch_remains_visible_in_real_desktop_facts_and_pages() {
    use fc_domain::{
        profile::{SubjectProfileDocument, SubjectProfileKey},
        schedule::{
            CourseCell, DayGroup, RowKind, Semester, TableRole, TeacherBinding, TimetableRow,
            WeekTable,
        },
        schedule_document::{ScheduleDocument, SemesterPlan},
        time::{Minute, MinuteRange},
    };
    use fc_ports::{
        DisplayRepository, ProfileRepository, ProfileRequest, ScheduleRepository, ScheduleRequest,
    };

    let f = Fixture::new();
    let admin = actor(1, TrustedOrigin::Local);
    let repo = f.repo(admin.clone());
    // The existing Fixture seeds its validated identity/classes/assignments
    // through an actual issued Generic Writer command. Timetable and profile
    // mutations below use their actual business ports and separate issuances.
    let row = TimetableRow {
        id: RowId::new(30).unwrap(),
        logical_key: LogicalRowKey::new(40).unwrap(),
        group: DayGroup::Morning,
        kind: RowKind::Course,
        range: MinuteRange::new(
            Minute::from_hm(0, 50).unwrap(),
            Minute::from_hm(1, 10).unwrap(),
        )
        .unwrap(),
        enabled_days: [true; 7],
        courses: std::array::from_fn(|_| {
            Some(CourseCell {
                subject_id: scope().subject_id,
                teacher: TeacherBinding::Explicit(scope().teacher_id),
            })
        }),
    };
    let candidate = ScheduleDocument {
        semesters: vec![SemesterPlan {
            semester: Semester {
                id: SemesterId::new(10).unwrap(),
                class_id: ClassId::new(1).unwrap(),
                revision: Revision::default(),
                enabled: true,
                start: LocalDate::new(2026, 10, 1).unwrap(),
                end: LocalDate::new(2026, 11, 30).unwrap(),
                first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
                tables: vec![WeekTable {
                    id: TableId::new(20).unwrap(),
                    role: TableRole::Single,
                    rows: vec![row],
                }],
            },
            name: "Synthetic batch classroom".into(),
            timing_sync: false,
            dual_history: None,
            row_details: vec![],
            cell_details: vec![],
        }],
        ..Default::default()
    };
    let schedule_command = f
        .commands
        .issue(
            &f.handle(),
            CommandFamily::Schedule,
            "batch-desktop-schedule",
            CommandOwner::authenticated(&admin),
        )
        .unwrap();
    repo.with_command_window(schedule_command)
        .execute_schedule(
            ScheduleRequest {
                command_id: schedule_command.command_id,
                expected_revision: Revision::default(),
                candidate,
            },
            &admin,
            &Time,
        )
        .unwrap();
    let baseline = repo.desktop_facts(0, &Time).unwrap();
    baseline.validate().unwrap();
    assert_eq!(
        baseline.materials_source.as_ref().unwrap().teacher_id,
        scope().teacher_id
    );
    assert_eq!(
        baseline.materials_source.as_ref().unwrap().subject_id,
        scope().subject_id
    );
    assert!(baseline.materials_page.is_none());

    let teacher = actor(2, TrustedOrigin::Local);
    let issued = issue(&f, "low-global-batch", &teacher);
    let ids = (1..=20).collect::<Vec<_>>();
    let before = f.revision();
    assert!(
        before.get() < 20,
        "fixture must expose folder/global revision mismatch"
    );
    let original = append_batch(&f, issued, &teacher, &ids).unwrap();
    let committed = f.revision();
    assert_eq!(committed, before.next().unwrap());
    assert_eq!(
        original
            .iter()
            .map(|r| r.added_sequence)
            .collect::<Vec<_>>(),
        (1..=20).collect::<Vec<_>>()
    );

    let profile_key = SubjectProfileKey {
        class_id: ClassId::new(1).unwrap(),
        subject_id: scope().subject_id,
        teacher_id: scope().teacher_id,
    };
    let mut profile = SubjectProfileDocument::new(ProfileId::new(500).unwrap(), profile_key);
    profile.folder_id = Some(original[0].folder_id);
    profile.materials.visible_rows = 5;
    let profile_command = f
        .commands
        .issue(
            &f.handle(),
            CommandFamily::Profiles,
            "batch-desktop-profile",
            CommandOwner::authenticated(&admin),
        )
        .unwrap();
    let saved = repo
        .with_command_window(profile_command)
        .execute_subject_profile(
            ProfileRequest {
                command_id: profile_command.command_id,
                key: profile_key,
                expected_revision: Revision::default(),
                candidate: profile,
            },
            &admin,
            &Time,
        )
        .unwrap();
    assert!(f.revision().get() < 20);
    let historical = batch_outcome(&f, issued, &teacher).unwrap().unwrap();
    assert_eq!(historical.records, original);
    let receipt_key = f.receipt_key(&key("batch-receipts", issued.command_id.get()));
    let receipt_before = f
        .handle()
        .try_get(receipt_key.clone())
        .unwrap()
        .wait()
        .unwrap()
        .unwrap()
        .value;
    let mut facts = repo.desktop_facts(0, &Time).unwrap();
    let mut visible = Vec::new();
    let mut page_count = 0;
    loop {
        // This is the original public Facts validator, not a locally fabricated
        // projection or a relaxed folder/global comparison.
        facts.validate().unwrap();
        assert!(facts.global_revision.get() < 20);
        assert_eq!(facts.profiles.subject_profile.as_ref(), Some(&saved));
        let page = facts.materials_page.as_ref().unwrap();
        assert!(
            page_count < 4,
            "desktop cursor must terminate after four pages"
        );
        assert_eq!(page.offset, visible.len() as u64);
        assert_eq!(page.folder.id, original[0].folder_id);
        assert_eq!(page.folder.revision, Revision::new(1));
        assert!(page.folder.revision <= facts.global_revision);
        assert_eq!(page.folder.last_added_sequence, 20);
        assert_eq!(page.total, 20);
        assert_eq!(page.class_id, Some(profile_key.class_id));
        assert_eq!(page.source, facts.materials_source);
        assert_eq!(page.items.len(), 5);
        visible.extend(page.items.iter().cloned());
        page_count += 1;
        let Some(cursor) = page.next_cursor.clone() else {
            break;
        };
        facts = repo
            .desktop_facts_with_material_cursor(0, Some(cursor), &Time)
            .unwrap();
    }
    assert_eq!(page_count, 4);
    assert_eq!(visible, original.iter().rev().cloned().collect::<Vec<_>>());
    let after_reads = f.revision();
    assert_eq!(
        batch_outcome(&f, issued, &teacher).unwrap(),
        Some(historical)
    );
    assert_eq!(append_batch(&f, issued, &teacher, &ids).unwrap(), original);
    assert_eq!(f.revision(), after_reads);
    assert_eq!(
        f.handle()
            .try_get(receipt_key)
            .unwrap()
            .wait()
            .unwrap()
            .unwrap()
            .value,
        receipt_before
    );
}
