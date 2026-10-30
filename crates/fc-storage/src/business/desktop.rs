//! Public local desktop facts. Read only the installed current class, never an
//! administrator-shaped actor. Every fact and the persistent day floor share the
//! existing writer transaction and the same authority publication revision.
use super::{display, display_codec as dto, *};
use fc_domain::{desktop::*, time::*};
use std::collections::BTreeSet;

const WATERMARK: &str = "business/desktop/max-observed-day/v1";
pub(super) fn read_watermark(
    d: &dyn TransactionDocuments,
    state: &IdentityState,
) -> Result<Option<LocalDate>, StorageError> {
    let Some(row) = d.get(WATERMARK)? else {
        return Ok(None);
    };
    if state.installation.is_none() {
        return Err(StorageError::Database);
    }
    let value = codec::value(&row.value)?;
    codec::object_fields(&value, &["format", "date"])?;
    if codec::number(&value, "format")? != 1 {
        return Err(StorageError::Database);
    }
    codec::read_date(&value["date"]).map(Some)
}
/// A successful date-sensitive content write has also observed its day. Persist
/// that observation in the existing business commit, even before the next frame.
pub(super) fn advance_watermark(
    d: &dyn TransactionDocuments,
    state: &IdentityState,
    day: LocalDate,
) -> Result<Option<Mutation>, StorageError> {
    if read_watermark(d, state)?.is_some_and(|current| current >= day) {
        return Ok(None);
    }
    Ok(Some(Mutation::Put {
        key: WATERMARK.into(),
        value: codec::bytes(&json!({"format":1,"date":codec::date(day)}))?,
    }))
}
pub(super) struct ReadDesktop<S> {
    pub(super) offset: u64,
    pub(super) material_cursor: Option<fc_domain::materials::MaterialCursor>,
    pub(super) clock: Arc<dyn Clock + Send + Sync>,
    pub(super) lesson_observations: super::lesson::Observations,
    pub(super) factory: Factory<S>,
}
fn record(
    d: &dyn TransactionDocuments,
    key: &ContentKey,
) -> Result<Option<ContentRecord>, StorageError> {
    let Some(row) = d.get(&content_key(key))? else {
        return Ok(None);
    };
    let result = codec::read_record(&codec::value(&row.value)?)?;
    if &result.key != key {
        return Err(StorageError::Database);
    }
    if result.text.is_empty() {
        return Ok(None);
    }
    Ok(Some(result))
}
impl<S: Send> TransactionTask<S> for ReadDesktop<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        if identity.installation.is_none() {
            return Err(StorageError::InvalidCommand);
        }
        let captured_at = self.clock.now();
        let previous = read_watermark(d, &identity)?;
        let date_watermark = previous.map_or(captured_at.date, |date| date.max(captured_at.date));
        let advances_day = previous != Some(date_watermark);
        let change_confirmation = schedule::confirm_change(d, captured_at)?;
        let schedule_document = schedule::load(d, &identity)?;
        let (lesson_context, lesson_mutation) = super::lesson::observe_with_floor(
            d,
            &identity,
            &schedule_document,
            captured_at,
            super::lesson::witnessed(&self.lesson_observations, captured_at)?
                .into_iter()
                .chain(
                    (captured_at.date < date_watermark)
                        .then(|| LocalDateTime::new(date_watermark, 0).unwrap()),
                )
                .max(),
        )?;
        let advances = advances_day || change_confirmation.is_some() || lesson_mutation.is_some();
        let current_revision = d.revision()?;
        let global_revision = if advances {
            current_revision
                .next()
                .map_err(|_| StorageError::InvalidCommand)?
        } else {
            current_revision
        };
        let profiles = super::profiles::select(d, &identity, &lesson_context, captured_at)?;
        let mut facts = DesktopFacts {
            global_revision,
            captured_at,
            date_watermark,
            clock_rollback: captured_at.date < date_watermark,
            schedule: schedule_document,
            lesson_context,
            profile_contents: super::profile_content::snapshots(d, &profiles)?,
            profiles,
            display: display::load(d, &identity)?,
            identity,
            homework_offset: 0,
            homework_eligible_total: 0,
            homework: Vec::new(),
            reminder: None,
            materials_source: None,
            materials_page: None,
        };
        if !facts.clock_rollback {
            let (source, page) = materials::desktop_page(
                d,
                &facts.lesson_context,
                &facts.identity,
                &facts.schedule,
                captured_at,
                self.material_cursor,
                facts
                    .profiles
                    .subject_profile
                    .as_ref()
                    .map_or(5, |p| p.materials.visible_rows as usize),
            )?;
            facts.materials_source = source;
            facts.materials_page = page;
            if let Some(class) = facts
                .identity
                .installation
                .as_ref()
                .and_then(|i| i.current_class)
            {
                let reminder_key = ContentKey::SharedReminder(ReminderKey {
                    class_id: class,
                    actual_date: captured_at.date,
                });
                facts.reminder = record(d, &reminder_key)?;
                // Include all metadata and the reminder before reserving the page's
                // actual JSON byte budget. Offsets/totals require at most 40 more bytes.
                let base = display::encode(&dto::facts(&facts), display::MAX_FACTS_BYTES)?.len();
                let mut remaining = display::MAX_FACTS_BYTES.saturating_sub(base + 128);
                let maximum_homework = 64 - usize::from(facts.reminder.is_some());
                // Native DirectWrite keeps each card below 262144 UTF-16 units.
                // Reserve the remaining units for subject/teacher/page labels.
                let mut remaining_units = 240_000usize.saturating_sub(
                    facts
                        .reminder
                        .as_ref()
                        .map_or(0, |r| r.text.encode_utf16().count()),
                );
                let mut remaining_card_units = 262_000usize;
                let keys = facts
                    .identity
                    .assignments
                    .iter()
                    .filter(|a| {
                        a.class_id == class
                            && a.enabled
                            && facts.identity.is_teacher(a.teacher_id, class, a.subject_id)
                    })
                    .map(|a| (a.subject_id, a.teacher_id))
                    .collect::<BTreeSet<_>>();
                let mut page_full = false;
                for (subject, teacher) in keys {
                    let key = ContentKey::Homework(HomeworkKey {
                        class_id: class,
                        actual_date: captured_at.date,
                        subject_id: subject,
                        owner_teacher_id: teacher,
                    });
                    let Some(value) = record(d, &key)? else {
                        continue;
                    };
                    if value.revision > global_revision
                        || facts.identity.account(value.last_actor_id).is_err()
                    {
                        return Err(StorageError::Database);
                    }
                    let position = facts.homework_eligible_total;
                    facts.homework_eligible_total = facts
                        .homework_eligible_total
                        .checked_add(1)
                        .ok_or(StorageError::InvalidCommand)?;
                    if position < self.offset || page_full {
                        continue;
                    }
                    let size = display::encode(&codec::record(&value), MAX_VALUE_BYTES)?.len() + 1;
                    let units = value.text.encode_utf16().count();
                    // Match native prepare_content exactly: one heading block
                    // "subject · teacher", then one complete homework text block.
                    let subject_name = &facts
                        .identity
                        .subjects
                        .iter()
                        .find(|s| s.id == subject)
                        .ok_or(StorageError::Database)?
                        .name;
                    let teacher_name = &facts
                        .identity
                        .account(teacher)
                        .map_err(|_| StorageError::Database)?
                        .display_name;
                    let card_units = units
                        + subject_name.encode_utf16().count()
                        + 3
                        + teacher_name.encode_utf16().count();
                    if facts.homework.len() == maximum_homework
                        || size > remaining
                        || units > remaining_units
                        || card_units > remaining_card_units
                    {
                        page_full = true;
                        continue;
                    }
                    remaining -= size;
                    remaining_units -= units;
                    remaining_card_units -= card_units;
                    facts.homework.push(value);
                }
                facts.homework_offset = self.offset.min(facts.homework_eligible_total);
            }
        }
        facts.validate().map_err(|_| StorageError::Database)?;
        let outcome = display::encode(&dto::facts(&facts), display::MAX_FACTS_BYTES)?;
        // A result produced across midnight must not be queued as today's facts.
        if self.clock.now().date != captured_at.date {
            return Err(StorageError::Busy);
        }
        if !advances {
            return Ok(TransactionDecision {
                outcome,
                commit: None,
                precommit: None,
            });
        }
        let snapshot = (self.factory)(&facts.identity, global_revision)?;
        let clock = self.clock;
        let context = facts.lesson_context.clone();
        let observations = self.lesson_observations;
        let identity = facts.identity.clone();
        let schedule = facts.schedule.clone();
        let mut mutations = Vec::new();
        if advances_day {
            mutations.push(Mutation::Put {
                key: WATERMARK.into(),
                value: codec::bytes(&json!({"format":1,"date":codec::date(date_watermark)}))?,
            });
        }
        if let Some(mutation) = lesson_mutation {
            mutations.push(mutation);
        }
        if let Some(confirmation) = change_confirmation {
            mutations.push(confirmation);
        }
        Ok(TransactionDecision {
            outcome,
            commit: Some((mutations, snapshot)),
            precommit: Some(Box::new(move || {
                let fresh_now = clock.now();
                let floor = super::lesson::witnessed(&observations, fresh_now)?;
                let fresh = context
                    .observe_with_floor(&identity, &schedule, fresh_now, floor)
                    .map_err(StorageError::Lesson)?;
                if fresh_now.date == captured_at.date
                    && fresh.current == context.current
                    && fresh.active_override == context.active_override
                    && fresh.history == context.history
                    && fresh.base_trust == context.base_trust
                    && fresh.blocked_until == context.blocked_until
                {
                    Ok(())
                } else {
                    Err(StorageError::Busy)
                }
            })),
        })
    }
}
