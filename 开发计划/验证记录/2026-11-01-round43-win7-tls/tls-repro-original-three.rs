pub mod reset {
    use std::{cell::RefCell, sync::{Arc, atomic::{AtomicBool, Ordering}}};
    pub struct TlsExit(pub Arc<AtomicBool>);
    impl Drop for TlsExit { fn drop(&mut self) { self.0.store(true, Ordering::Release); } }
    std::thread_local! {
        static EXIT: RefCell<Option<TlsExit>> = const { RefCell::new(None) };
    }
    pub fn touch() -> bool { EXIT.with(|slot| slot.borrow().is_none()) }
}
pub mod runtime {
    use std::{cell::RefCell, sync::{Arc, atomic::{AtomicBool, Ordering}}};
    pub struct TlsExit(pub Arc<AtomicBool>);
    impl Drop for TlsExit { fn drop(&mut self) { self.0.store(true, Ordering::Release); } }
    std::thread_local! {
        static EXIT: RefCell<Option<TlsExit>> = const { RefCell::new(None) };
    }
    pub fn touch() -> bool { EXIT.with(|slot| slot.borrow().is_none()) }
}
pub mod phase {
    use std::cell::Cell;
    std::thread_local! {
        static PHASE: Cell<&'static str> = const { Cell::new("startup") };
    }
    pub fn touch() -> &'static str { PHASE.with(|slot| slot.get()) }
}
