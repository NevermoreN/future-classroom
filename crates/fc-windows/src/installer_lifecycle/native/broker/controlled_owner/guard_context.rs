//! One synchronous native Install's external-record context, never a Session or
//! ApiOwner borrow. MSI may call on another thread. All callback state/control
//! access is serialized, no lock is held by the API caller across Install, and
//! there is deliberately no unsafe Send/Sync implementation for Resources.
use super::super::super::super::guard_contract::{Grant, INFO_PREFIX};
use super::*;
use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicPtr, AtomicUsize},
        Mutex, TryLockError,
    },
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

type RecordHandler = Option<unsafe extern "system" fn(*mut c_void, u32, MSIHANDLE) -> i32>;
// The generated windows 0.62 binding types the final output pointer as a handler
// value. Use the actual SDK pointer-to-handler ABI; do not pass a function as an
// output buffer. The borrowed MSI record is never changed or closed.
#[link(name = "msi")]
unsafe extern "system" {
    #[link_name = "MsiSetExternalUIRecord"]
    fn set_external_ui_record(
        handler: RecordHandler,
        filter: u32,
        context: *const c_void,
        previous: *mut RecordHandler,
    ) -> u32;
}

struct State {
    // This owner contains Control/Token only, never a gate, Pending, transaction
    // or thread-affine MSI handle. Native kernel handles can be used on callback
    // threads. FFI is the sole cross-thread entry; this mutex is the exclusive
    // access boundary, rather than a blanket Send exception for Resources.
    resources: Resources,
    pipe: Option<HANDLE>,
    nonce: Option<Nonce>,
    granted: bool,
    returned: bool,
    write: Option<Box<GrantWrite>>,
}
struct GrantWrite {
    overlapped: OVERLAPPED,
    bytes: [u8; v2::FRAME_BYTES],
    length: usize,
    pending: bool,
    cancel_attempted: bool,
}
struct Context {
    state: Mutex<State>,
    // Borrowed original G/B pipe: Session keeps the cap alive. Its API thread
    // performs no pipe IO until Install returns and registration is removed.
    broker_pipe: HANDLE,
    deadline: Instant,
    deadline_ticks: u64,
    transaction: u32,
    package: Option<ObservedPackage>,
    sid: Vec<u8>,
    active: AtomicUsize,
    fault: AtomicBool,
    retired: AtomicBool,
}
// One original G has exactly one Run/registration. Even successful removal does
// not assume an undocumented queued-callback quiescence guarantee: retire the
// one metadata Box until physical G exit. On success it owns no native caps or
// CLAIMS slot. On UNKNOWN it instead preserves original caps/IO/claim, faulted.
// This same single slot rejects a second registration; no extra owner pool.
static CONTEXT_UNTIL_EXIT: AtomicPtr<Context> = AtomicPtr::new(std::ptr::null_mut());

