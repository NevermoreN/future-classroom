pub mod reset {
    use std::{cell::RefCell, sync::{Arc, atomic::{AtomicBool, Ordering}}};
    pub struct TlsExit(pub Arc<AtomicBool>);
    impl Drop for TlsExit { fn drop(&mut self) { self.0.store(true, Ordering::Release); } }
    std::thread_local! {
        #[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "nightly-2026-10-29 rebuilt Win7 std wraps this const initializer in a non-const function"))]
        static EXIT: RefCell<Option<TlsExit>> = const { RefCell::new(None) };
    }
    pub fn touch() -> bool { EXIT.with(|slot| slot.borrow().is_none()) }
}
pub mod runtime {
    use std::{cell::RefCell, sync::{Arc, atomic::{AtomicBool, Ordering}}};
    pub struct TlsExit(pub Arc<AtomicBool>);
    impl Drop for TlsExit { fn drop(&mut self) { self.0.store(true, Ordering::Release); } }
    std::thread_local! {
        #[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "nightly-2026-10-29 rebuilt Win7 std wraps this const initializer in a non-const function"))]
        static EXIT: RefCell<Option<TlsExit>> = const { RefCell::new(None) };
    }
    pub fn touch() -> bool { EXIT.with(|slot| slot.borrow().is_none()) }
}
pub mod phase {
    use std::cell::Cell;
    std::thread_local! {
        #[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "nightly-2026-10-29 rebuilt Win7 std wraps this const initializer in a non-const function"))]
        static PHASE: Cell<&'static str> = const { Cell::new("startup") };
    }
    pub fn touch() -> &'static str { PHASE.with(|slot| slot.get()) }
}

std::thread_local! { static UNANNOTATED: std::cell::Cell<bool> = std::cell::Cell::new(false); }
pub fn sentinel() -> bool { UNANNOTATED.with(|slot| slot.get()) }
