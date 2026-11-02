//! Immediate Type 1 guard for the controlled owner's actual MSI invocation.
//!
//! The session and callback records are borrowed. Only a grant written by the
//! original, pinned G image after its original B acknowledged this invocation
//! is admissible. The INFO message and its UI return value carry no authority.
use super::*;
use crate::installer_lifecycle::{
    broker::manifest,
    guard_contract::{v2, Grant, Nonce, GUARD_TEMPLATE_QUERY},
};
use fc_ports::RandomSource;
use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    panic::{catch_unwind, AssertUnwindSafe},
    time::Duration,
};
use windows::{
    core::{PCWSTR, PSTR, PWSTR},
    Win32::System::{
        ApplicationInstallationAndServicing::*,
        LibraryLoader::{GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS},
        Pipes::*,
        SystemInformation::GetTickCount64,
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
    },
};

mod abi2;
mod carrier_image;

const MSI_FAILURE: u32 = 1603;
const MAX_IMAGE: usize = 64 * 1024 * 1024;
const SEAL_MAGIC: &[u8; 16] = b"FC-SETUP-MSI\0v1\0";
// This is cancellation/drain reserve, never a new grant/install budget. A late
// native completion is retained, not released by elapsed time or a retry-close.
const CANCEL_DRAIN_MS: u64 = 1000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Io {
    None,
    Accept,
    Read,
}
struct MsiOwned {
    handle: MSIHANDLE,
    view_open: bool,
    close_attempted: bool,
    close_return: Option<u32>,
}
struct GuardState {
    resources: Resources,
    module: Option<HMODULE>,
    module_close_attempted: bool,
    msi: Vec<MsiOwned>,
    pipe: Option<HANDLE>,
    event: Option<HANDLE>,
    overlapped: OVERLAPPED,
    io: Io,
    grant_bytes: [u8; v2::FRAME_BYTES],
    entry_tick: u64,
    grant_deadline: Option<u64>,
    peer: Option<GuardianIdentity>,
}
impl GuardState {
    fn new() -> Result<Box<Self>> {
        // The same CLAIMS pool bounds this CA alongside all other native owners.
        let resources = Resources::empty()?;
        Ok(Box::new(Self {
            resources,
            module: None,
            module_close_attempted: false,
            msi: Vec::with_capacity(8),
            pipe: None,
            event: None,
            overlapped: OVERLAPPED::default(),
            io: Io::None,
            grant_bytes: [0; v2::FRAME_BYTES],
            entry_tick: unsafe { GetTickCount64() },
            grant_deadline: None,
            peer: None,
        }))
    }
    fn room(&self, count: usize) -> Result<()> {
        if self.resources.handles.len()
            + self.msi.len()
            + usize::from(self.module.is_some())
            + count
            > HANDLE_LIMIT
        {
            return Err(LifecycleError::Invalid);
        }
        self.resources.check_live()
    }
    fn time(&self) -> Result<()> {
        self.resources.check_live()?;
        let now = unsafe { GetTickCount64() };
        if now.saturating_sub(self.entry_tick) >= 120_000
            || self.grant_deadline.is_some_and(|deadline| now >= deadline)
        {
            Err(LifecycleError::Expired)
        } else {
            Ok(())
        }
    }
    fn module_reference(&mut self) -> Result<()> {
        self.room(1)?;
        let mut module = HMODULE::default();
        // FROM_ADDRESS acquires an ordinary additional loader reference. The
        // host still owns the invocation's original reference until CA return.
        // No permanent PIN and no CloseHandle on an HMODULE are used.
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                PCWSTR(FcControlledInnerGuard as *const () as *const u16),
                &mut module,
            )?;
        }
        self.module = Some(module);
        Ok(())
    }
    fn register_msi(&mut self, handle: MSIHANDLE, view_open: bool) -> Result<MSIHANDLE> {
        if handle.0 == 0 {
            return Err(LifecycleError::Unsettled);
        }
        // room was checked BEFORE each API that can acquire a handle.
        self.msi.push(MsiOwned {
            handle,
            view_open,
            close_attempted: false,
            close_return: None,
        });
        Ok(handle)
    }
    fn close_msi(&mut self, handle: MSIHANDLE) -> Result<()> {
        let index = self
            .msi
            .iter()
            .position(|item| item.handle == handle)
            .ok_or(LifecycleError::Invalid)?;
        let item = &mut self.msi[index];
        if item.close_attempted {
            return Err(LifecycleError::Faulted);
        }
        if item.view_open {
            let code = unsafe { MsiViewClose(handle) };
            if code != 0 {
                item.close_attempted = true;
                item.close_return = Some(code);
                return Err(LifecycleError::Unsettled);
            }
            item.view_open = false;
        }
        item.close_attempted = true;
        let code = unsafe { MsiCloseHandle(handle) };
        item.close_return = Some(code);
        if code != 0 {
            return Err(LifecycleError::Unsettled);
        }
        self.msi.remove(index);
        Ok(())
    }
    fn effective_user(&mut self) -> Result<()> {
        self.room(1)?;
        let mut token = HANDLE::default();
        match unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token) } {
            Ok(()) => {
                self.resources.add(token, Role::Token);
                self.resources.sid = Resources::sid_from_token(token)?;
            }
            Err(error) if error.code() == ERROR_NO_TOKEN.to_hresult() => {
                self.resources.sid = self.resources.token_sid(unsafe { GetCurrentProcess() })?;
            }
            Err(error) => return Err(error.into()),
        }
        require_user(&self.resources.sid)?;
        Ok(())
    }
    fn prepare_accept(&mut self, nonce: Nonce, length: usize) -> Result<()> {
        if length != 48 && length != v2::FRAME_BYTES {
            return Err(LifecycleError::Invalid);
        }
        self.room(2)?;
        let event = unsafe { CreateEventW(None, true, false, None)? };
        self.resources.add(event, Role::Control);
        self.event = Some(event);
        self.overlapped.hEvent = event;
        let name = HSTRING::from(nonce.pipe_name());
        let pipe = with_security(&self.resources.sid, false, |attributes| {
            let pipe = unsafe {
                CreateNamedPipeW(
                    &name,
                    PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_MESSAGE
                        | PIPE_READMODE_MESSAGE
                        | PIPE_WAIT
                        | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    length as u32,
                    length as u32,
                    0,
                    Some(attributes),
                )
            };
            if pipe == INVALID_HANDLE_VALUE {
                Err(Error::from_thread())
            } else {
                Ok(pipe)
            }
        })?;
        self.resources.add(pipe, Role::Control);
        self.pipe = Some(pipe);
        self.io = Io::Accept;
        match unsafe { ConnectNamedPipe(pipe, Some(&raw mut self.overlapped)) } {
            Ok(()) => {
                self.io = Io::None;
                Ok(())
            }
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => {
                self.io = Io::None;
                Ok(())
            }
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => Ok(()),
            Err(error) => {
                self.io = Io::None;
                Err(error.into())
            }
        }
    }
    fn message(&mut self, install: MSIHANDLE, info: &str) -> Result<()> {
        self.room(1)?;
        let record = self.register_msi(unsafe { MsiCreateRecord(1) }, false)?;
        let text = HSTRING::from(info);
        let format = HSTRING::from("[1]");
        if unsafe { MsiRecordSetStringW(record, 0, &format) } != 0
            || unsafe { MsiRecordSetStringW(record, 1, &text) } != 0
        {
            return Err(LifecycleError::Unsettled);
        }
        // G writes the single grant and returns from the callback without
        // waiting for CA continuation. Thus this synchronous API cannot form a
        // duplex wait cycle. The UI numeric result is never authorization.
        let code = unsafe { MsiProcessMessage(install, INSTALLMESSAGE_INFO, record) };
        let closed = self.close_msi(record);
        if code < 0 {
            return Err(LifecycleError::Unsettled);
        }
        closed
    }
    fn completed_io(&mut self) -> Result<u32> {
        let pipe = self.pipe.ok_or(LifecycleError::Invalid)?;
        let mut bytes = 0;
        match unsafe { GetOverlappedResult(pipe, &raw const self.overlapped, &mut bytes, false) } {
            Ok(()) => {
                self.io = Io::None;
                Ok(bytes)
            }
            Err(error) if error.code() == ERROR_IO_INCOMPLETE.to_hresult() => {
                Err(LifecycleError::Unsettled)
            }
            Err(error) if error.code() == ERROR_OPERATION_ABORTED.to_hresult() => {
                self.io = Io::None;
                Err(error.into())
            }
            // A failed completion query is not proof of native return. Keep
            // the original pending state so finish must cancel/drain or retain.
            Err(error) => Err(error.into()),
        }
    }
    fn receive(&mut self, nonce: Nonce, format: &abi2::PackageFormat) -> Result<()> {
        self.time()?;
        if self.io == Io::Accept {
            // No new wait for a client after the INFO callback has returned.
            // An external direct inner invocation is rejected immediately.
            self.completed_io()?;
        }
        let length = match format {
            abi2::PackageFormat::Legacy => 48,
            abi2::PackageFormat::Abi2(_) => v2::FRAME_BYTES,
        };
        let pipe = self.pipe.ok_or(LifecycleError::Invalid)?;
        let mut available = 0;
        let mut message_remaining = 0;
        unsafe {
            PeekNamedPipe(
                pipe,
                None,
                0,
                None,
                Some(&mut available),
                Some(&mut message_remaining),
            )?;
        }
        if available as usize != length || message_remaining as usize != length {
            return Err(LifecycleError::Invalid);
        }
        self.overlapped = OVERLAPPED {
            hEvent: self.event.ok_or(LifecycleError::Invalid)?,
            ..Default::default()
        };
        unsafe { ResetEvent(self.overlapped.hEvent)? };
        let mut bytes = 0;
        self.io = Io::Read;
        match unsafe {
            ReadFile(
                pipe,
                Some(&mut self.grant_bytes[..length]),
                Some(&mut bytes),
                Some(&raw mut self.overlapped),
            )
        } {
            Ok(()) => {
                if self.completed_io()? as usize != length {
                    return Err(LifecycleError::Unsettled);
                }
            }
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
                // Data was already present, but an incomplete read still
                // cannot grant success. Cancellation/drain handles this path.
                if self.completed_io()? as usize != length {
                    return Err(LifecycleError::Unsettled);
                }
            }
            Err(error) => {
                // A direct failure other than IO_PENDING did not queue IO.
                self.io = Io::None;
                return Err(error.into());
            }
        }
        self.grant_deadline = Some(match format {
            abi2::PackageFormat::Legacy => {
                let grant = Grant::parse(&self.grant_bytes[..length])?;
                grant.validate(nonce, unsafe { GetTickCount64() })?;
                grant.deadline_ticks()
            }
            abi2::PackageFormat::Abi2(package) => {
                let grant = v2::Grant::parse(&self.grant_bytes[..length])?;
                grant.validate(package, nonce, unsafe { GetTickCount64() })?;
                grant.deadline_ticks()
            }
        });
        self.time()
    }
    fn peer_image(&mut self) -> Result<HANDLE> {
        self.time()?;
        let pipe = self.pipe.ok_or(LifecycleError::Invalid)?;
        let mut pid = 0;
        unsafe { GetNamedPipeClientProcessId(pipe, &mut pid)? };
        if pid == 0 {
            return Err(LifecycleError::Invalid);
        }
        self.room(1)?;
        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )?
        };
        self.resources.add(process, Role::Guardian);
        if unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT
            || unsafe { GetProcessId(process) } != pid
        {
            return Err(LifecycleError::Unsettled);
        }
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user)? };
        let creation = u64::from(created.dwLowDateTime) | (u64::from(created.dwHighDateTime) << 32);
        if creation == 0 {
            return Err(LifecycleError::Invalid);
        }
        self.peer = Some(GuardianIdentity {
            process_id: pid,
            creation_filetime: creation,
        });
        self.room(1)?;
        if self.resources.token_sid(process)? != self.resources.sid {
            return Err(LifecycleError::Invalid);
        }
        let mut path = [0u16; PATH_LIMIT];
        let mut length = path.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            )?;
        }
        if length == 0 || length as usize >= path.len() {
            return Err(LifecycleError::Invalid);
        }
        let path = PathBuf::from(OsString::from_wide(&path[..length as usize]));
        valid_path(&path)?;
        let mut parents: Vec<_> = path.ancestors().skip(1).collect();
        parents.reverse();
        if parents.len() > 48 {
            return Err(LifecycleError::Invalid);
        }
        for ancestor in parents {
            self.room(1)?;
            let handle = self.resources.open_file(
                ancestor,
                READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                OPEN_EXISTING,
                true,
                false,
            )?;
            self.resources.add(handle, Role::Parent);
            verify_info(handle, true)?;
        }
        self.room(1)?;
        let image = self.resources.open_file(
            &path,
            GENERIC_READ.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        self.resources.add(image, Role::Parent);
        verify_info(image, false)?;
        let mut current_pid = 0;
        unsafe { GetNamedPipeClientProcessId(pipe, &mut current_pid)? };
        if current_pid != pid || unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT {
            return Err(LifecycleError::Unsettled);
        }
        Ok(image)
    }
    fn read_at(&self, file: HANDLE, offset: usize, bytes: &mut [u8]) -> Result<()> {
        self.time()?;
        let mut read = 0;
        unsafe {
            SetFilePointerEx(file, offset as i64, None, FILE_BEGIN)?;
            ReadFile(file, Some(bytes), Some(&mut read), None)?;
        }
        if read as usize != bytes.len() {
            return Err(LifecycleError::Unsettled);
        }
        Ok(())
    }
    fn template_matches(
        &mut self,
        install: MSIHANDLE,
        image: HANDLE,
        format: &abi2::PackageFormat,
    ) -> Result<()> {
        self.room(1)?;
        let database = self.register_msi(unsafe { MsiGetActiveDatabase(install) }, false)?;
        self.room(1)?;
        let mut view = MSIHANDLE(0);
        let query = HSTRING::from(GUARD_TEMPLATE_QUERY);
        if unsafe { MsiDatabaseOpenViewW(database, &query, &mut view) } != 0 {
            return Err(LifecycleError::Unsettled);
        }
        let view = self.register_msi(view, false)?;
        if unsafe { MsiViewExecute(view, MSIHANDLE(0)) } != 0 {
            return Err(LifecycleError::Unsettled);
        }
        if let Some(item) = self.msi.iter_mut().find(|item| item.handle == view) {
            item.view_open = true;
        }
        self.room(1)?;
        let mut row = MSIHANDLE(0);
        if unsafe { MsiViewFetch(view, &mut row) } != 0 {
            return Err(LifecycleError::Invalid);
        }
        let row = self.register_msi(row, false)?;
        let length = unsafe { MsiRecordDataSize(row, 1) } as usize;
        let mut image_length = 0;
        unsafe { GetFileSizeEx(image, &mut image_length)? };
        if !(69..=MAX_IMAGE).contains(&length) || image_length != length as i64 {
            return Err(LifecycleError::Invalid);
        }
        let hole = match format {
            abi2::PackageFormat::Abi2(_) => self.abi2_image_hole(image, length)?,
            abi2::PackageFormat::Legacy => {
                let mut footer = [0; 20];
                self.read_at(image, length - 20, &mut footer)?;
                let n = u32::from_le_bytes(
                    footer[..4]
                        .try_into()
                        .map_err(|_| LifecycleError::Invalid)?,
                ) as usize;
                if &footer[4..] != manifest::FOOTER
                    || n == 0
                    || n > manifest::MAX_MANIFEST
                    || n + 68 >= length
                {
                    return Err(LifecycleError::Invalid);
                }
                let seal = length - 20 - n - 48;
                let hole = seal + 16;
                let mut seal_bytes = [0; 48];
                self.read_at(image, seal, &mut seal_bytes)?;
                if &seal_bytes[..16] != SEAL_MAGIC || seal_bytes[16..].iter().all(|&b| b == 0) {
                    return Err(LifecycleError::Invalid);
                }
                if seal >= 48 {
                    let mut prior = [0; 16];
                    self.read_at(image, seal - 48, &mut prior)?;
                    if &prior == SEAL_MAGIC {
                        return Err(LifecycleError::Invalid);
                    }
                }
                let mut raw_manifest = vec![0; n];
                self.read_at(image, length - 20 - n, &mut raw_manifest)?;
                let manifest = manifest::parse(&raw_manifest)?;
                let mut product = [0u16; 40];
                let mut product_length = product.len() as u32;
                if unsafe {
                    MsiGetPropertyW(
                        install,
                        &HSTRING::from("ProductCode"),
                        Some(PWSTR(product.as_mut_ptr())),
                        Some(&mut product_length),
                    )
                } != 0
                    || product_length as usize >= product.len()
                    || String::from_utf16(&product[..product_length as usize]).ok()
                        != Some(format!("{{{}}}", manifest.product))
                {
                    return Err(LifecycleError::Invalid);
                }
                hole
            }
        };
        let mut offset = 0;
        let mut template = [0u8; 16384];
        let mut actual = [0u8; 16384];
        while offset < length {
            self.time()?;
            let count = template.len().min(length - offset);
            let mut got = count as u32;
            if unsafe { MsiRecordReadStream(row, 1, Some(PSTR(template.as_mut_ptr())), &mut got) }
                != 0
                || got as usize != count
            {
                return Err(LifecycleError::Unsettled);
            }
            self.read_at(image, offset, &mut actual[..count])?;
            for i in 0..count {
                let at = offset + i;
                if (hole..hole + 32).contains(&at) {
                    // This one fixed slot is the only normalized region.
                    if template[i] != 0 {
                        return Err(LifecycleError::Invalid);
                    }
                } else if template[i] != actual[i] {
                    return Err(LifecycleError::Invalid);
                }
            }
            offset += count;
        }
        let mut end = 1;
        if unsafe { MsiRecordReadStream(row, 1, Some(PSTR(template.as_mut_ptr())), &mut end) } != 0
            || end != 0
        {
            return Err(LifecycleError::Invalid);
        }
        self.room(1)?;
        let mut extra = MSIHANDLE(0);
        let next = unsafe { MsiViewFetch(view, &mut extra) };
        if next == 0 {
            self.register_msi(extra, false)?;
            return Err(LifecycleError::Invalid);
        }
        if next != ERROR_NO_MORE_ITEMS.0 {
            return Err(LifecycleError::Unsettled);
        }
        self.close_msi(row)?;
        self.close_msi(view)?;
        self.close_msi(database)?;
        self.time()
    }
    fn cancel_and_drain(&mut self) -> Result<()> {
        if self.io == Io::None {
            return Ok(());
        }
        let pipe = self.pipe.ok_or(LifecycleError::Invalid)?;
        // CancelIoEx's success is a request only. ERROR_NOT_FOUND can race a
        // completion; in both cases the original OVERLAPPED must be harvested.
        let cancel = unsafe { CancelIoEx(pipe, Some(&raw const self.overlapped)) };
        if let Err(error) = cancel {
            if error.code() != ERROR_NOT_FOUND.to_hresult() {
                return Err(error.into());
            }
        }
        let started = unsafe { GetTickCount64() };
        loop {
            let mut bytes = 0;
            match unsafe {
                GetOverlappedResult(pipe, &raw const self.overlapped, &mut bytes, false)
            } {
                Ok(()) => {
                    self.io = Io::None;
                    return Ok(());
                }
                Err(error) if error.code() == ERROR_OPERATION_ABORTED.to_hresult() => {
                    self.io = Io::None;
                    return Ok(());
                }
                Err(error) if error.code() == ERROR_IO_INCOMPLETE.to_hresult() => {}
                Err(error) => return Err(error.into()),
            }
            if unsafe { GetTickCount64() }.saturating_sub(started) >= CANCEL_DRAIN_MS {
                return Err(LifecycleError::Unsettled);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn finish(&mut self) -> Result<()> {
        self.cancel_and_drain()?;
        while let Some(item) = self.msi.last() {
            self.close_msi(item.handle)?;
        }
        // Keep the original CLAIMS slot until the final module reference has
        // returned too. A failed kernel close retains that loader reference.
        for index in (0..self.resources.handles.len()).rev() {
            self.resources.close(index)?;
        }
        if let Some(module) = self.module {
            if self.module_close_attempted {
                return Err(LifecycleError::Faulted);
            }
            self.module_close_attempted = true;
            unsafe { FreeLibrary(module)? };
            self.module = None;
        }
        self.resources.release()
    }
    fn run(&mut self, install: MSIHANDLE) -> Result<()> {
        self.module_reference()?;
        self.effective_user()?;
        let mut bytes = [0; 16];
        crate::SystemRandom.fill(&mut bytes)?;
        let nonce = Nonce::from_bytes(bytes)?;
        let format = self.package_format(install)?;
        let info = match &format {
            abi2::PackageFormat::Legacy => nonce.info(),
            abi2::PackageFormat::Abi2(package) => {
                let info = package.info(nonce);
                // Format roundtrip does not turn DB facts into a ticket.
                let (parsed, echoed) =
                    crate::installer_lifecycle::guard_contract::v2::ObservedPackage::parse_info(
                        &info,
                    )?;
                if &parsed != package || echoed != nonce {
                    return Err(LifecycleError::Invalid);
                }
                info
            }
        };
        let length = match &format {
            abi2::PackageFormat::Legacy => 48,
            abi2::PackageFormat::Abi2(_) => v2::FRAME_BYTES,
        };
        self.prepare_accept(nonce, length)?;
        self.message(install, &info)?;
        // The client remains held in G's original callback context until its
        // later InstallInitialize callback or negative API return. Consequently
        // the actual peer query does not depend on a closed client's metadata.
        self.receive(nonce, &format)?;
        let image = self.peer_image()?;
        self.template_matches(install, image, &format)?;
        let process = self.resources.handle(Role::Guardian)?;
        let peer = self.peer.ok_or(LifecycleError::Invalid)?;
        if unsafe { GetProcessId(process) } != peer.process_id
            || unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT
        {
            return Err(LifecycleError::Unsettled);
        }
        self.time()
    }
}

/// Windows Installer lends this actual session handle to the immediate Type 1
/// action. The caller's hInstall is never closed, retained, or accepted through
/// a property/command line. This entry remains outside the released setup route.
///
/// # Safety
/// Windows Installer must call this export with its live installation session.
#[unsafe(no_mangle)]
#[allow(non_snake_case)] // Exact Windows Installer Target export, system ABI.
pub unsafe extern "system" fn FcControlledInnerGuard(h_install: u32) -> u32 {
    if h_install == 0 {
        return MSI_FAILURE;
    }
    let Ok(mut state) = GuardState::new() else {
        return MSI_FAILURE;
    };
    // Keep the stable allocation outside the unwind boundary. A panic cannot
    // drop an outstanding OVERLAPPED or its grant buffer during stack unwinding.
    let accepted = catch_unwind(AssertUnwindSafe(|| state.run(MSIHANDLE(h_install))))
        .is_ok_and(|result| result.is_ok());
    let closed =
        catch_unwind(AssertUnwindSafe(|| state.finish())).is_ok_and(|result| result.is_ok());
    if !closed {
        // This transfers the original claim's handles into its existing fixed
        // ledger. The stable state allocation also retains original IO memory,
        // MSI IDs and extra loader reference until physical host process exit.
        // No second owner pool, reconstructed handle or retry-close exists.
        state.resources.retain_until_exit();
        let _retained_original = Box::into_raw(state);
        return MSI_FAILURE;
    }
    let now = unsafe { GetTickCount64() };
    let fresh = state.grant_deadline.is_some_and(|deadline| now < deadline)
        && now.saturating_sub(state.entry_tick) < 120_000
        && !FAULTED.load(Ordering::Acquire);
    if accepted && fresh {
        0
    } else {
        MSI_FAILURE
    }
}