pub(super) struct Registration {
    context: Option<Box<Context>>,
    registered: bool,
}
impl Registration {
    pub(super) fn new(
        broker_pipe: HANDLE,
        deadline: Instant,
        deadline_ticks: u64,
        transaction: u32,
        sid: Vec<u8>,
        package: Option<ObservedPackage>,
    ) -> Result<Self> {
        current(deadline)?;
        if transaction == 0 || !CONTEXT_UNTIL_EXIT.load(Ordering::Acquire).is_null() {
            return Err(LifecycleError::Invalid);
        }
        let mut this = Self {
            context: Some(Box::new(Context {
                state: Mutex::new(State {
                    resources: Resources::empty()?,
                    pipe: None,
                    nonce: None,
                    granted: false,
                    returned: false,
                    write: None,
                }),
                broker_pipe,
                deadline,
                deadline_ticks,
                transaction,
                package,
                sid,
                active: AtomicUsize::new(0),
                fault: AtomicBool::new(false),
                retired: AtomicBool::new(false),
            })),
            registered: false,
        };
        let context = this.context.as_ref().unwrap();
        let mut previous = None;
        let code = unsafe {
            set_external_ui_record(
                Some(record_callback),
                (INSTALLLOGMODE_INFO.0 | INSTALLLOGMODE_ACTIONSTART.0) as u32,
                (&**context as *const Context).cast(),
                &mut previous,
            )
        };
        if code != 0 {
            return Err(LifecycleError::Unsettled);
        }
        this.registered = true;
        // This private one-shot G installs its only external record handler.
        // A pre-existing handler's context cannot be recovered by this API;
        // reject the unexpected state without entering Install.
        if previous.is_some() {
            return Err(LifecycleError::Invalid);
        }
        Ok(this)
    }
    fn remove(&mut self) -> Result<()> {
        // Stop new protocol work before removing the handler. A queued callback
        // may still enter later, but the fixed retired metadata remains alive.
        self.context
            .as_ref()
            .unwrap()
            .retired
            .store(true, Ordering::Release);
        if self.registered {
            let mut previous = None;
            let rc = unsafe { set_external_ui_record(None, 0, std::ptr::null(), &mut previous) };
            if rc != 0 {
                return Err(LifecycleError::Unsettled);
            }
            self.registered = false;
            if previous.map(|f| f as usize) != Some(record_callback as *const () as usize) {
                return Err(LifecycleError::Invalid);
            }
        }
        // The only MSI API invocation is synchronous and has returned before
        // finish/Drop. Removal prevents future delivery; also check no callback
        // is still touching the context before its native controls can close.
        if self
            .context
            .as_ref()
            .unwrap()
            .active
            .load(Ordering::Acquire)
            != 0
        {
            return Err(LifecycleError::Unsettled);
        }
        Ok(())
    }
    fn retire(&mut self, fault: bool) {
        if fault {
            FAULTED.store(true, Ordering::Release);
        }
        if let Some(context) = self.context.take() {
            context.retired.store(true, Ordering::Release);
            let pointer = Box::into_raw(context);
            if CONTEXT_UNTIL_EXIT
                .compare_exchange(
                    std::ptr::null_mut(),
                    pointer,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                // Impossible for this one-shot G. Never free a potentially live
                // callback context or overwrite the first retained capability.
                std::process::abort();
            }
        }
    }
    pub(super) fn finish(mut self) -> Result<()> {
        if let Err(error) = self.remove() {
            self.retire(true);
            return Err(error);
        }
        let context = self.context.as_ref().unwrap();
        let mut state = context.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.write.as_ref().is_some_and(|write| write.pending) {
            let _ = context.cancel_write(&mut state);
            if state.write.as_ref().is_some_and(|write| write.pending) {
                drop(state);
                self.retire(true);
                return Err(LifecycleError::Unsettled);
            }
        }
        let proven = !context.fault.load(Ordering::Acquire)
            && state.granted
            && state.returned
            && state.pipe.is_none();
        // Negative controls are returned only after actual Install has returned.
        // Failed Close remains sticky in Resources; it cannot be retried into a
        // successful Returned message or successful physical G exit.
        let controls = state.resources.release();
        drop(state);
        if let Err(error) = controls {
            self.retire(true);
            return Err(error);
        }
        self.retire(!proven);
        if proven {
            Ok(())
        } else {
            Err(LifecycleError::Unsettled)
        }
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        if self.context.is_none() {
            return;
        }
        if self.remove().is_err() {
            self.retire(true);
            return;
        }
        let pending = self
            .context
            .as_ref()
            .unwrap()
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .write
            .as_ref()
            .is_some_and(|write| write.pending);
        if pending {
            self.retire(true);
            return;
        }
        // Unexpected early Rust return is negative. Return known controls once;
        // failed close stays owned by this original context/CLAIMS until exit.
        let _ = self
            .context
            .as_ref()
            .unwrap()
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .resources
            .release();
        self.retire(true);
    }
}

fn field_one(record: MSIHANDLE) -> Result<Option<String>> {
    if record.0 == 0 {
        return Err(LifecycleError::Invalid);
    }
    if unsafe { MsiRecordGetFieldCount(record) } == 0 {
        return Ok(None);
    }
    let mut b = [0u16; 4096];
    let mut length = (b.len() - 1) as u32;
    let rc = unsafe {
        MsiRecordGetStringW(
            record,
            1,
            Some(windows::core::PWSTR(b.as_mut_ptr())),
            Some(&mut length),
        )
    };
    // Unrelated long INFO records are not our closed protocol. Missing the one
    // exact guard record will still make finish negative, never a fake grant.
    if rc == ERROR_MORE_DATA.0 {
        return Ok(None);
    }
    if rc != 0 || length as usize >= b.len() {
        return Err(LifecycleError::Invalid);
    }
    Ok(Some(
        String::from_utf16(&b[..length as usize]).map_err(|_| LifecycleError::Invalid)?,
    ))
}

