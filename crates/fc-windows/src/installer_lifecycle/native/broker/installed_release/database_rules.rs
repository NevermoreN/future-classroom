//! Pure read bounds and one-attempt native close accounting, never authority.
pub(super) const ROW_LIMIT: usize = 64;
pub(super) const BINARY_LIMIT: usize = 32;
pub(super) const TEXT_LIMIT: usize = 512;
pub(super) const RECEIPT_LIMIT: usize = 256;

pub(super) fn room(kernel: usize, msi: usize, incoming: usize) -> bool {
    kernel
        .checked_add(msi)
        .and_then(|n| n.checked_add(incoming))
        .is_some_and(|n| n <= 64)
}
pub(super) fn text(value: &str) -> bool {
    value.encode_utf16().count() <= TEXT_LIMIT && !value.chars().any(char::is_control)
}

#[derive(Debug, Default)]
pub(super) struct CloseState {
    pub(super) view_open: bool,
    pub(super) view_close_return: Option<u32>,
    pub(super) handle_close_return: Option<u32>,
    view_attempted: bool,
    handle_attempted: bool,
}
impl CloseState {
    pub(super) fn view_attempt(&mut self) -> bool {
        if !self.view_open || self.view_attempted || self.handle_attempted {
            return false;
        }
        self.view_attempted = true;
        true
    }
    pub(super) fn view_returned(&mut self, code: u32) {
        self.view_close_return = Some(code);
        if code == 0 {
            self.view_open = false;
        }
    }
    pub(super) fn handle_attempt(&mut self) -> bool {
        if self.view_open || self.handle_attempted {
            return false;
        }
        self.handle_attempted = true;
        true
    }
    pub(super) fn handle_returned(&mut self, code: u32) {
        self.handle_close_return = Some(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combined_native_namespaces_share_one_64_handle_bound() {
        assert!(room(60, 3, 1));
        assert!(!room(60, 4, 1));
        assert!(!room(usize::MAX, 1, 1));
        assert!(room(0, 64, 0));
    }
    #[test]
    fn view_must_return_zero_before_handle_close_and_no_close_can_retry() {
        let mut state = CloseState {
            view_open: true,
            ..Default::default()
        };
        assert!(!state.handle_attempt());
        assert!(state.view_attempt());
        state.view_returned(6);
        assert!(!state.view_attempt());
        assert!(!state.handle_attempt());
        assert_eq!(state.view_close_return, Some(6));
        let mut state = CloseState {
            view_open: true,
            ..Default::default()
        };
        assert!(state.view_attempt());
        state.view_returned(0);
        assert!(state.handle_attempt());
        state.handle_returned(6);
        assert!(!state.handle_attempt());
        assert_eq!(state.handle_close_return, Some(6));
    }
    #[test]
    fn record_close_has_one_attempt_and_success_records_actual_zero() {
        let mut state = CloseState::default();
        assert!(!state.view_attempt());
        assert!(state.handle_attempt());
        state.handle_returned(0);
        assert!(!state.handle_attempt());
        assert_eq!(state.handle_close_return, Some(0));
    }
    #[test]
    fn captured_text_is_bounded_and_has_no_embedded_controls() {
        assert!(text(&"x".repeat(TEXT_LIMIT)));
        assert!(!text(&"x".repeat(TEXT_LIMIT + 1)));
        assert!(!text("value\0suffix"));
        assert!(!text("value\n"));
        assert!(!text(&"😀".repeat(TEXT_LIMIT / 2 + 1)));
        assert_eq!(ROW_LIMIT, 64);
        assert_eq!(BINARY_LIMIT, 32);
        assert_eq!(RECEIPT_LIMIT, 256);
    }
}
