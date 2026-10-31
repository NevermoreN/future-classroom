use std::cell::Cell;
std::thread_local! { #[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "fixed nightly rebuilt Win7 std wraps even const TLS initializers in a non-const function"))] static VALUE: Cell<bool> = const { Cell::new(false) }; }
pub fn read() -> bool { VALUE.with(|value| value.get()) }
