use super::*;
use crate::{
    auth::{ActorContext, TrustedOrigin},
    identity::{IdentityCommand, IdentityState},
    ids::{AuthEpoch, ClassId, PrincipalId, SchoolId, TableId},
    schedule::{AcademicDay, DayMapping, Semester, TableRole, WeekTable},
    schedule_document::{DocumentErrorKind, ScheduleDocument, SemesterPlan},
};
fn date(year: i32, month: u8, day: u8) -> LocalDate {
    LocalDate::new(year, month, day).unwrap()
}
fn candidate(year: i32, days: &[(i32, u8, u8, PublicDayKind)]) -> CalendarCandidate {
    CalendarCandidate {
        source_year: year,
        fetched_at: LocalDateTime::new(date(2026, 10, 31), 36000).unwrap(),
        source_sha256: [42; 32],
        papers: vec!["https://www.gov.cn/zhengce/zhengceku/202511/content_7047091.htm".into()],
        days: days
            .iter()
            .map(|&(y, m, d, kind)| PublicCalendarDay {
                date: date(y, m, d),
                kind,
                name: "合成测试假日".into(),
            })
            .collect(),
        etag: Some("\"source-v1\"".into()),
        last_modified: None,
    }
}
fn rest() -> CalendarCandidate {
    candidate(2026, &[(2026, 10, 26, PublicDayKind::Rest)])
}
fn data() -> PublicCalendarData {
    PublicCalendarData {
        years: vec![rest()],
    }
}
fn manual(day: LocalDate, mapping: DayMapping) -> CalendarEntry {
    CalendarEntry {
        actual_semester_id: SemesterId::new(1).unwrap(),
        date: day,
        mapping,
    }
}
fn context() -> AutoAdoptionContext {
    AutoAdoptionContext {
        current_mode: CalendarMode::Public,
        captured_mode_revision: Revision::new(3),
        current_mode_revision: Revision::new(3),
        policy_authorized: true,
    }
}
fn document() -> ScheduleDocument {
    let mut doc = ScheduleDocument::default();
    doc.semesters.push(SemesterPlan::new(
        Semester {
            id: SemesterId::new(1).unwrap(),
            class_id: ClassId::new(1).unwrap(),
            revision: Revision::default(),
            enabled: true,
            start: date(2026, 10, 26),
            end: date(2026, 12, 31),
            first_week_monday: date(2026, 10, 26),
            tables: vec![WeekTable {
                id: TableId::new(1).unwrap(),
                role: TableRole::Single,
                rows: vec![],
            }],
        },
        "测试学期".into(),
    ));
    doc
}
fn resolve(doc: &ScheduleDocument, day: LocalDate) -> AcademicDay {
    doc.resolve_day(ClassId::new(1).unwrap(), day).unwrap()
}

