// Included inside backend::tests to share its existing real-writer synthetic
// account fixture. No bypass is added to the production public capability.
mod public_desktop_lesson {
    use super::*;
    use fc_domain::{lesson_context::LessonInitiator, schedule::*, schedule_document::*};
    use fc_ports::DesktopLessonBinding;

    struct Fixture {
        service: Joined,
        clock: Arc<TestClock>,
        _directory: Directory,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = Directory::new();
            let clock = Arc::new(TestClock::new());
            let mut service = open(&directory, clock.clone());
            policy_accounts(&mut service.0);
            let admin = policy_lease(&service.0, 1, TrustedOrigin::Local);
            service
                .0
                .identity(
                    admin,
                    IdentityCommand::BindCurrentClass {
                        id: ClassId::new(1).unwrap(),
                    },
                )
                .unwrap();
            let row = TimetableRow {
                id: RowId::new(1).unwrap(),
                logical_key: LogicalRowKey::new(1).unwrap(),
                group: DayGroup::Morning,
                kind: RowKind::Course,
                range: MinuteRange::new(Minute::new(0).unwrap(), Minute::new(120).unwrap())
                    .unwrap(),
                enabled_days: [true; 7],
                courses: std::array::from_fn(|_| {
                    Some(CourseCell {
                        subject_id: SubjectId::new(1).unwrap(),
                        teacher: TeacherBinding::Explicit(PrincipalId::new(2).unwrap()),
                    })
                }),
            };
            let document = ScheduleDocument {
                semesters: vec![SemesterPlan::new(
                    Semester {
                        id: SemesterId::new(1).unwrap(),
                        class_id: ClassId::new(1).unwrap(),
                        revision: Revision::default(),
                        enabled: true,
                        start: LocalDate::new(2026, 10, 1).unwrap(),
                        end: LocalDate::new(2026, 10, 31).unwrap(),
                        first_week_monday: LocalDate::new(2026, 9, 28).unwrap(),
                        tables: vec![WeekTable {
                            id: TableId::new(1).unwrap(),
                            role: TableRole::Single,
                            rows: vec![row],
                        }],
                    },
                    "Synthetic public desktop".into(),
                )],
                ..ScheduleDocument::default()
            };
            service
                .0
                .save_schedule(admin, Revision::default(), document)
                .unwrap();
            Self {
                service,
                clock,
                _directory: directory,
            }
        }
        fn stamp(&self, generation: u64) -> DesktopLessonStamp {
            let facts = self.service.0.desktop_facts(0).unwrap();
            let current = facts.lesson_context.current.as_ref().unwrap();
            DesktopLessonStamp {
                generation: SceneGeneration::new(generation),
                source_revision: facts.global_revision,
                binding: DesktopLessonBinding {
                    expected_revision: facts.lesson_context.revision,
                    expected_identity_revision: facts.identity.revision,
                    expected_schedule_revision: facts.schedule.revision,
                    lesson: current.key.clone(),
                },
                captured_at: facts.captured_at,
                valid_until: current.effective_until,
            }
        }
        fn flow(&self, generation: u64) -> DesktopLessonFlow {
            let stamp = self.stamp(generation);
            let control = self.service.0.session_control();
            assert!(control.publish_desktop_lesson_scene(stamp.clone()));
            control.begin_desktop_lesson_flow(&stamp).unwrap()
        }
    }
    fn cancelled<T>(result: Result<T>) {
        assert!(matches!(
            result,
            Err(BackendError::Storage(StorageError::ExecutionCancelled))
        ));
    }

    #[test]
    fn public_desktop_requires_exact_published_entry_and_only_one_cancellable_flow() {
        let fixture = Fixture::new();
        let control = fixture.service.0.session_control();
        let stamp = fixture.stamp(1);
        cancelled(control.begin_desktop_lesson_flow(&stamp));
        assert!(control.publish_desktop_lesson_scene(stamp.clone()));
        let mut wrong = stamp.clone();
        wrong.binding.expected_revision = Revision::new(999);
        cancelled(control.begin_desktop_lesson_flow(&wrong));
        let owner = control.begin_desktop_lesson_flow(&stamp).unwrap();
        let guard = owner.guard();
        assert!(guard.is_current());
        cancelled(control.begin_desktop_lesson_flow(&stamp));
        control.clear_desktop_lesson_scene();
        assert!(guard.is_current());
        let second = fixture.stamp(2);
        assert!(control.publish_desktop_lesson_scene(second.clone()));
        cancelled(control.begin_desktop_lesson_flow(&second));
        owner.cancel();
        assert!(!guard.is_current());
        let next = control.begin_desktop_lesson_flow(&second).unwrap();
        drop(owner); // The old owner cannot cancel a later exact flow.
        assert!(next.guard().is_current());
        drop(next);
        cancelled(fixture.service.0.read_desktop_lesson(&guard));
        assert!(!control.publish_desktop_lesson_scene(stamp));
    }

    #[test]
    fn public_desktop_os_callback_revokes_active_flow_and_old_scene_before_main_pump_cleanup() {
        let mut fixture = Fixture::new();
        let control = fixture.service.0.session_control();
        let original = fixture.stamp(1);
        assert!(control.publish_desktop_lesson_scene(original.clone()));
        let owner = control.begin_desktop_lesson_flow(&original).unwrap();
        let guard = owner.guard();
        let before = fixture.service.0.global_revision().unwrap();
        control.cancel_desktop_lesson_flow();
        // No owner.cancel/main-pump event or worker-ticket return was needed.
        assert!(!guard.is_current());
        cancelled(
            fixture
                .service
                .0
                .change_desktop_lesson(&guard, LessonAction::Restore),
        );
        cancelled(control.begin_desktop_lesson_flow(&original));
        assert!(!control.publish_desktop_lesson_scene(original));
        assert_eq!(fixture.service.0.global_revision().unwrap(), before);
        assert!(control.health_gate().check_sensitive().is_ok());
        let next = fixture.flow(2);
        drop(owner);
        assert!(next.guard().is_current());
    }

    #[test]
    fn public_desktop_os_callback_also_invalidates_unconsumed_published_entry() {
        let fixture = Fixture::new();
        let control = fixture.service.0.session_control();
        let original = fixture.stamp(1);
        assert!(control.publish_desktop_lesson_scene(original.clone()));
        control.cancel_desktop_lesson_flow();
        cancelled(control.begin_desktop_lesson_flow(&original));
        assert!(!control.publish_desktop_lesson_scene(original));
        let next = fixture.flow(2);
        assert!(fixture.service.0.read_desktop_lesson(&next.guard()).is_ok());
    }

    #[test]
    fn public_desktop_entry_rejects_stale_publication_and_uses_earlier_original_scene_end() {
        let fixture = Fixture::new();
        let control = fixture.service.0.session_control();
        let stamp = fixture.stamp(1);
        let mut stale = stamp.clone();
        stale.source_revision = Revision::default();
        assert!(!control.publish_desktop_lesson_scene(stale));
        let mut invalid = stamp.clone();
        invalid.generation = SceneGeneration::default();
        assert!(!control.publish_desktop_lesson_scene(invalid));
        let mut invalid = stamp.clone();
        invalid.valid_until = LocalDateTime::new(stamp.captured_at.date, 7201).unwrap();
        assert!(!control.publish_desktop_lesson_scene(invalid));
        let mut short = stamp;
        short.valid_until = LocalDateTime::new(
            short.captured_at.date,
            short.captured_at.second_of_day() + 30,
        )
        .unwrap();
        assert!(control.publish_desktop_lesson_scene(short.clone()));
        let owner = control.begin_desktop_lesson_flow(&short).unwrap();
        let guard = owner.guard();
        assert!(guard.allocation_bytes() <= 1024);
        fixture.clock.tick.store(29_999, Ordering::Release);
        assert!(guard.is_current());
        fixture.clock.tick.store(30_000, Ordering::Release);
        assert!(!guard.is_current());
    }

    #[test]
    fn public_desktop_without_login_survives_local_lock_and_commits_honest_origin_then_new_entry_restores(
    ) {
        let mut fixture = Fixture::new();
        let owner = fixture.flow(1);
        let guard = owner.guard();
        let admin = policy_lease(&fixture.service.0, 1, TrustedOrigin::Local);
        fixture.service.0.session_control().lock_local();
        assert!(fixture
            .service
            .0
            .check_session(admin, SessionPurpose::Manage)
            .is_err());
        let source = fixture.service.0.read_desktop_lesson(&guard).unwrap();
        assert_eq!(source.teachers.len(), 1);
        assert_eq!(source.teachers[0].id, PrincipalId::new(2).unwrap());
        let target = source.context.current.unwrap().base;
        let changed = fixture
            .service
            .0
            .change_desktop_lesson(&guard, LessonAction::Replace(target))
            .unwrap();
        assert_eq!(
            changed.context.active_override.as_ref().unwrap().initiator,
            LessonInitiator::LocalDesktop
        );
        assert!(guard.is_current()); // Own publication must not turn committed success into stale-source failure.
        assert!(matches!(
            fixture
                .service
                .0
                .change_desktop_lesson(&guard, LessonAction::Restore),
            Err(BackendError::Lesson(LessonRepositoryError::Domain(
                fc_domain::lesson_context::LessonContextError::RevisionConflict { .. }
            )))
        ));
        owner.cancel();
        let restore_owner = fixture.flow(2);
        let restored = fixture
            .service
            .0
            .change_desktop_lesson(&restore_owner.guard(), LessonAction::Restore)
            .unwrap();
        assert!(restored.context.active_override.is_none());
        assert!(restored.context.revision > changed.context.revision);
    }

    #[test]
    fn public_desktop_cancellation_and_original_budget_do_not_mutate_or_rebind() {
        let mut fixture = Fixture::new();
        let owner = fixture.flow(1);
        let guard = owner.guard();
        let before = fixture.service.0.global_revision().unwrap();
        owner.cancel();
        cancelled(
            fixture
                .service
                .0
                .change_desktop_lesson(&guard, LessonAction::Restore),
        );
        assert_eq!(fixture.service.0.global_revision().unwrap(), before);
        let next = fixture.flow(2);
        let next_guard = next.guard();
        fixture.clock.tick.store(119_999, Ordering::Release);
        assert!(next_guard.is_current());
        assert!(fixture.service.0.read_desktop_lesson(&next_guard).is_ok());
        fixture.clock.tick.store(120_000, Ordering::Release);
        cancelled(fixture.service.0.read_desktop_lesson(&next_guard));
        fixture.clock.tick.store(0, Ordering::Release);
        assert!(!next_guard.is_current());
        assert_eq!(fixture.service.0.global_revision().unwrap(), before);
    }

    #[test]
    fn public_desktop_monotonic_regression_and_health_failure_cannot_deliver_late_success() {
        let fixture = Fixture::new();
        let owner = fixture.flow(1);
        let guard = owner.guard();
        fixture.clock.tick.store(2, Ordering::Release);
        assert!(guard.is_current());
        fixture.clock.tick.store(1, Ordering::Release);
        assert!(!guard.is_current());
        fixture.clock.tick.store(3, Ordering::Release);
        assert!(!guard.is_current());
        let next = fixture.flow(2);
        let next_guard = next.guard();
        fixture.service.0.session_control().fault();
        assert!(!next_guard.is_current());
        assert!(fixture.service.0.read_desktop_lesson(&next_guard).is_err());
    }

    #[test]
    fn public_desktop_foreign_backend_and_request_execution_scope_are_not_authority() {
        let fixture = Fixture::new();
        let mut other = Fixture::new();
        let owner = fixture.flow(1);
        let guard = owner.guard();
        cancelled(other.service.0.read_desktop_lesson(&guard));
        assert!(guard.is_current());
        let other_owner = other.flow(1);
        let other_guard = other_owner.guard();
        let before = other.service.0.global_revision().unwrap();
        {
            let mut scope = other.service.0.begin_execution_scope(Arc::new(|| false));
            cancelled(scope.read_desktop_lesson(&other_guard));
            cancelled(scope.change_desktop_lesson(&other_guard, LessonAction::Restore));
        }
        assert!(other_guard.is_current());
        assert_eq!(other.service.0.global_revision().unwrap(), before);
        assert!(other.service.0.read_desktop_lesson(&other_guard).is_ok());
    }

    #[test]
    fn public_desktop_external_catalog_or_schedule_change_does_not_rebase_original_binding() {
        let mut fixture = Fixture::new();
        let owner = fixture.flow(1);
        let guard = owner.guard();
        let admin = policy_lease(&fixture.service.0, 1, TrustedOrigin::Local);
        fixture
            .service
            .0
            .identity(
                admin,
                IdentityCommand::RenameSchool {
                    name: "Changed catalog".into(),
                },
            )
            .unwrap();
        assert!(matches!(
            fixture.service.0.read_desktop_lesson(&guard),
            Err(BackendError::Lesson(LessonRepositoryError::Domain(
                fc_domain::lesson_context::LessonContextError::LessonContextChanged
            )))
        ));
        owner.cancel();
        let next = fixture.flow(2);
        let next_guard = next.guard();
        let schedule = fixture.service.0.read_schedule(admin).unwrap();
        fixture
            .service
            .0
            .save_schedule(admin, schedule.revision, schedule)
            .unwrap();
        assert!(matches!(
            fixture.service.0.read_desktop_lesson(&next_guard),
            Err(BackendError::Lesson(LessonRepositoryError::Domain(
                fc_domain::lesson_context::LessonContextError::LessonContextChanged
            )))
        ));
    }

    #[test]
    fn public_desktop_waiting_is_not_submit_authority_and_allows_own_real_publication_to_finish() {
        let mut fixture = Fixture::new();
        let owner = fixture.flow(1);
        let guard = owner.guard();
        assert!(guard.is_current());
        assert!(!guard.is_waiting_for_authority());
        let target = fixture
            .service
            .0
            .read_desktop_lesson(&guard)
            .unwrap()
            .context
            .current
            .unwrap()
            .base;
        let control = fixture.service.0.session_control();
        let checking = guard.clone();
        let observed = Arc::new(AtomicBool::new(false));
        let witness = observed.clone();
        {
            let mut scope = fixture.service.0.begin_execution_scope(Arc::new(move || {
                if control.health_gate().status().health
                    == fc_runtime::health::Health::AuthorityUpdating
                {
                    assert!(!checking.is_current());
                    assert!(checking.is_waiting_for_authority());
                    witness.store(true, Ordering::Release);
                }
                true
            }));
            assert_eq!(
                scope
                    .change_desktop_lesson(&guard, LessonAction::Replace(target))
                    .unwrap()
                    .context
                    .active_override
                    .unwrap()
                    .initiator,
                LessonInitiator::LocalDesktop
            );
        }
        assert!(observed.load(Ordering::Acquire));
        assert!(guard.is_current());
        assert!(!guard.is_waiting_for_authority());
    }

    #[test]
    fn public_desktop_waiting_never_defers_original_deadline_during_real_precommit_phase() {
        let mut fixture = Fixture::new();
        let owner = fixture.flow(1);
        let guard = owner.guard();
        let target = fixture
            .service
            .0
            .read_desktop_lesson(&guard)
            .unwrap()
            .context
            .current
            .unwrap()
            .base;
        let before = fixture.service.0.global_revision().unwrap();
        let control = fixture.service.0.session_control();
        let clock = fixture.clock.clone();
        let checking = guard.clone();
        let observed = Arc::new(AtomicBool::new(false));
        let witness = observed.clone();
        {
            let mut scope = fixture.service.0.begin_execution_scope(Arc::new(move || {
                if control.health_gate().status().health
                    == fc_runtime::health::Health::AuthorityUpdating
                {
                    assert!(checking.is_waiting_for_authority());
                    clock.tick.store(120_000, Ordering::Release);
                    assert!(!checking.is_waiting_for_authority());
                    witness.store(true, Ordering::Release);
                }
                true
            }));
            assert_eq!(
                scope.change_desktop_lesson(&guard, LessonAction::Replace(target)),
                Err(BackendError::Lesson(LessonRepositoryError::Unavailable))
            );
        }
        assert!(observed.load(Ordering::Acquire));
        assert!(!guard.is_current());
        assert!(!guard.is_waiting_for_authority());
        assert_eq!(fixture.service.0.global_revision().unwrap(), before);
        assert!(fixture
            .service
            .0
            .control
            .health_gate()
            .check_sensitive()
            .is_ok());
    }

    #[test]
    fn public_desktop_precommit_os_callback_rolls_back_without_fault_and_never_renews_capability() {
        let mut fixture = Fixture::new();
        let owner = Arc::new(fixture.flow(1));
        let guard = owner.guard();
        let source = fixture.service.0.read_desktop_lesson(&guard).unwrap();
        let target = source.context.current.unwrap().base;
        let before = fixture.service.0.global_revision().unwrap();
        let control = fixture.service.0.session_control();
        let checking = guard.clone();
        {
            let mut scope = fixture.service.0.begin_execution_scope(Arc::new(move || {
                if control.health_gate().status().health
                    == fc_runtime::health::Health::AuthorityUpdating
                {
                    assert!(checking.is_waiting_for_authority());
                    control.cancel_desktop_lesson_flow();
                    assert!(!checking.is_waiting_for_authority());
                }
                true
            }));
            assert_eq!(
                scope.change_desktop_lesson(&guard, LessonAction::Replace(target)),
                Err(BackendError::Lesson(LessonRepositoryError::Unavailable))
            );
        }
        assert!(!guard.is_current());
        assert_eq!(fixture.service.0.global_revision().unwrap(), before);
        assert!(fixture
            .service
            .0
            .control
            .health_gate()
            .check_sensitive()
            .is_ok());
        let admin = policy_lease(&fixture.service.0, 1, TrustedOrigin::Local);
        assert!(fixture
            .service
            .0
            .read_lesson(admin)
            .unwrap()
            .active_override
            .is_none());
    }

    #[test]
    fn public_desktop_cancel_after_real_commit_suppresses_delivery_but_does_not_claim_rollback() {
        let mut fixture = Fixture::new();
        let owner = Arc::new(fixture.flow(1));
        let guard = owner.guard();
        let target = fixture
            .service
            .0
            .read_desktop_lesson(&guard)
            .unwrap()
            .context
            .current
            .unwrap()
            .base;
        let before = fixture.service.0.global_revision().unwrap();
        let control = fixture.service.0.session_control();
        let cancel_owner = owner.clone();
        {
            let mut scope = fixture.service.0.begin_execution_scope(Arc::new(move || {
                let status = control.health_gate().status();
                if status.health == fc_runtime::health::Health::Healthy && status.revision > before
                {
                    cancel_owner.cancel();
                }
                true
            }));
            assert_eq!(
                scope.change_desktop_lesson(&guard, LessonAction::Replace(target)),
                Err(BackendError::Lesson(LessonRepositoryError::Unavailable))
            );
        }
        assert!(!guard.is_current());
        assert!(fixture.service.0.global_revision().unwrap() > before);
        let admin = policy_lease(&fixture.service.0, 1, TrustedOrigin::Local);
        assert_eq!(
            fixture
                .service
                .0
                .read_lesson(admin)
                .unwrap()
                .active_override
                .unwrap()
                .initiator,
            LessonInitiator::LocalDesktop
        );
    }
}
