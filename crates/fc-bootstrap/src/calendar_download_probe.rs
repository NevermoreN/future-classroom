//! Opt-in, one-shot public HTTPS diagnostic. No app root, writer, UI, or cache.
use fc_domain::calendar::PROVIDER_ID;
use fc_windows::calendar_download::{self, Download};
use fc_windows::system::CurrentClock;
use std::time::{Duration, Instant};

pub fn run() -> Result<(), String> {
    const YEAR: i32 = 2026;
    let started = Instant::now();
    let cancelled = || started.elapsed() >= Duration::from_secs(30);
    let clock = CurrentClock::new();
    // The same synchronous transport is used by the application worker. This
    // command owns the main thread exclusively and creates no request threads.
    let downloaded = calendar_download::fetch(YEAR, None, &cancelled).map_err(|e| {
        format!("CALENDAR_DOWNLOAD_PROBE_FAIL provider={PROVIDER_ID} year={YEAR} stage=download error={e:?} elapsed_ms={} cooperative_budget_ms=30000 native_hard_preemption=NOT_PROVEN", started.elapsed().as_millis())
    })?;
    let Download::Modified {
        bytes,
        etag,
        last_modified,
    } = downloaded
    else {
        return Err(format!("CALENDAR_DOWNLOAD_PROBE_FAIL provider={PROVIDER_ID} year={YEAR} stage=download error=Unexpected304 native_hard_preemption=NOT_PROVEN"));
    };
    let fetched_at = clock.try_now().map_err(|_| {
        format!("CALENDAR_DOWNLOAD_PROBE_FAIL provider={PROVIDER_ID} year={YEAR} stage=clock error=InvalidCivilTime native_hard_preemption=NOT_PROVEN")
    })?;
    let candidate = fc_protocol::calendar_feed::parse(YEAR, &bytes, fetched_at, etag, last_modified).map_err(|e| {
        format!("CALENDAR_DOWNLOAD_PROBE_FAIL provider={PROVIDER_ID} year={YEAR} stage=parse error={e:?} elapsed_ms={} native_hard_preemption=NOT_PROVEN", started.elapsed().as_millis())
    })?;
    if cancelled() {
        return Err(format!("CALENDAR_DOWNLOAD_PROBE_FAIL provider={PROVIDER_ID} year={YEAR} stage=parse error=Deadline elapsed_ms={} native_hard_preemption=NOT_PROVEN", started.elapsed().as_millis()));
    }
    let hash: String = candidate
        .source_sha256
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    println!("CALENDAR_DOWNLOAD_PROBE_PASS provider={PROVIDER_ID} year={YEAR} days={} papers={} body_bytes={} source_sha256={hash} etag_present={} last_modified_present={} elapsed_ms={} cooperative_budget_ms=30000 native_hard_preemption=NOT_PROVEN application_data_writes=0", candidate.days.len(), candidate.papers.len(), bytes.len(), candidate.etag.is_some(), candidate.last_modified.is_some(), started.elapsed().as_millis());
    Ok(())
}
