//! Calendar composition on the existing shared pool. The Backend mutex is held
//! only for bounded writer calls, never while WinHTTP owns a physical request.
use super::{Failure, Outcome, Result};
use fc_application::identity::SessionLease;
use fc_bootstrap::backend::Backend;
use fc_domain::{ids::*, schedule_document::ScheduleDocument};
use fc_ports::calendar::*;
use fc_runtime::worker::JobContext;
use fc_windows::{calendar_download as download, system::CurrentClock};
use std::sync::{Arc, Mutex};

/// The replaceable source adapter owns transport plus decoding. The writer only
/// accepts validated candidates; source data never carries an actor or grant.
struct HolidayCnProvider;
impl CalendarProvider for HolidayCnProvider {
    fn fetch(
        &self,
        year: i32,
        validators: Option<&CalendarFetchValidators>,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<CalendarProviderResponse, CalendarFetchFailure> {
        let validators = validators.map(|v| download::Validators {
            etag: v.etag.clone(),
            last_modified: v.last_modified.clone(),
        });
        match download::fetch(year, validators.as_ref(), cancelled).map_err(failure)? {
            download::Download::NotModified => Ok(CalendarProviderResponse::NotModified),
            download::Download::Modified {
                bytes,
                etag,
                last_modified,
            } => {
                let now = CurrentClock::new()
                    .try_now()
                    .map_err(|_| CalendarFetchFailure::InvalidData)?;
                fc_protocol::calendar_feed::parse(year, &bytes, now, etag, last_modified)
                    .map(CalendarProviderResponse::Downloaded)
                    .map_err(|_| CalendarFetchFailure::InvalidData)
            }
        }
    }
}

pub(super) enum Operation {
    Load(i32),
    Download(i32),
    Adopt {
        year: i32,
        expected: Revision,
        candidates: Vec<CalendarCandidateRef>,
    },
    Policy {
        year: i32,
        expected: Revision,
        enabled: bool,
    },
    Background,
}
pub(super) struct Job {
    pub backend: Arc<Mutex<Backend>>,
    pub lease: Option<SessionLease>,
    pub operation: Operation,
}
pub(super) struct Loaded {
    pub owner: PrincipalId,
    pub document: ScheduleDocument,
    pub status: CalendarStatus,
    pub message: String,
}
impl Job {
    pub fn allocation_bytes(&self) -> Option<usize> {
        let extra = match &self.operation {
            Operation::Adopt { candidates, .. } => candidates
                .capacity()
                .checked_mul(std::mem::size_of::<CalendarCandidateRef>())?,
            _ => 0,
        };
        extra.checked_add(1024)
    }
    pub fn is_download(&self) -> bool {
        matches!(
            self.operation,
            Operation::Download(_) | Operation::Background
        )
    }
    fn loaded(&self, year: i32, message: String) -> Result<Outcome> {
        let lease = self.lease.ok_or(Failure::Stopped)?;
        let backend = self.backend.lock().map_err(|_| Failure::Stopped)?;
        let document = backend.read_schedule(lease)?;
        let status = backend.calendar_status(lease, year)?;
        Ok(Outcome::Calendar(Box::new(Loaded {
            owner: lease.principal_id(),
            document,
            status,
            message,
        })))
    }
    pub fn run(self, context: &JobContext) -> Result<Outcome> {
        context.checkpoint().map_err(|_| Failure::Stopped)?;
        match &self.operation {
            Operation::Load(year) => self.loaded(*year, String::new()),
            Operation::Adopt {
                year,
                expected,
                candidates,
            } => {
                self.backend
                    .lock()
                    .map_err(|_| Failure::Stopped)?
                    .adopt_calendar_while(
                        self.lease.ok_or(Failure::Stopped)?,
                        *expected,
                        candidates.clone(),
                        Arc::new(context.execution_fence()),
                    )?;
                self.loaded(
                    *year,
                    "所选年度候选已采用；补班日仍需明确课程来源，人工映射优先。".into(),
                )
            }
            Operation::Policy {
                year,
                expected,
                enabled,
            } => {
                self.backend
                    .lock()
                    .map_err(|_| Failure::Stopped)?
                    .set_calendar_auto_policy_while(
                        self.lease.ok_or(Failure::Stopped)?,
                        *expected,
                        *enabled,
                        Arc::new(context.execution_fence()),
                    )?;
                self.loaded(
                    *year,
                    if *enabled {
                        "已开启受限的纯放假变化自动采用。"
                    } else {
                        "已关闭自动采用。"
                    }
                    .into(),
                )
            }
            Operation::Download(year) => {
                let reservation = self
                    .backend
                    .lock()
                    .map_err(|_| Failure::Stopped)?
                    .reserve_calendar_fetch_while(
                        self.lease.ok_or(Failure::Stopped)?,
                        *year,
                        Arc::new(context.execution_fence()),
                    )?;
                match reservation {
                    CalendarReservation::RateLimited { next_allowed } => self.loaded(*year, format!(
                        "尚未到下一次允许下载的时间：{:04}-{:02}-{:02} {:02}:{:02}。保留已有候选与采用资料。",
                        next_allowed.date.year(),next_allowed.date.month(),next_allowed.date.day(),
                        next_allowed.second_of_day()/3600,next_allowed.second_of_day()%3600/60)),
                    CalendarReservation::Reserved(ticket) => {
                        let message = fetch_and_finish(&self.backend, ticket, context)?;
                        self.loaded(*year, message)
                    }
                }
            }
            Operation::Background => {
                // A busy foreground KDF/writer must not occupy the other pool
                // worker while waiting for its mutex just to check maintenance.
                let ticket = self
                    .backend
                    .try_lock()
                    .map_err(|_| Failure::Stopped)?
                    .calendar_background_fetch_while(Arc::new(context.execution_fence()))?;
                let attempted = ticket.is_some();
                if let Some(ticket) = ticket {
                    fetch_and_finish(&self.backend, ticket, context)?;
                }
                Ok(Outcome::CalendarMaintained { attempted })
            }
        }
    }
}

fn failure(error: download::DownloadError) -> CalendarFetchFailure {
    use download::DownloadError as E;
    match error {
        E::Cancelled => CalendarFetchFailure::Cancelled,
        E::Timeout => CalendarFetchFailure::Deadline,
        E::Tls => CalendarFetchFailure::Tls,
        E::TooLarge => CalendarFetchFailure::TooLarge,
        E::Forbidden | E::NotFound | E::HttpStatus => CalendarFetchFailure::Http,
        E::InvalidYear | E::InvalidValidator | E::InvalidResponse => {
            CalendarFetchFailure::InvalidData
        }
        E::Unavailable | E::Busy | E::Network => CalendarFetchFailure::Network,
    }
}
fn message(outcome: &CalendarFetchOutcome) -> &'static str {
    match outcome {
        CalendarFetchOutcome::Downloaded(_) => {
            "候选缓存已更新。请核对差异和通知来源，再明确采用；仅已授权的纯放假变化可能自动生效。"
        }
        CalendarFetchOutcome::NotModified => "来源返回未修改，已有候选仍保留。",
        CalendarFetchOutcome::Failed(CalendarFetchFailure::Cancelled) => {
            "下载已取消，保留已有候选和采用资料；下载间隔不退还。"
        }
        CalendarFetchOutcome::Failed(CalendarFetchFailure::InvalidData) => {
            "来源数据校验未通过，保留已有候选和采用资料。"
        }
        CalendarFetchOutcome::Failed(CalendarFetchFailure::TooLarge) => {
            "来源超过响应容量上限，保留已有资料。"
        }
        CalendarFetchOutcome::Failed(CalendarFetchFailure::Tls) => {
            "安全连接校验未通过，保留已有资料。"
        }
        CalendarFetchOutcome::Failed(CalendarFetchFailure::Deadline) => {
            "本次下载超过处理期限，保留已有资料。"
        }
        CalendarFetchOutcome::Failed(_) => "本次下载未成功，已有候选及离线采用资料仍可使用。",
    }
}
fn fetch_and_finish(
    backend: &Arc<Mutex<Backend>>,
    ticket: CalendarFetchTicket,
    context: &JobContext,
) -> Result<String> {
    let validators = CalendarFetchValidators {
        etag: ticket.etag.clone(),
        last_modified: ticket.last_modified.clone(),
    };
    let mut outcome = match HolidayCnProvider.fetch(ticket.year, Some(&validators), &|| {
        context.checkpoint().is_err()
    }) {
        Ok(CalendarProviderResponse::NotModified) => CalendarFetchOutcome::NotModified,
        Ok(CalendarProviderResponse::Downloaded(candidate)) => {
            CalendarFetchOutcome::Downloaded(candidate)
        }
        Err(error) => CalendarFetchOutcome::Failed(error),
    };
    // Cancellation/deadline cannot publish a body obtained by a non-preemptible
    // native call after its owner expired. The attempt itself stays rate-limited.
    if let Err(error) = context.checkpoint() {
        outcome =
            CalendarFetchOutcome::Failed(if error == fc_runtime::worker::JobError::Deadline {
                CalendarFetchFailure::Deadline
            } else {
                CalendarFetchFailure::Cancelled
            });
    }
    let text = message(&outcome).to_owned();
    let status = backend
        .lock()
        .map_err(|_| Failure::Stopped)?
        .finish_calendar_fetch_while(ticket, outcome, Arc::new(context.execution_fence()))?;
    // The final writer fence may turn a late success into cancellation. Report
    // the persisted result rather than claiming an uncommitted cache update.
    Ok(
        if status
            .years
            .iter()
            .find(|y| y.year == status.selected_year)
            .is_some_and(|y| y.last_error == Some(CalendarFetchFailure::Cancelled))
        {
            message(&CalendarFetchOutcome::Failed(
                CalendarFetchFailure::Cancelled,
            ))
            .into()
        } else {
            text
        },
    )
}
