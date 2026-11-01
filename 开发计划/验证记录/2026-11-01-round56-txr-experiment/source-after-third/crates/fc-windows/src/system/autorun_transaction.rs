//! Private stack-owned KTM/TxR lifetime. No ordinary registry write fallback.
use super::{write_failure, AutostartFailure};
use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Mutex,
    },
    time::Instant,
};
use windows::{
    core::{w, Error, HSTRING},
    Win32::{Foundation::*, Storage::FileSystem::*, System::Registry::*},
};

const IDLE: u8 = 0;
const ACTIVE: u8 = 1;
const FAULTED: u8 = 2;
static MUTATION: AtomicU8 = AtomicU8::new(IDLE);
// Raw identities remain owned here until process exit on a cleanup failure.
// Integer storage does not grant another thread permission to use the handles.
struct Retained {
    key: Option<usize>,
    transaction: Option<usize>,
}
static RETAINED: Mutex<Option<Retained>> = Mutex::new(None);

pub(super) struct MutationPermit;
impl MutationPermit {
    pub(super) fn acquire() -> Option<Self> {
        MUTATION
            .compare_exchange(IDLE, ACTIVE, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}
impl Drop for MutationPermit {
    fn drop(&mut self) {
        // A failed close cannot be reset by logical completion/cancellation.
        let _ = MUTATION.compare_exchange(ACTIVE, IDLE, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub(super) fn remaining_millis(deadline: Instant) -> Option<u32> {
    let remaining = deadline.checked_duration_since(Instant::now())?.as_millis();
    if remaining == 0 {
        return None;
    }
    // Neither zero (default/infinite) nor INFINITE is an acceptable timeout.
    Some(remaining.min(u128::from(u32::MAX - 1)) as u32)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Outcome {
    Committed,
    Aborted,
    Undetermined,
}
pub(super) struct Settlement {
    pub(super) outcome: Outcome,
    pub(super) released: bool,
}
pub(super) struct NativeTransaction<'a> {
    _permit: &'a MutationPermit,
    key: Option<HKEY>,
    transaction: Option<HANDLE>,
    outcome: Outcome,
    cleanup_failed: bool,
    #[cfg(test)]
    fail_key_close: bool,
    #[cfg(test)]
    fail_transaction_close: bool,
}
impl<'a> NativeTransaction<'a> {
    pub(super) fn begin(
        permit: &'a MutationPermit,
        deadline: Instant,
    ) -> Result<Self, AutostartFailure> {
        let timeout = remaining_millis(deadline).ok_or(AutostartFailure::Cancelled)?;
        // SAFETY: non-inheritable, unbound transaction. Reserved isolation args
        // are zero; the remaining original deadline is finite, never restarted.
        let transaction = unsafe {
            CreateTransaction(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                0,
                0,
                timeout,
                w!("FutureClassroom current-user Run change"),
            )
        }
        .map_err(native_failure)?;
        Ok(Self {
            _permit: permit,
            key: None,
            transaction: Some(transaction),
            outcome: Outcome::Undetermined,
            cleanup_failed: false,
            #[cfg(test)]
            fail_key_close: false,
            #[cfg(test)]
            fail_transaction_close: false,
        })
    }
    pub(super) fn open_key(&mut self, path: &str, create: bool) -> Result<(), AutostartFailure> {
        let name = HSTRING::from(path);
        let mut key = HKEY::default();
        let access = KEY_QUERY_VALUE | KEY_SET_VALUE;
        let transaction = self.transaction.expect("owned active transaction");
        // SAFETY: caller supplies only the fixed production or generated scratch
        // path. Both query and staged write use this same transacted HKEY.
        let status = unsafe {
            if create {
                RegCreateKeyTransactedW(
                    HKEY_CURRENT_USER,
                    &name,
                    None,
                    None,
                    REG_OPTION_NON_VOLATILE,
                    access,
                    None,
                    &mut key,
                    None,
                    transaction,
                    None,
                )
            } else {
                RegOpenKeyTransactedW(
                    HKEY_CURRENT_USER,
                    &name,
                    None,
                    access,
                    &mut key,
                    transaction,
                    None,
                )
            }
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if status != ERROR_SUCCESS {
            return Err(write_failure(status));
        }
        self.key = Some(key);
        Ok(())
    }
    pub(super) fn key(&self) -> Option<HKEY> {
        self.key
    }
    pub(super) fn close_key(&mut self) -> Result<(), AutostartFailure> {
        if self.cleanup_failed {
            return Err(AutostartFailure::Interrupted);
        }
        if let Some(key) = self.key {
            #[cfg(test)]
            if self.fail_key_close {
                self.cleanup_failed = true;
                return Err(AutostartFailure::Interrupted);
            }
            // SAFETY: the one exclusive key returned by the transacted open.
            // HKEY must never be passed to CloseHandle (which can falsely succeed).
            let status = unsafe { RegCloseKey(key) };
            if status != ERROR_SUCCESS {
                self.cleanup_failed = true;
                return Err(AutostartFailure::Interrupted);
            }
            self.key = None;
        }
        Ok(())
    }
    pub(super) fn commit(&mut self) -> Result<(), AutostartFailure> {
        if self.key.is_some() || self.cleanup_failed {
            return Err(AutostartFailure::Interrupted);
        }
        // SAFETY: bound HKEY was actually closed, original transaction still owned.
        // This synchronous syscall cannot be forcibly interrupted by a job signal.
        let result = unsafe { CommitTransaction(self.transaction.expect("owned transaction")) };
        if result.is_ok() {
            self.outcome = Outcome::Committed;
        } else {
            self.query_outcome();
        }
        result.map_err(native_failure)
    }
    fn query_outcome(&mut self) {
        let Some(transaction) = self.transaction else {
            return;
        };
        let (mut outcome, mut isolation, mut flags, mut timeout) = (0, 0, 0, 0);
        // SAFETY: exclusive live transaction; all fixed-size output pointers live.
        let result = unsafe {
            GetTransactionInformation(
                transaction,
                &mut outcome,
                &mut isolation,
                &mut flags,
                &mut timeout,
                None,
            )
        };
        if result.is_ok() {
            self.outcome = if outcome == TransactionOutcomeCommitted.0 as u32 {
                Outcome::Committed
            } else if outcome == TransactionOutcomeAborted.0 as u32 {
                Outcome::Aborted
            } else {
                Outcome::Undetermined
            };
        }
    }
    fn settle_inner(&mut self) -> Settlement {
        if self.transaction.is_none() {
            return Settlement {
                outcome: self.outcome,
                released: self.key.is_none(),
            };
        }
        let _ = self.close_key(); // any failure is retained below, never swallowed
        if self.outcome != Outcome::Committed {
            // SAFETY: rollback only the original transaction; never a compensating
            // non-transacted write. RollbackTransaction is synchronous.
            let rollback = unsafe { RollbackTransaction(self.transaction.unwrap()) };
            if rollback.is_ok() {
                self.outcome = Outcome::Aborted;
            } else {
                self.query_outcome();
            }
        }
        if !self.cleanup_failed {
            // SAFETY: correct close API for this original transaction HANDLE.
            if self.close_transaction_handle() {
                self.transaction = None;
            } else {
                self.cleanup_failed = true;
            }
        }
        if self.cleanup_failed {
            // The single permit proves this slot cannot have another owner.
            // No user code, SQL or native syscall runs while its short lock is held.
            retain(Retained {
                key: self.key.take().map(|h| h.0 as usize),
                transaction: self.transaction.take().map(|h| h.0 as usize),
            });
            return Settlement {
                outcome: self.outcome,
                released: false,
            };
        }
        Settlement {
            outcome: self.outcome,
            released: true,
        }
    }
    fn close_transaction_handle(&self) -> bool {
        #[cfg(test)]
        if self.fail_transaction_close {
            // Test-only withholding of the first real close, not an invalid handle
            // or a second close. The exact valid HANDLE goes to retention.
            return false;
        }
        // SAFETY: original owned transaction; production attempts this once.
        unsafe { CloseHandle(self.transaction.unwrap()) }.is_ok()
    }
    #[cfg(test)]
    pub(super) fn fail_transaction_close_for_test(&mut self) {
        self.fail_transaction_close = true;
    }
    pub(super) fn settle(mut self) -> Settlement {
        self.settle_inner()
    }
    #[cfg(test)]
    pub(super) fn fail_key_close_for_test(&mut self) {
        self.fail_key_close = true;
    }
}
impl Drop for NativeTransaction<'_> {
    fn drop(&mut self) {
        if self.key.is_some() || self.transaction.is_some() {
            let _ = self.settle_inner();
        }
    }
}
fn retain(owner: Retained) {
    MUTATION.store(FAULTED, Ordering::Release);
    let mut slot = RETAINED.lock().unwrap_or_else(|poison| poison.into_inner());
    assert!(slot.is_none(), "single retained native transaction owner");
    debug_assert!(owner.key.is_some() || owner.transaction.is_some());
    *slot = Some(owner);
}
pub(super) fn retain_key(key: HKEY) {
    retain(Retained {
        key: Some(key.0 as usize),
        transaction: None,
    });
}

fn native_failure(error: Error) -> AutostartFailure {
    if error.code() == windows::core::HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        AutostartFailure::AccessDenied
    } else {
        AutostartFailure::WriteFailed
    }
}

#[cfg(test)]
pub(super) fn retained_for_test() -> (bool, bool, bool) {
    let slot = RETAINED.lock().unwrap_or_else(|p| p.into_inner());
    (
        MUTATION.load(Ordering::Acquire) == FAULTED,
        slot.as_ref().is_some_and(|s| s.key.is_some()),
        slot.as_ref().is_some_and(|s| s.transaction.is_some()),
    )
}
#[cfg(test)]
pub(super) fn release_retained_for_test() -> bool {
    let mut retained = RETAINED.lock().unwrap_or_else(|p| p.into_inner()).take();
    let Some(owner) = &mut retained else {
        return false;
    };
    // Only the isolated child test invokes explicit negative cleanup with exact
    // original handles; failures put the remaining owner back in the same slot.
    let released = (|| {
        if let Some(key) = owner.key {
            if unsafe { RegCloseKey(HKEY(key as *mut _)) } != ERROR_SUCCESS {
                return false;
            }
            owner.key = None;
        }
        if let Some(tx) = owner.transaction {
            let handle = HANDLE(tx as *mut _);
            let _ = unsafe { RollbackTransaction(handle) };
            if unsafe { CloseHandle(handle) }.is_err() {
                return false;
            }
            owner.transaction = None;
        }
        true
    })();
    if !released {
        *RETAINED.lock().unwrap_or_else(|p| p.into_inner()) = retained;
    }
    // Deliberately retain the permanent FAULTED gate even after test cleanup.
    released
}