unsafe extern "system" fn record_callback(
    raw: *mut c_void,
    message: u32,
    record: MSIHANDLE,
) -> i32 {
    if raw.is_null() {
        return 2;
    }
    // SAFETY: registration pins this Box; its fixed one-shot retirement keeps
    // the address alive until physical process exit, including queued entries.
    let context = unsafe { &*raw.cast::<Context>() };
    if context.active.fetch_add(1, Ordering::AcqRel) != 0 {
        context.fault.store(true, Ordering::Release);
        context.active.fetch_sub(1, Ordering::AcqRel);
        return 2;
    }
    let result = catch_unwind(AssertUnwindSafe(|| context.record(message, record)));
    let good = matches!(result, Ok(Ok(())));
    if !good {
        context.fault.store(true, Ordering::Release);
    }
    context.active.fetch_sub(1, Ordering::AcqRel);
    // Cancellation is merely negative UI policy, never a terminal receipt.
    if good {
        0
    } else {
        2
    }
}
impl Context {
    fn record(&self, message: u32, record: MSIHANDLE) -> Result<()> {
        current(self.deadline)?;
        if self.retired.load(Ordering::Acquire) || self.fault.load(Ordering::Acquire) {
            return Err(LifecycleError::Unsettled);
        }
        let kind = message & 0xff00_0000;
        if kind != INSTALLMESSAGE_INFO.0 as u32 && kind != INSTALLMESSAGE_ACTIONSTART.0 as u32 {
            return Ok(());
        }
        let Some(text) = field_one(record)? else {
            return Ok(());
        };
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) | Err(TryLockError::Poisoned(_)) => {
                return Err(LifecycleError::Unsettled);
            }
        };
        if kind == INSTALLMESSAGE_INFO.0 as u32 && text.starts_with(INFO_PREFIX) {
            if self.package.is_some() {
                return Err(LifecycleError::Invalid);
            }
            self.grant(&mut state, Nonce::parse_info(&text)?)?;
        } else if kind == INSTALLMESSAGE_INFO.0 as u32 && text.starts_with(v2::INFO_PREFIX) {
            let (package, nonce) = ObservedPackage::parse_info(&text)?;
            if self.package.as_ref() != Some(&package) {
                return Err(LifecycleError::Invalid);
            }
            self.grant(&mut state, nonce)?;
        } else if kind == INSTALLMESSAGE_ACTIONSTART.0 as u32 && text == "InstallInitialize" {
            // A native typed successor, rather than an INFO log or callback
            // return, proves the preceding immediate guard actually returned0.
            if !first_successor(state.granted, state.returned)? {
                // InstallExecute can replay this typed action while running
                // the queued script. The original guard was already returned;
                // this notification owns no new pipe, grant or bridge receipt.
                return Ok(());
            }
            let pipe = state.pipe.ok_or(LifecycleError::Unsettled)?;
            state.resources.close_handle(pipe)?;
            state.pipe = None;
            state.resources.release()?;
            let nonce = state.nonce.ok_or(LifecycleError::Invalid)?;
            if let Some(package) = &self.package {
                let returned = v2::Bridge::new(
                    BridgeKind::Returned,
                    self.transaction,
                    nonce,
                    package.clone(),
                )?;
                send_guard_bytes(self.deadline, self.broker_pipe, &returned.encode())?;
            } else {
                let returned = Bridge::new(BridgeKind::Returned, self.transaction, nonce, 0)?;
                send_frame(self.deadline, self.broker_pipe, &returned.encode())?;
            }
            state.returned = true;
        }
        Ok(())
    }
    fn grant(&self, state: &mut State, nonce: Nonce) -> Result<()> {
        if state.nonce.is_some() || state.granted || state.returned {
            return Err(LifecycleError::Invalid);
        }
        state.nonce = Some(nonce);
        // CA has already posted its server Connect before sending INFO. Opening
        // only the fixed nonce path is synchronous and never waits on CA reply.
        if state.resources.handles.len() >= HANDLE_LIMIT {
            return Err(LifecycleError::Invalid);
        }
        let pipe = unsafe {
            CreateFileW(
                &HSTRING::from(nonce.pipe_name()),
                GENERIC_WRITE.0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION | FILE_FLAG_OVERLAPPED,
                None,
            )?
        };
        state.resources.add(pipe, Role::Control);
        state.pipe = Some(pipe);
        let server = self.authenticate_server(state, pipe)?;
        if let Some(package) = &self.package {
            let request = v2::Bridge::new(
                BridgeKind::Request,
                self.transaction,
                nonce,
                package.clone(),
            )?;
            send_guard_bytes(self.deadline, self.broker_pipe, &request.encode())?;
            let GuardFrame::Package(bytes) = receive_guard_frame(self.deadline, self.broker_pipe)?
            else {
                return Err(LifecycleError::Invalid);
            };
            let ack = v2::Bridge::parse(&bytes)?;
            if ack.kind() != BridgeKind::Ack
                || ack.transaction_id() != self.transaction
                || ack.nonce() != nonce
                || ack.package() != package
            {
                return Err(LifecycleError::Invalid);
            }
            live(server)?;
            let grant = v2::Grant::new(
                package.clone(),
                nonce,
                self.deadline_ticks,
                self.transaction,
            )?;
            grant.validate(package, nonce, unsafe { GetTickCount64() })?;
            current(self.deadline)?;
            self.write_grant(state, pipe, &grant.encode())?;
        } else {
            let request = Bridge::new(BridgeKind::Request, self.transaction, nonce, 0)?;
            send_frame(self.deadline, self.broker_pipe, &request.encode())?;
            let ack = Bridge::parse(&receive_frame(self.deadline, self.broker_pipe)?)?;
            if ack.kind() != BridgeKind::Ack
                || ack.transaction_id() != self.transaction
                || ack.nonce() != nonce
                || ack.code() != 0
            {
                return Err(LifecycleError::Invalid);
            }
            live(server)?;
            let grant = Grant::new(nonce, self.deadline_ticks, self.transaction)?;
            grant.validate(nonce, unsafe { GetTickCount64() })?;
            current(self.deadline)?;
            self.write_grant(state, pipe, &grant.encode())?;
        }
        current(self.deadline)?;
        // Keep pipe/server/token/image alive while CA resumes, captures its real
        // G client and reads buffered grant. Do not FlushFileBuffers or wait for
        // CA here: it remains blocked inside this very MsiProcessMessage call.
        state.granted = true;
        Ok(())
    }
    fn write_grant(&self, state: &mut State, pipe: HANDLE, bytes: &[u8]) -> Result<()> {
        let expected = if self.package.is_some() {
            v2::FRAME_BYTES
        } else {
            48
        };
        if bytes.len() != expected {
            return Err(LifecycleError::Invalid);
        }
        if state.resources.handles.len() >= HANDLE_LIMIT || state.write.is_some() {
            return Err(LifecycleError::Invalid);
        }
        let event = unsafe { CreateEventW(None, true, false, None)? };
        state.resources.add(event, Role::Control);
        // Both native buffers stay at fixed heap addresses even if a deadline
        // leaves IO incomplete. Never free an outstanding OVERLAPPED on stack.
        state.write = Some(Box::new(GrantWrite {
            overlapped: OVERLAPPED {
                hEvent: event,
                ..Default::default()
            },
            bytes: {
                let mut out = [0; v2::FRAME_BYTES];
                out[..bytes.len()].copy_from_slice(bytes);
                out
            },
            length: bytes.len(),
            pending: true,
            cancel_attempted: false,
        }));
        let write = state.write.as_mut().unwrap();
        // Windows 7 also requires the immediate-count pointer. Completion
        // authority remains the original GetOverlappedResult below.
        let mut immediate = 0;
        let start = unsafe {
            WriteFile(
                pipe,
                Some(&write.bytes[..write.length]),
                Some(&mut immediate),
                Some(&mut write.overlapped),
            )
        };
        if let Err(error) = start {
            if error.code() != ERROR_IO_PENDING.to_hresult() {
                write.pending = false; // API did not queue this operation
                return Err(error.into());
            }
        }
        loop {
            let write = state.write.as_mut().unwrap();
            let mut transferred = 0;
            match unsafe { GetOverlappedResult(pipe, &write.overlapped, &mut transferred, false) } {
                Ok(()) => {
                    write.pending = false;
                    if transferred as usize == write.length {
                        return current(self.deadline);
                    }
                    return Err(LifecycleError::Unsettled);
                }
                Err(error) if error.code() == ERROR_IO_INCOMPLETE.to_hresult() => {
                    if let Err(error) = pause(self.deadline) {
                        let _ = self.cancel_write(state);
                        return Err(error);
                    }
                }
                Err(error) if error.code() == ERROR_OPERATION_ABORTED.to_hresult() => {
                    write.pending = false;
                    return Err(error.into());
                }
                Err(error) => {
                    let _ = self.cancel_write(state);
                    return Err(error.into());
                }
            }
        }
    }
    fn cancel_write(&self, state: &mut State) -> Result<()> {
        let Some(write) = state.write.as_mut().filter(|write| write.pending) else {
            return Ok(());
        };
        let pipe = state.pipe.ok_or(LifecycleError::Unsettled)?;
        if !write.cancel_attempted {
            write.cancel_attempted = true;
            let cancel = unsafe { CancelIoEx(pipe, Some(&write.overlapped)) };
            if let Err(error) = cancel {
                if error.code() != ERROR_NOT_FOUND.to_hresult() {
                    return Err(error.into());
                }
            }
        }
        // Cancel success is not completion. Harvest the original operation; if
        // the original deadline is exhausted, unknown IO retains this entire
        // bounded context until process exit rather than inventing a return.
        loop {
            let mut transferred = 0;
            match unsafe { GetOverlappedResult(pipe, &write.overlapped, &mut transferred, false) } {
                Ok(()) => {
                    write.pending = false;
                    return Ok(());
                }
                Err(error) if error.code() == ERROR_OPERATION_ABORTED.to_hresult() => {
                    write.pending = false;
                    return Ok(());
                }
                Err(error) if error.code() == ERROR_IO_INCOMPLETE.to_hresult() => {
                    pause(self.deadline)?
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    fn authenticate_server(&self, state: &mut State, pipe: HANDLE) -> Result<HANDLE> {
        let mut pid = 0;
        unsafe {
            GetNamedPipeServerProcessId(pipe, &mut pid)?;
        }
        let process = state.resources.process(pid, Role::Control)?;
        let original = process_identity(process)?;
        live(process)?;
        if !engine_parent_sid_allowed(&state.resources.token_sid(process)?, &self.sid) {
            return Err(LifecycleError::Invalid);
        }
        let path = image_path(process)?;
        let mut windows = [0u16; PATH_LIMIT];
        let n = unsafe { GetWindowsDirectoryW(Some(&mut windows)) };
        if n == 0 || n as usize >= windows.len() {
            return Err(LifecycleError::Invalid);
        }
        let root = PathBuf::from(OsString::from_wide(&windows[..n as usize]));
        let actual = path.to_string_lossy().to_ascii_lowercase();
        if ![
            root.join("System32").join("msiexec.exe"),
            root.join("SysWOW64").join("msiexec.exe"),
        ]
        .iter()
        .any(|p| p.to_string_lossy().to_ascii_lowercase() == actual)
        {
            return Err(LifecycleError::Invalid);
        }
        let image = state.resources.open_file(
            &path,
            GENERIC_READ.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        state.resources.add(image, Role::Control);
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe {
            GetFileInformationByHandle(image, &mut info)?;
        }
        if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY.0 | FILE_ATTRIBUTE_REPARSE_POINT.0)
            != 0
        {
            return Err(LifecycleError::Invalid);
        }
        let mut again = 0;
        unsafe {
            GetNamedPipeServerProcessId(pipe, &mut again)?;
        }
        if again != original.process_id || process_identity(process)? != original {
            return Err(LifecycleError::Invalid);
        }
        live(process)?;
        Ok(process)
    }
}

fn first_successor(granted: bool, returned: bool) -> Result<bool> {
    if !granted {
        return Err(LifecycleError::Unsettled);
    }
    Ok(!returned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_replay_does_not_return_original_guard_twice() {
        assert_eq!(
            first_successor(false, false),
            Err(LifecycleError::Unsettled)
        );
        assert_eq!(first_successor(false, true), Err(LifecycleError::Unsettled));
        // The scheduling notification consumes the original return exactly
        // once; the later InstallExecute script notification does not cancel
        // MSI or perform a second native close/bridge send. Actual MSI replay
        // and transaction/effects settlement remain integration requirements.
        assert_eq!(first_successor(true, false), Ok(true));
        assert_eq!(first_successor(true, true), Ok(false));
    }
}
