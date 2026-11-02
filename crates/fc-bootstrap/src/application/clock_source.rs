//! Instance-local civil source. Normal constructors always delegate to Windows.
//! The only diagnostic constructor requires the probe's closed owned-deployment token.
use fc_domain::time::{LocalDate, LocalDateTime, TimeError};
use fc_ports::{Clock, MonotonicClock};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(super) struct AppClock {
    system: fc_windows::system::CurrentClock,
    civil: Option<Arc<AtomicBool>>,
}
pub(super) struct MidnightController(Arc<AtomicBool>);
impl AppClock {
    pub(super) fn new() -> Self {
        Self {
            system: fc_windows::system::CurrentClock::new(),
            civil: None,
        }
    }
    pub(super) fn for_owned_teacher_probe(
        _owned: &super::teacher_browser_probe::OwnedDeployment,
    ) -> (Self, MidnightController) {
        let civil = Arc::new(AtomicBool::new(false));
        (
            Self {
                system: fc_windows::system::CurrentClock::new(),
                civil: Some(civil.clone()),
            },
            MidnightController(civil),
        )
    }
    pub(super) fn try_now(&self) -> Result<LocalDateTime, TimeError> {
        match &self.civil {
            None => self.system.try_now(),
            Some(advanced) => {
                let next = advanced.load(Ordering::Acquire);
                LocalDateTime::new(
                    LocalDate::new(2026, 11, if next { 3 } else { 2 })?,
                    if next { 30 } else { 86370 },
                )
            }
        }
    }
}
impl Default for AppClock {
    fn default() -> Self {
        Self::new()
    }
}
impl Clock for AppClock {
    fn now(&self) -> LocalDateTime {
        self.try_now()
            .expect("Windows local clock outside supported civil range")
    }
}
impl MonotonicClock for AppClock {
    fn monotonic_millis(&self) -> u64 {
        self.system.monotonic_millis()
    }
}
impl MidnightController {
    pub(super) fn advance_once(
        &self,
        _owned: &super::teacher_browser_probe::OwnedDeployment,
    ) -> Result<(), &'static str> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "isolated midnight already advanced")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_clock_is_direct_windows_delegate_and_monotonic_is_real() {
        let clock = AppClock::new();
        assert!(clock.civil.is_none());
        let start = clock.monotonic_millis();
        assert!(clock.try_now().is_ok());
        assert!(clock.monotonic_millis() >= start);
    }
    #[test]
    fn civil_transition_is_one_minute_once_and_does_not_advance_monotonic() {
        let civil = Arc::new(AtomicBool::new(false));
        let controller = MidnightController(civil.clone());
        let clock = AppClock {
            system: fc_windows::system::CurrentClock::new(),
            civil: Some(civil),
        };
        let before = clock.now();
        let tick = clock.monotonic_millis();
        assert!(controller
            .0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok());
        let after = clock.now();
        assert_eq!(after.date.days_since(before.date), 1);
        assert_eq!(before.second_of_day(), 86370);
        assert_eq!(after.second_of_day(), 30);
        assert!(controller
            .0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err());
        assert!(clock.monotonic_millis() >= tick);
    }
}
