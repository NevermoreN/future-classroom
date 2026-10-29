//! Current local civil time and independent process-monotonic lease time.
use fc_domain::time::{LocalDate, LocalDateTime, TimeError};
use fc_ports::{Clock, MonotonicClock};
use std::time::Instant;

pub struct CurrentClock {
    started: Instant,
}
impl CurrentClock {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
    pub fn try_now(&self) -> Result<LocalDateTime, TimeError> {
        // SAFETY: GetLocalTime returns a value and takes/retains no application pointers.
        let value = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        let month = u8::try_from(value.wMonth).map_err(|_| TimeError::InvalidDate)?;
        let day = u8::try_from(value.wDay).map_err(|_| TimeError::InvalidDate)?;
        if value.wHour >= 24 || value.wMinute >= 60 || value.wSecond >= 60 {
            return Err(TimeError::InvalidSecond);
        }
        let date = LocalDate::new(i32::from(value.wYear), month, day)?;
        LocalDateTime::new(
            date,
            u32::from(value.wHour) * 3600
                + u32::from(value.wMinute) * 60
                + u32::from(value.wSecond),
        )
    }
}
impl Default for CurrentClock {
    fn default() -> Self {
        Self::new()
    }
}
impl Clock for CurrentClock {
    fn now(&self) -> LocalDateTime {
        self.try_now()
            .expect("Windows local clock outside supported civil range")
    }
}
impl MonotonicClock for CurrentClock {
    fn monotonic_millis(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_current_clock_provides_valid_civil_and_monotonic_times() {
        let clock = CurrentClock::new();
        let first = clock.monotonic_millis();
        assert!(clock.try_now().is_ok());
        assert!(clock.monotonic_millis() >= first);
    }
}