#[test]
fn real_provider_notice_paths_and_fixed_provider_are_valid() {
    let c = rest();
    assert_eq!(c.validate(), Ok(()));
    assert_eq!(
        c.source_url(),
        "https://raw.githubusercontent.com/NateScarlet/holiday-cn/master/2026.json"
    );
    assert!(valid_notice_url(
        "https://www.gov.cn/zhengce/content/2023-10/25/content_6911527.htm"
    ));
    assert_eq!(PROVIDER_LICENSE, "MIT");
    assert_eq!(PROVIDER_VERSION, "HolidayCnV1");
}
#[test]
fn notice_url_rejects_credentials_cross_origin_query_fragment_and_path_tricks() {
    for url in [
        "http://www.gov.cn/zhengce/content/2026/content_1.htm",
        "https://www.gov.cn.evil.test/zhengce/content/2026/content_1.htm",
        "https://www.gov.cn@evil.test/zhengce/content/2026/content_1.htm",
        "https://evil.test@www.gov.cn/zhengce/content/2026/content_1.htm",
        "https://www.gov.cn:443/zhengce/content/2026/content_1.htm",
        "https://www.gov.cn/zhengce/content//content_1.htm",
        "https://www.gov.cn/zhengce/content/../content_1.htm",
        "https://www.gov.cn/zhengce/content/%2e%2e/content_1.htm",
        "https://www.gov.cn/zhengce/content/2026/content_1.htm?x=1",
        "https://www.gov.cn/zhengce/content/2026/content_1.htm#x",
        "https://www.gov.cn/zhengce/content/2026\\content_1.htm",
        "https://www.gov.cn/zhengce/content/2026/content_1.json",
    ] {
        assert!(!valid_notice_url(url), "accepted {url}");
    }
}
#[test]
fn empty_days_and_empty_or_duplicate_papers_cannot_verify_year() {
    let mut c = rest();
    c.days.clear();
    assert_eq!(c.validate(), Err(CalendarError::EmptyYear));
    c = rest();
    c.papers.clear();
    assert_eq!(c.validate(), Err(CalendarError::InvalidSource));
    c = rest();
    c.papers.push(c.papers[0].clone());
    assert_eq!(c.validate(), Err(CalendarError::InvalidSource));
}
#[test]
fn year_range_and_duplicate_dates_are_rejected() {
    let mut c = rest();
    c.source_year = 0;
    assert_eq!(c.validate(), Err(CalendarError::InvalidYear));
    c.source_year = 10000;
    assert_eq!(c.validate(), Err(CalendarError::InvalidYear));
    c = rest();
    c.days.push(c.days[0].clone());
    assert_eq!(c.validate(), Err(CalendarError::DuplicateDate));
}
#[test]
fn preceding_december_spill_is_retained_without_verifying_its_year() {
    let c = candidate(
        2027,
        &[
            (2026, 12, 31, PublicDayKind::Rest),
            (2027, 1, 1, PublicDayKind::Rest),
        ],
    );
    assert_eq!(c.validate(), Ok(()));
    let data = PublicCalendarData { years: vec![c] };
    assert_eq!(data.verified_years(), vec![2027]);
    assert_eq!(
        data.merged_day(date(2026, 12, 31)).unwrap().kind,
        Some(PublicDayKind::Rest)
    );
}
#[test]
fn distant_spill_and_spill_only_files_are_rejected() {
    for (y, m, d) in [(2025, 11, 30), (2024, 12, 31), (2027, 1, 1)] {
        let c = candidate(
            2026,
            &[
                (2026, 1, 1, PublicDayKind::Rest),
                (y, m, d, PublicDayKind::Rest),
            ],
        );
        assert_eq!(c.validate(), Err(CalendarError::InvalidDate));
    }
    let c = candidate(2026, &[(2025, 12, 31, PublicDayKind::Rest)]);
    assert_eq!(c.validate(), Err(CalendarError::EmptyYear));
}
#[test]
fn text_header_and_source_limits_apply_before_adoption() {
    let mut c = rest();
    c.days[0].name = "🙂".repeat(64);
    assert_eq!(c.validate(), Ok(()));
    c.days[0].name.push('🙂');
    assert_eq!(c.validate(), Err(CalendarError::InvalidText));
    c = rest();
    c.days[0].name = "\r\n".into();
    assert_eq!(c.validate(), Err(CalendarError::InvalidText));
    c = rest();
    c.etag = Some("x\r\nInjected: y".into());
    assert_eq!(c.validate(), Err(CalendarError::InvalidText));
    c = rest();
    c.last_modified = Some("x".repeat(MAX_LAST_MODIFIED_BYTES + 1));
    assert_eq!(c.validate(), Err(CalendarError::InvalidText));
    c = rest();
    c.papers = vec!["x".repeat(MAX_PAPER_BYTES + 1)];
    assert_eq!(c.validate(), Err(CalendarError::InvalidSource));
    c = rest();
    c.papers = (0..17)
        .map(|i| format!("https://www.gov.cn/zhengce/content/2026/content_{i}.htm"))
        .collect();
    assert_eq!(c.validate(), Err(CalendarError::Capacity));
}
#[test]
fn candidate_date_and_aggregate_year_limits_are_distinct() {
    let mut c = rest();
    c.days = vec![c.days[0].clone(); MAX_CANDIDATE_DAYS + 1];
    assert_eq!(c.validate(), Err(CalendarError::Capacity));
    let mut data = PublicCalendarData::default();
    for y in 2000..2033 {
        data.years
            .push(candidate(y, &[(y, 1, 1, PublicDayKind::Rest)]));
    }
    assert_eq!(data.validate(), Err(CalendarError::Capacity));
    data.years.truncate(2);
    data.years[1] = data.years[0].clone();
    assert_eq!(data.validate(), Err(CalendarError::DuplicateYear));
}
#[test]
fn aggregate_day_limit_is_checked_across_valid_annual_files() {
    let mut data = PublicCalendarData::default();
    for y in 2000..2012 {
        let mut c = candidate(y, &[(y, 1, 1, PublicDayKind::Rest)]);
        c.days.clear();
        for month in 1..=12 {
            for day in 1..=31 {
                if let Ok(date) = LocalDate::new(y, month, day) {
                    c.days.push(PublicCalendarDay {
                        date,
                        kind: PublicDayKind::Rest,
                        name: "测试".into(),
                    });
                }
            }
        }
        assert!(c.validate().is_ok());
        data.years.push(c);
    }
    assert_eq!(data.validate(), Err(CalendarError::Capacity));
}
#[test]
fn cross_year_equal_days_merge_and_conflicting_days_are_order_independent() {
    let a = candidate(2026, &[(2026, 12, 31, PublicDayKind::Rest)]);
    let mut b = candidate(
        2027,
        &[
            (2026, 12, 31, PublicDayKind::Rest),
            (2027, 1, 1, PublicDayKind::Rest),
        ],
    );
    let data = PublicCalendarData {
        years: vec![a.clone(), b.clone()],
    };
    let merged = data.merged_day(date(2026, 12, 31)).unwrap();
    assert_eq!(merged.kind, Some(PublicDayKind::Rest));
    assert_eq!(merged.source_years, vec![2026, 2027]);
    b.days[0].kind = PublicDayKind::Makeup;
    let first = PublicCalendarData {
        years: vec![a.clone(), b.clone()],
    };
    let reverse = PublicCalendarData { years: vec![b, a] };
    assert_eq!(
        first.merged_day(date(2026, 12, 31)),
        reverse.merged_day(date(2026, 12, 31))
    );
    assert_eq!(first.merged_day(date(2026, 12, 31)).unwrap().kind, None);
}
#[test]
fn proposal_shows_additions_deletions_changes_and_human_shadowing() {
    let old = PublicCalendarData {
        years: vec![candidate(
            2026,
            &[
                (2026, 10, 26, PublicDayKind::Rest),
                (2026, 10, 27, PublicDayKind::Rest),
            ],
        )],
    };
    let replacement = candidate(
        2026,
        &[
            (2026, 10, 27, PublicDayKind::Makeup),
            (2026, 10, 28, PublicDayKind::Rest),
        ],
    );
    let entry = manual(
        date(2026, 10, 27),
        DayMapping::SourceDate(date(2026, 10, 29)),
    );
    let proposal = old
        .proposal(&[replacement], std::slice::from_ref(&entry))
        .unwrap();
    assert_eq!(proposal.differences.len(), 3);
    assert!(proposal.differences[0].after.is_none());
    assert_eq!(
        proposal.differences[1].before.as_ref().unwrap().kind,
        Some(PublicDayKind::Rest)
    );
    assert_eq!(
        proposal.differences[1].after.as_ref().unwrap().kind,
        Some(PublicDayKind::Makeup)
    );
    assert_eq!(
        proposal.differences[1].manual_overrides,
        vec![entry.actual_semester_id]
    );
    assert!(proposal.differences[2].before.is_none());
    assert_eq!(old.years[0].days.len(), 2);
}
#[test]
fn proposal_preserves_unselected_years_and_reports_conflicts() {
    let old = data();
    let next = candidate(
        2027,
        &[
            (2026, 12, 31, PublicDayKind::Makeup),
            (2027, 1, 1, PublicDayKind::Rest),
        ],
    );
    let mut baseline = old.clone();
    baseline.years[0].days.push(PublicCalendarDay {
        date: date(2026, 12, 31),
        kind: PublicDayKind::Rest,
        name: "测试".into(),
    });
    let p = baseline.proposal(&[next], &[]).unwrap();
    assert_eq!(p.data.years[0], baseline.years[0]);
    assert_eq!(p.conflicts, vec![date(2026, 12, 31)]);
    assert_eq!(p.selected_years, vec![2027]);
    assert!(matches!(
        old.proposal(&[], &[]),
        Err(CalendarError::EmptySelection)
    ));
    assert!(matches!(
        old.proposal(&[rest(), rest()], &[]),
        Err(CalendarError::DuplicateYear)
    ));
}
fn addition(old: &PublicCalendarData) -> CalendarProposal {
    let mut c = old.years[0].clone();
    c.days.push(PublicCalendarDay {
        date: date(2026, 10, 28),
        kind: PublicDayKind::Rest,
        name: "新增休假".into(),
    });
    old.proposal(&[c], &[]).unwrap()
}
#[test]
fn auto_is_off_without_grant_and_rejects_mode_switch_or_new_year() {
    let old = data();
    let p = addition(&old);
    assert!(p.auto_eligible(&old, &[], context()));
    for ctx in [
        AutoAdoptionContext {
            policy_authorized: false,
            ..context()
        },
        AutoAdoptionContext {
            current_mode: CalendarMode::School,
            ..context()
        },
        AutoAdoptionContext {
            current_mode_revision: Revision::new(5),
            ..context()
        },
    ] {
        assert!(!p.auto_eligible(&old, &[], ctx));
    }
    let p = PublicCalendarData::default()
        .proposal(&[rest()], &[])
        .unwrap();
    assert!(!p.auto_eligible(&PublicCalendarData::default(), &[], context()));
}
#[test]
fn auto_preserves_existing_unchanged_makeup_but_never_adds_or_removes_one() {
    let mut old = data();
    old.years[0].days.push(PublicCalendarDay {
        date: date(2026, 10, 27),
        kind: PublicDayKind::Makeup,
        name: "补班".into(),
    });
    let p = addition(&old);
    assert!(p.auto_eligible(&old, &[], context()));
    let mut changed = old.years[0].clone();
    changed.days.retain(|d| d.kind != PublicDayKind::Makeup);
    assert!(!old
        .proposal(&[changed], &[])
        .unwrap()
        .auto_eligible(&old, &[], context()));
    let mut changed = old.years[0].clone();
    changed.days.push(PublicCalendarDay {
        date: date(2026, 10, 29),
        kind: PublicDayKind::Makeup,
        name: "新补班".into(),
    });
    assert!(!old
        .proposal(&[changed], &[])
        .unwrap()
        .auto_eligible(&old, &[], context()));
}
#[test]
fn auto_rejects_changed_notice_manual_touched_and_forged_differences() {
    let old = data();
    let mut changed = addition(&old).data.years[0].clone();
    changed.papers = vec!["https://www.gov.cn/zhengce/content/2026/content_1.htm".into()];
    assert!(!old
        .proposal(&[changed], &[])
        .unwrap()
        .auto_eligible(&old, &[], context()));
    let entries = [manual(date(2026, 10, 28), DayMapping::Rest)];
    let p = addition(&old);
    assert!(!p.auto_eligible(&old, &entries, context()));
    let mut forged = p.clone();
    forged.differences.clear();
    assert!(!forged.auto_eligible(&old, &[], context()));
}
#[test]
fn auto_accepts_pure_rest_removal_and_ignores_paper_list_order() {
    let mut old = data();
    old.years[0]
        .papers
        .push("https://www.gov.cn/zhengce/content/2026/content_2.htm".into());
    old.years[0].days.push(PublicCalendarDay {
        date: date(2026, 10, 28),
        kind: PublicDayKind::Rest,
        name: "待移除休假".into(),
    });
    let mut replacement = old.years[0].clone();
    replacement.days.pop();
    replacement.papers.reverse();
    let p = old.proposal(&[replacement], &[]).unwrap();
    assert!(p.auto_eligible(&old, &[], context()));
}
#[test]
fn document_resolver_ignores_legacy_verified_flags() {
    let mut doc = document();
    doc.public_calendar.public_verified_years = vec![2026];
    assert!(
        matches!(resolve(&doc,date(2026,10,27)),AcademicDay::Scheduled(d) if d.public_data_unverified)
    );
    doc.public_calendar_data = data();
    assert!(
        matches!(resolve(&doc,date(2026,10,27)),AcademicDay::Scheduled(d) if !d.public_data_unverified)
    );
    assert_eq!(resolve(&doc, date(2026, 10, 26)), AcademicDay::Rest);
}
#[test]
fn document_resolver_requires_makeup_mapping_and_human_entry_wins() {
    let mut doc = document();
    doc.public_calendar_data.years =
        vec![candidate(2026, &[(2026, 10, 26, PublicDayKind::Makeup)])];
    assert_eq!(resolve(&doc, date(2026, 10, 26)), AcademicDay::Unconfirmed);
    doc.public_calendar.entries.push(manual(
        date(2026, 10, 26),
        DayMapping::SourceDate(date(2026, 10, 27)),
    ));
    assert!(
        matches!(resolve(&doc,date(2026,10,26)),AcademicDay::Scheduled(d) if d.source_date==date(2026,10,27))
    );
    doc.public_calendar.entries[0].mapping = DayMapping::Rest;
    assert_eq!(resolve(&doc, date(2026, 10, 26)), AcademicDay::Rest);
}
#[test]
fn document_conflict_is_unconfirmed_until_manual_mapping_resolves_it() {
    let mut doc = document();
    doc.public_calendar_data.years = vec![
        candidate(2026, &[(2026, 12, 31, PublicDayKind::Rest)]),
        candidate(
            2027,
            &[
                (2026, 12, 31, PublicDayKind::Makeup),
                (2027, 1, 1, PublicDayKind::Rest),
            ],
        ),
    ];
    assert_eq!(resolve(&doc, date(2026, 12, 31)), AcademicDay::Unconfirmed);
    doc.public_calendar
        .entries
        .push(manual(date(2026, 12, 31), DayMapping::Rest));
    assert_eq!(resolve(&doc, date(2026, 12, 31)), AcademicDay::Rest);
}
#[test]
fn school_mode_ignores_public_days_and_keeps_missing_coverage_unconfirmed() {
    let mut doc = document();
    doc.public_calendar_data = data();
    doc.calendar_mode = CalendarMode::School;
    assert_eq!(resolve(&doc, date(2026, 10, 26)), AcademicDay::Unconfirmed);
    doc.school_calendar.school_coverage = Some((date(2026, 10, 26), date(2026, 12, 31)));
    assert!(matches!(
        resolve(&doc, date(2026, 10, 26)),
        AcademicDay::Scheduled(_)
    ));
}
#[test]
fn spill_effect_does_not_claim_prior_year_verified() {
    let mut doc = document();
    doc.public_calendar_data.years = vec![candidate(
        2027,
        &[
            (2026, 12, 31, PublicDayKind::Rest),
            (2027, 1, 1, PublicDayKind::Rest),
        ],
    )];
    assert_eq!(resolve(&doc, date(2026, 12, 31)), AcademicDay::Rest);
    assert!(
        matches!(resolve(&doc,date(2026,12,30)),AcademicDay::Scheduled(d) if d.public_data_unverified)
    );
}
fn identity_actor() -> (IdentityState, ActorContext, LocalDateTime) {
    let now = LocalDateTime::new(date(2026, 10, 31), 36000).unwrap();
    let identity = IdentityState::default()
        .transition(
            &IdentityCommand::FirstAdministrator {
                school_id: SchoolId::new(1).unwrap(),
                school_name: "测试".into(),
                id: PrincipalId::new(1).unwrap(),
                login_name: "admin".into(),
                display_name: "管理员".into(),
            },
            None,
            now,
        )
        .unwrap()
        .next;
    let actor = ActorContext {
        principal_id: PrincipalId::new(1).unwrap(),
        school_id: SchoolId::new(1).unwrap(),
        auth_epoch: AuthEpoch::new(1),
        expires_at: LocalDateTime::new(now.date, 40000).unwrap(),
        origin: TrustedOrigin::Local,
    };
    (identity, actor, now)
}
#[test]
fn restricted_adoption_advances_only_schedule_and_public_applied_revisions() {
    let (identity, actor, now) = identity_actor();
    let old = ScheduleDocument::default()
        .transition(
            &ScheduleDocument::default(),
            Revision::default(),
            &identity,
            &actor,
            now,
        )
        .unwrap();
    let adopted = old
        .transition_calendar_data(data(), old.revision, &identity)
        .unwrap();
    assert_eq!(adopted.revision, old.revision.next().unwrap());
    assert_eq!(
        adopted.public_calendar.applied_revision,
        old.public_calendar.applied_revision.next().unwrap()
    );
    assert_eq!(
        adopted.public_calendar.mapping_revision,
        old.public_calendar.mapping_revision
    );
    assert_eq!(adopted.public_calendar.public_verified_years, vec![2026]);
    assert_eq!(adopted.school_calendar, old.school_calendar);
    assert_eq!(adopted.calendar_mode_revision, old.calendar_mode_revision);
    assert_eq!(adopted.rules_revision, old.rules_revision);
    assert_eq!(adopted.semesters, old.semesters);
    let mut invalid = data();
    invalid.years[0].days.clear();
    assert!(
        matches!(adopted.transition_calendar_data(invalid,adopted.revision,&identity),Err(e) if e.kind==DocumentErrorKind::InvalidCalendar)
    );
    assert!(
        matches!(adopted.transition_calendar_data(data(),old.revision,&identity),Err(e) if matches!(e.kind,DocumentErrorKind::RevisionConflict{..}))
    );
}
#[test]
fn adopted_metadata_alone_changes_public_applied_not_mapping_revision() {
    let (identity, actor, now) = identity_actor();
    let old = ScheduleDocument::default()
        .transition(
            &ScheduleDocument::default(),
            Revision::default(),
            &identity,
            &actor,
            now,
        )
        .unwrap()
        .transition_calendar_data(data(), Revision::new(1), &identity)
        .unwrap();
    let mut replacement = old.public_calendar_data.clone();
    replacement.years[0].source_sha256 = [9; 32];
    let new = old
        .transition_calendar_data(replacement, old.revision, &identity)
        .unwrap();
    assert_eq!(
        new.public_calendar.applied_revision,
        old.public_calendar.applied_revision.next().unwrap()
    );
    assert_eq!(
        new.public_calendar.mapping_revision,
        old.public_calendar.mapping_revision
    );
}
#[test]
fn allocation_counts_adopted_strings_headers_and_reserved_capacity() {
    let mut doc = ScheduleDocument::default();
    let baseline = doc.allocation_bytes().unwrap();
    doc.public_calendar_data = data();
    let used = doc.allocation_bytes().unwrap();
    assert!(used > baseline);
    doc.public_calendar_data.years[0].days[0].name.reserve(8192);
    assert!(doc.allocation_bytes().unwrap() >= used + 8192);
}

#[test]
fn auto_does_not_adopt_rest_name_or_overlap_provenance_changes() {
    let old = data();
    let mut replacement = old.years[0].clone();
    replacement.days[0].name = "更名，仍需人工复核".into();
    let proposal = old.proposal(&[replacement], &[]).unwrap();
    assert!(!proposal.auto_eligible(&old, &[], context()));
    let old = PublicCalendarData {
        years: vec![
            candidate(2026, &[(2026, 12, 31, PublicDayKind::Rest)]),
            candidate(2027, &[(2027, 1, 1, PublicDayKind::Rest)]),
        ],
    };
    let mut replacement = old.years[1].clone();
    replacement.days.push(PublicCalendarDay {
        date: date(2026, 12, 31),
        kind: PublicDayKind::Rest,
        name: "新增重叠来源".into(),
    });
    let proposal = old.proposal(&[replacement], &[]).unwrap();
    assert!(!proposal.auto_eligible(&old, &[], context()));
}
