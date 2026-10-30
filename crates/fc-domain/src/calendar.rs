//! Non-active public-calendar candidates and pure, reviewable adoption proposals.
//! A provider is a candidate source, never authority to choose school teaching days.
use crate::{
    ids::{Revision, SemesterId},
    schedule::{CalendarEntry, CalendarMode},
    time::{LocalDate, LocalDateTime},
};
use std::collections::{BTreeMap, BTreeSet};

pub const PROVIDER_ID: &str = "NateScarlet/holiday-cn";
pub const PROVIDER_VERSION: &str = "HolidayCnV1";
pub const PROVIDER_LICENSE: &str = "MIT";
pub const PROVIDER_REPOSITORY: &str = "https://github.com/NateScarlet/holiday-cn";
pub const MAX_CANDIDATE_BYTES: usize = 64 * 1024;
pub const MAX_CANDIDATE_DAYS: usize = 400;
pub const MAX_CANDIDATE_PAPERS: usize = 16;
pub const MAX_PUBLIC_YEARS: usize = 32;
pub const MAX_PUBLIC_DAYS: usize = 4096;
pub const MAX_PAPER_BYTES: usize = 512;
pub const MAX_ETAG_BYTES: usize = 256;
pub const MAX_LAST_MODIFIED_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublicDayKind {
    Rest,
    Makeup,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicCalendarDay {
    pub date: LocalDate,
    pub kind: PublicDayKind,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarCandidate {
    pub source_year: i32,
    pub fetched_at: LocalDateTime,
    pub source_sha256: [u8; 32],
    pub papers: Vec<String>,
    pub days: Vec<PublicCalendarDay>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarError {
    InvalidYear,
    Capacity,
    EmptyYear,
    InvalidSource,
    InvalidDate,
    DuplicateDate,
    InvalidText,
    DuplicateYear,
    EmptySelection,
}

/// Only canonical HTTPS State Council notice URLs are accepted. This is provenance
/// syntax validation, not a claim that downloading the candidate authenticates or
/// interprets the notice. Network TLS and administrator review remain separate.
pub fn valid_notice_url(url: &str) -> bool {
    let Some(path) = url
        .strip_prefix("https://www.gov.cn/zhengce/content/")
        .or_else(|| url.strip_prefix("https://www.gov.cn/zhengce/zhengceku/"))
    else {
        return false;
    };
    if url.len() > MAX_PAPER_BYTES
        || !(path.ends_with(".htm") || path.ends_with(".html"))
        || path.is_empty()
    {
        return false;
    }
    path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    })
}
fn header_valid(value: &Option<String>, limit: usize) -> bool {
    value.as_ref().is_none_or(|s| {
        !s.is_empty() && s.len() <= limit && s.bytes().all(|b| (0x20..=0x7e).contains(&b))
    })
}
impl CalendarCandidate {
    pub fn source_url(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/NateScarlet/holiday-cn/master/{}.json",
            self.source_year
        )
    }
    pub fn validate(&self) -> Result<(), CalendarError> {
        if !(1..=9999).contains(&self.source_year) {
            return Err(CalendarError::InvalidYear);
        }
        if self.days.is_empty() {
            return Err(CalendarError::EmptyYear);
        }
        if self.days.len() > MAX_CANDIDATE_DAYS || self.papers.len() > MAX_CANDIDATE_PAPERS {
            return Err(CalendarError::Capacity);
        }
        let mut papers = BTreeSet::new();
        if self.papers.is_empty()
            || self
                .papers
                .iter()
                .any(|s| !valid_notice_url(s) || !papers.insert(s))
        {
            return Err(CalendarError::InvalidSource);
        }
        if !header_valid(&self.etag, MAX_ETAG_BYTES)
            || !header_valid(&self.last_modified, MAX_LAST_MODIFIED_BYTES)
        {
            return Err(CalendarError::InvalidText);
        }
        let mut dates = BTreeSet::new();
        for day in &self.days {
            if day.date.year() != self.source_year
                && !(day.date.year() == self.source_year - 1 && day.date.month() == 12)
            {
                return Err(CalendarError::InvalidDate);
            }
            if !dates.insert(day.date) {
                return Err(CalendarError::DuplicateDate);
            }
            if day.name.trim().is_empty()
                || day.name.chars().count() > 64
                || day.name.chars().any(char::is_control)
            {
                return Err(CalendarError::InvalidText);
            }
        }
        // A file containing only preceding-year spill cannot verify source_year.
        if !self.days.iter().any(|d| d.date.year() == self.source_year) {
            return Err(CalendarError::EmptyYear);
        }
        Ok(())
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        let mut bytes = self
            .papers
            .capacity()
            .checked_mul(std::mem::size_of::<String>())?
            .checked_add(
                self.days
                    .capacity()
                    .checked_mul(std::mem::size_of::<PublicCalendarDay>())?,
            )?;
        for paper in &self.papers {
            bytes = bytes.checked_add(paper.capacity())?;
        }
        for day in &self.days {
            bytes = bytes.checked_add(day.name.capacity())?;
        }
        for header in [&self.etag, &self.last_modified].into_iter().flatten() {
            bytes = bytes.checked_add(header.capacity())?;
        }
        Some(bytes)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublicCalendarData {
    pub years: Vec<CalendarCandidate>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicDaySummary {
    /// None means sources disagree: the effective public day is UNCONFIRMED.
    pub kind: Option<PublicDayKind>,
    pub names: Vec<String>,
    pub source_years: Vec<i32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarDifference {
    pub date: LocalDate,
    pub before: Option<PublicDaySummary>,
    pub after: Option<PublicDaySummary>,
    /// Existing human entries are preserved verbatim and continue to win.
    pub manual_overrides: Vec<SemesterId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarProposal {
    pub data: PublicCalendarData,
    pub differences: Vec<CalendarDifference>,
    pub conflicts: Vec<LocalDate>,
    pub selected_years: Vec<i32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoAdoptionContext {
    pub current_mode: CalendarMode,
    pub captured_mode_revision: Revision,
    pub current_mode_revision: Revision,
    /// The sole writer must derive this from a still-valid durable administrator grant.
    pub policy_authorized: bool,
}
impl PublicCalendarData {
    pub fn validate(&self) -> Result<(), CalendarError> {
        if self.years.len() > MAX_PUBLIC_YEARS {
            return Err(CalendarError::Capacity);
        }
        let mut seen = BTreeSet::new();
        let mut days = 0usize;
        for candidate in &self.years {
            candidate.validate()?;
            if !seen.insert(candidate.source_year) {
                return Err(CalendarError::DuplicateYear);
            }
            days = days
                .checked_add(candidate.days.len())
                .ok_or(CalendarError::Capacity)?;
            if days > MAX_PUBLIC_DAYS {
                return Err(CalendarError::Capacity);
            }
        }
        Ok(())
    }
    pub fn verified_years(&self) -> Vec<i32> {
        let mut years: Vec<_> = self.years.iter().map(|c| c.source_year).collect();
        years.sort_unstable();
        years
    }
    pub fn merged_day(&self, date: LocalDate) -> Option<PublicDaySummary> {
        let mut summary: Option<PublicDaySummary> = None;
        for candidate in &self.years {
            for day in candidate.days.iter().filter(|d| d.date == date) {
                let merged = summary.get_or_insert_with(|| PublicDaySummary {
                    kind: Some(day.kind),
                    names: Vec::new(),
                    source_years: Vec::new(),
                });
                if merged.kind != Some(day.kind) {
                    merged.kind = None;
                }
                if !merged.names.contains(&day.name) {
                    merged.names.push(day.name.clone());
                }
                if !merged.source_years.contains(&candidate.source_year) {
                    merged.source_years.push(candidate.source_year);
                }
            }
        }
        if let Some(summary) = &mut summary {
            summary.names.sort();
            summary.source_years.sort_unstable();
        }
        summary
    }
    fn merged_days(&self) -> BTreeMap<LocalDate, PublicDaySummary> {
        let mut merged: BTreeMap<LocalDate, PublicDaySummary> = BTreeMap::new();
        for candidate in &self.years {
            for day in &candidate.days {
                let summary = merged.entry(day.date).or_insert_with(|| PublicDaySummary {
                    kind: Some(day.kind),
                    names: Vec::new(),
                    source_years: Vec::new(),
                });
                if summary.kind != Some(day.kind) {
                    summary.kind = None;
                }
                if !summary.names.contains(&day.name) {
                    summary.names.push(day.name.clone());
                }
                if !summary.source_years.contains(&candidate.source_year) {
                    summary.source_years.push(candidate.source_year);
                }
            }
        }
        for summary in merged.values_mut() {
            summary.names.sort();
            summary.source_years.sort_unstable();
        }
        merged
    }
    /// Replace selected source years, retaining all other adopted years. A proposal
    /// is only data: committing it still requires exact session, candidate and schedule CAS.
    pub fn proposal(
        &self,
        replacements: &[CalendarCandidate],
        manual: &[CalendarEntry],
    ) -> Result<CalendarProposal, CalendarError> {
        self.validate()?;
        if replacements.is_empty() {
            return Err(CalendarError::EmptySelection);
        }
        let mut selected = BTreeSet::new();
        for candidate in replacements {
            candidate.validate()?;
            if !selected.insert(candidate.source_year) {
                return Err(CalendarError::DuplicateYear);
            }
        }
        let mut data = self.clone();
        data.years.retain(|c| !selected.contains(&c.source_year));
        data.years.extend_from_slice(replacements);
        data.years.sort_by_key(|c| c.source_year);
        data.validate()?;
        let before = self.merged_days();
        let after = data.merged_days();
        let dates: BTreeSet<_> = before.keys().chain(after.keys()).copied().collect();
        let mut differences = Vec::new();
        for date in dates {
            let old = before.get(&date);
            let new = after.get(&date);
            if old != new {
                let manual_overrides: BTreeSet<_> = manual
                    .iter()
                    .filter(|m| m.date == date)
                    .map(|m| m.actual_semester_id)
                    .collect();
                differences.push(CalendarDifference {
                    date,
                    before: old.cloned(),
                    after: new.cloned(),
                    manual_overrides: manual_overrides.into_iter().collect(),
                });
            }
        }
        let conflicts = after
            .iter()
            .filter(|(_, s)| s.kind.is_none())
            .map(|(d, _)| *d)
            .collect();
        Ok(CalendarProposal {
            data,
            differences,
            conflicts,
            selected_years: selected.into_iter().collect(),
        })
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        let mut bytes = self
            .years
            .capacity()
            .checked_mul(std::mem::size_of::<CalendarCandidate>())?;
        for candidate in &self.years {
            bytes = bytes.checked_add(candidate.allocation_bytes()?)?;
        }
        Some(bytes)
    }
}
impl CalendarProposal {
    /// Only existing, manually adopted provenance can be extended automatically.
    /// Recompute the diff so a modified display/proposal object is not authorization.
    pub fn auto_eligible(
        &self,
        previous: &PublicCalendarData,
        manual: &[CalendarEntry],
        context: AutoAdoptionContext,
    ) -> bool {
        if !context.policy_authorized
            || context.current_mode != CalendarMode::Public
            || context.captured_mode_revision != context.current_mode_revision
            || self.selected_years.is_empty()
        {
            return false;
        }
        let replacements: Vec<_> = self
            .data
            .years
            .iter()
            .filter(|c| self.selected_years.contains(&c.source_year))
            .cloned()
            .collect();
        let Ok(verified) = previous.proposal(&replacements, manual) else {
            return false;
        };
        if &verified != self || !self.conflicts.is_empty() || self.differences.is_empty() {
            return false;
        }
        for candidate in &replacements {
            let Some(old) = previous
                .years
                .iter()
                .find(|c| c.source_year == candidate.source_year)
            else {
                return false;
            };
            let old_papers: BTreeSet<_> = old.papers.iter().collect();
            let new_papers: BTreeSet<_> = candidate.papers.iter().collect();
            if old_papers != new_papers {
                return false;
            }
        }
        self.differences.iter().all(|d| {
            d.manual_overrides.is_empty()
                && match (&d.before, &d.after) {
                    (None, Some(day)) | (Some(day), None) => day.kind == Some(PublicDayKind::Rest),
                    _ => false,
                }
        })
    }
}

#[cfg(test)]
#[path = "calendar_tests.rs"]
mod tests;
