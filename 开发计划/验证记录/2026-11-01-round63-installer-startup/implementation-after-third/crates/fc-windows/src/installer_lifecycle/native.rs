use super::*;
use std::{
    marker::PhantomData,
    os::windows::io::AsRawHandle,
    path::{Component, Path},
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::*,
        Security::*,
        Storage::FileSystem::*,
        System::{Threading::*, WindowsProgramming::DRIVE_FIXED},
        UI::Shell::*,
    },
    core::{Error, HSTRING, Result as WinResult},
};

const HANDLE_LIMIT: usize = 64;
const PROCESS_OWNERS: usize = 8;
const PATH_LIMIT: usize = 1024;
static CLAIMS: AtomicU8 = AtomicU8::new(0);
static FAULTED: AtomicBool = AtomicBool::new(false);
// A failed CloseHandle never loses its original capability. The fixed ledger
// owns integer representations only after native activity has stopped. No code
// reconstructs, calls or closes these values; OS process exit is final recovery.
// No unsafe Send/Sync impl, pointer transmutation, unbounded leak or replacement.
struct Retained {
    handles: [usize; HANDLE_LIMIT],
    count: usize,
}
static RETAINED: Mutex<[Retained; PROCESS_OWNERS]> = Mutex::new(
    [const {
        Retained {
            handles: [0; HANDLE_LIMIT],
            count: 0,
        }
    }; PROCESS_OWNERS],
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Parent,
    Gate,
    Pending,
    Guardian,
    Token,
}
struct Owned {
    handle: HANDLE,
    role: Role,
}
pub(super) struct Resources {
    handles: Vec<Owned>,
    slot: Option<u8>,
    root: PathBuf,
    sid: Vec<u8>,
    pending: Vec<u8>,
    pending_identity: Option<(u32, u64)>,
    failed: bool,
    producers_returned: bool,
    #[cfg(test)]
    fail_close: bool,
    #[cfg(test)]
    fail_close_role: Option<Role>,
    _thread: PhantomData<Rc<()>>,
}
fn denied() -> Error {
    Error::from_hresult(E_ACCESSDENIED)
}
fn invalid() -> Error {
    Error::from_hresult(E_INVALIDARG)
}

impl Resources {
    fn empty() -> Result<Self> {
        if FAULTED.load(Ordering::Acquire) {
            return Err(LifecycleError::Faulted);
        }
        let mut old = CLAIMS.load(Ordering::Acquire);
        let slot = loop {
            let Some(slot) = (0..PROCESS_OWNERS).find(|i| old & (1 << i) == 0) else {
                return Err(LifecycleError::Busy);
            };
            match CLAIMS.compare_exchange_weak(
                old,
                old | (1 << slot),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break slot as u8,
                Err(next) => old = next,
            }
        };
        let mut owner = Self {
            handles: Vec::with_capacity(HANDLE_LIMIT),
            slot: Some(slot),
            root: PathBuf::new(),
            sid: Vec::new(),
            pending: Vec::new(),
            pending_identity: None,
            failed: false,
            producers_returned: false,
            #[cfg(test)]
            fail_close: false,
            #[cfg(test)]
            fail_close_role: None,
            _thread: PhantomData,
        };
        if FAULTED.load(Ordering::Acquire) {
            owner.release()?;
            return Err(LifecycleError::Faulted);
        }
        Ok(owner)
    }
    fn add(&mut self, handle: HANDLE, role: Role) -> HANDLE {
        // All callers check count before creating a native resource.
        assert!(self.handles.len() < HANDLE_LIMIT);
        self.handles.push(Owned { handle, role });
        handle
    }
    fn handle(&self, role: Role) -> Result<HANDLE> {
        self.handles
            .iter()
            .find(|h| h.role == role)
            .map(|h| h.handle)
            .ok_or(LifecycleError::Invalid)
    }
    fn token_sid(&mut self, process: HANDLE) -> WinResult<Vec<u8>> {
        if self.handles.len() >= HANDLE_LIMIT {
            return Err(invalid());
        }
        let mut token = HANDLE::default();
        // SAFETY: borrowed process, owned token immediately registered; fixed
        // aligned output keeps TOKEN_USER/SID pointers alive during the copy.
        unsafe {
            OpenProcessToken(process, TOKEN_QUERY, &mut token)?;
        }
        self.add(token, Role::Token);
        let mut buffer = [0u64; 512];
        let mut size = 0;
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                std::mem::size_of_val(&buffer) as u32,
                &mut size,
            )?;
            if size as usize > std::mem::size_of_val(&buffer)
                || (size as usize) < std::mem::size_of::<TOKEN_USER>()
            {
                return Err(denied());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let start = user.User.Sid.0 as usize;
            let base = buffer.as_ptr() as usize;
            if start < base
                || start
                    .checked_add(8)
                    .is_none_or(|end| end > base + size as usize)
            {
                return Err(denied());
            }
            let len = 8 + 4 * usize::from(*((start + 1) as *const u8));
            if len > 68
                || start
                    .checked_add(len)
                    .is_none_or(|end| end > base + size as usize)
                || !IsValidSid(user.User.Sid).as_bool()
            {
                return Err(denied());
            }
            Ok(std::slice::from_raw_parts(start as *const u8, len).to_vec())
        }
    }
    pub(super) fn check_live(&self) -> Result<()> {
        if self.failed || FAULTED.load(Ordering::Acquire) {
            return Err(LifecycleError::Faulted);
        }
        if self.slot.is_none() {
            return Err(LifecycleError::Invalid);
        }
        Ok(())
    }
    pub(super) fn open(identity: &InstallIdentity, exclusive: bool) -> Result<Self> {
        let mut owner = Self::empty()?;
        // SAFETY: current-process pseudohandle is borrowed, never registered/closed.
        owner.sid = owner.token_sid(unsafe { GetCurrentProcess() })?;
        require_user(&owner.sid)?;
        if owner.sid != identity.sid {
            return Err(LifecycleError::Invalid);
        }
        valid_path(&identity.local)?;
        // Every ancestor is pinned without DELETE sharing before the next
        // child opens. Reparse points are opened themselves and then rejected.
        let mut parents: Vec<_> = identity.local.ancestors().collect();
        parents.reverse();
        if parents.len() > 48 {
            return Err(LifecycleError::Invalid);
        }
        for path in parents {
            let h = owner.open_file(
                path,
                READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                OPEN_EXISTING,
                true,
                false,
            )?;
            owner.add(h, Role::Parent);
            verify_info(h, true)?;
        }
        let local_handle = owner.handles.last().ok_or(LifecycleError::Invalid)?.handle;
        verify_acl(local_handle, &owner.sid, false)?;
        #[cfg(test)]
        let leaf = identity
            .leaf
            .as_deref()
            .unwrap_or("FutureClassroom-Installer-99378807-098D-5F04-8D4D-F181DFA218D5");
        #[cfg(not(test))]
        let leaf = "FutureClassroom-Installer-99378807-098D-5F04-8D4D-F181DFA218D5";
        owner.root = identity.local.join(leaf);
        let name = native_path(&owner.root)?;
        let created = with_security(&owner.sid, true, |attributes| unsafe {
            CreateDirectoryW(&name, Some(attributes))
        });
        if let Err(error) = created {
            if error.code() != ERROR_ALREADY_EXISTS.to_hresult() {
                return Err(error.into());
            }
        }
        let h = owner.open_file(
            &owner.root,
            READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            true,
            false,
        )?;
        owner.add(h, Role::Parent);
        verify_info(h, true)?;
        verify_acl(h, &owner.sid, true)?;
        let access = GENERIC_READ.0 | READ_CONTROL.0 | if exclusive { GENERIC_WRITE.0 } else { 0 };
        let gate = owner.open_file(
            &owner.root.join("gate.lock"),
            access,
            if exclusive {
                FILE_SHARE_MODE(0)
            } else {
                FILE_SHARE_READ
            },
            OPEN_ALWAYS,
            false,
            true,
        )?;
        owner.add(gate, Role::Gate);
        verify_info(gate, false)?;
        verify_acl(gate, &owner.sid, true)?;
        owner.absent_pending()?;
        owner.check_live()?;
        Ok(owner)
    }
    fn open_file(
        &self,
        path: &Path,
        access: u32,
        share: FILE_SHARE_MODE,
        disposition: FILE_CREATION_DISPOSITION,
        directory: bool,
        protected: bool,
    ) -> WinResult<HANDLE> {
        if self.handles.len() >= HANDLE_LIMIT {
            return Err(invalid());
        }
        let path = native_path(path)?;
        let flags = FILE_FLAG_OPEN_REPARSE_POINT
            | if directory {
                FILE_FLAG_BACKUP_SEMANTICS
            } else {
                FILE_ATTRIBUTE_NORMAL
            };
        // SAFETY: synchronous fixed local path and optional stack descriptor;
        // every successful handle is immediately registered by its caller.
        let open = |sa: Option<*const SECURITY_ATTRIBUTES>| unsafe {
            CreateFileW(&path, access, share, sa, disposition, flags, None)
        };
        if protected {
            with_security(&self.sid, directory, |sa| open(Some(sa)))
        } else {
            open(None)
        }
    }
    fn absent_pending(&mut self) -> Result<()> {
        match self.open_file(
            &self.root.join("pending.v1"),
            GENERIC_READ.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        ) {
            Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
            Err(error) => Err(error.into()),
            Ok(handle) => {
                self.add(handle, Role::Pending);
                Err(LifecycleError::Pending)
            }
        }
    }
    pub(super) fn create_pending(
        &mut self,
        identity: &InstallIdentity,
        operation: MaintenanceOperation,
    ) -> Result<OperationId> {
        self.check_live()?;
        use fc_ports::RandomSource;
        let mut nonce = [0; 16];
        crate::SystemRandom.fill(&mut nonce)?;
        let id = OperationId(nonce);
        let mut bytes = b"FC-INSTALL-PENDING\0v1\0".to_vec();
        bytes.extend_from_slice(INSTALL_FAMILY.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&nonce);
        bytes.push(operation.tag());
        bytes.extend_from_slice(&(identity.sid.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&identity.sid);
        if bytes.len() > MAX_PENDING_BYTES {
            return Err(LifecycleError::Invalid);
        }
        let handle = self.open_file(
            &self.root.join("pending.v1"),
            GENERIC_READ.0 | GENERIC_WRITE.0 | DELETE.0 | READ_CONTROL.0,
            FILE_SHARE_MODE(0),
            CREATE_NEW,
            false,
            true,
        )?;
        self.add(handle, Role::Pending);
        self.pending_identity = Some(verify_info(handle, false)?);
        verify_acl(handle, &self.sid, true)?;
        self.pending = bytes;
        let mut written = 0;
        // SAFETY: synchronous IO on unique original handle, <=4KiB stable bytes.
        unsafe {
            WriteFile(handle, Some(&self.pending), Some(&mut written), None)?;
        }
        if written as usize != self.pending.len() {
            return Err(LifecycleError::Unsettled);
        }
        unsafe {
            FlushFileBuffers(handle)?;
        }
        self.verify_pending()?;
        Ok(id)
    }
    fn verify_pending(&self) -> Result<()> {
        let handle = self.handle(Role::Pending)?;
        if self.pending_identity != Some(verify_info(handle, false)?) {
            return Err(LifecycleError::Unsettled);
        }
        verify_acl(handle, &self.sid, true)?;
        let mut size = 0;
        unsafe {
            GetFileSizeEx(handle, &mut size)?;
        }
        if size < 1 || size as usize > MAX_PENDING_BYTES || size as usize != self.pending.len() {
            return Err(LifecycleError::Unsettled);
        }
        let mut bytes = [0; MAX_PENDING_BYTES];
        let mut read = 0;
        unsafe {
            SetFilePointerEx(handle, 0, None, FILE_BEGIN)?;
            ReadFile(
                handle,
                Some(&mut bytes[..size as usize]),
                Some(&mut read),
                None,
            )?;
        }
        if read as usize != size as usize || bytes[..size as usize] != self.pending {
            return Err(LifecycleError::Unsettled);
        }
        Ok(())
    }
    pub(super) fn watch_guardian(
        &mut self,
        child: &std::process::Child,
    ) -> Result<GuardianIdentity> {
        if self.handles.len() >= HANDLE_LIMIT {
            return Err(LifecycleError::Invalid);
        }
        let mut process = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                HANDLE(child.as_raw_handle()),
                GetCurrentProcess(),
                &mut process,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )?;
        }
        self.add(process, Role::Guardian);
        let sid = self.token_sid(process)?;
        if sid != self.sid {
            return Err(LifecycleError::Invalid);
        }
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe {
            GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user)?;
        }
        let process_id = unsafe { GetProcessId(process) };
        if process_id == 0 || process_id != child.id() {
            return Err(LifecycleError::Invalid);
        }
        Ok(GuardianIdentity {
            process_id,
            creation_filetime: (u64::from(created.dwHighDateTime) << 32)
                | u64::from(created.dwLowDateTime),
        })
    }
    pub(super) fn guardian_returned(&self) -> Result<()> {
        let process = self.handle(Role::Guardian)?;
        let result = unsafe { WaitForSingleObject(process, 0) };
        if result == WAIT_TIMEOUT {
            return Err(LifecycleError::GuardianRunning);
        }
        if result != WAIT_OBJECT_0 {
            return Err(LifecycleError::Unsettled);
        }
        let mut code = 0;
        unsafe {
            GetExitCodeProcess(process, &mut code)?;
        }
        if code != 0 {
            return Err(LifecycleError::Unsettled);
        }
        Ok(())
    }
    pub(super) fn clear_pending(&mut self, deadline: Instant) -> Result<()> {
        if self.producers_returned {
            return Err(LifecycleError::Unsettled);
        }
        self.verify_pending()?;
        // Release every non-gate/non-pending native resource before positive
        // deletion. A failing close keeps Pending + gate and faults this owner.
        for i in (0..self.handles.len()).rev() {
            if !matches!(
                self.handles[i].role,
                Role::Gate | Role::Pending | Role::Parent
            ) {
                self.close(i)?;
            }
        }
        // Negative commitpoint: exact End/effects and original guardian exit
        // were checked by MaintenanceOwner; every producer handle above has
        // now actually returned. Only original Pending/Parent/Gate controls
        // remain. Keep pins and delete THIS inode, never a reopened path.
        self.producers_returned = true;
        if Instant::now() >= deadline {
            return Err(LifecycleError::Expired);
        }
        let index = self
            .handles
            .iter()
            .position(|h| h.role == Role::Pending)
            .ok_or(LifecycleError::Invalid)?;
        let handle = self.handles[index].handle;
        let deletion = FILE_DISPOSITION_INFO {
            DeleteFile: true.into(),
        };
        unsafe {
            SetFileInformationByHandle(
                handle,
                FileDispositionInfo,
                (&raw const deletion).cast(),
                std::mem::size_of_val(&deletion) as u32,
            )?;
        }
        self.close(index)?;
        self.pending_identity = None;
        self.absent_pending()?;
        Ok(())
    }
    fn close(&mut self, index: usize) -> Result<()> {
        #[cfg(test)]
        if self.fail_close || self.fail_close_role == Some(self.handles[index].role) {
            self.failed = true;
            FAULTED.store(true, Ordering::Release);
            return Err(LifecycleError::Faulted);
        }
        // SAFETY: uniquely registered successful handle, removed only on checked
        // CloseHandle success. Error never becomes Released or a replacement.
        if let Err(error) = unsafe { CloseHandle(self.handles[index].handle) } {
            self.failed = true;
            FAULTED.store(true, Ordering::Release);
            return Err(error.into());
        }
        self.handles.remove(index);
        Ok(())
    }
    pub(super) fn release(&mut self) -> Result<()> {
        if self.failed {
            return Err(LifecycleError::Faulted);
        }
        // This only releases native ownership. It NEVER deletes Pending; owner
        // cancellation/Drop leaves the negative record even after handle return.
        for i in (0..self.handles.len()).rev() {
            if self.handles[i].role != Role::Gate {
                self.close(i)?;
            }
        }
        if let Some(index) = self.handles.iter().position(|h| h.role == Role::Gate) {
            self.close(index)?;
        }
        if let Some(slot) = self.slot.take() {
            CLAIMS.fetch_and(!(1 << slot), Ordering::AcqRel);
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn inject_close_failure(&mut self) {
        self.fail_close = true;
    }
    #[cfg(test)]
    pub(super) fn inject_close_failure_at(&mut self, role: &str) {
        self.fail_close_role = Some(match role {
            "guardian" => Role::Guardian,
            "pending" => Role::Pending,
            "parent" => Role::Parent,
            "gate" => Role::Gate,
            _ => panic!("fixed test close role"),
        });
    }
    #[cfg(test)]
    pub(super) fn producers_returned(&self) -> bool {
        self.producers_returned
    }
    #[cfg(test)]
    pub(super) fn root(&self) -> &Path {
        &self.root
    }
    pub(super) fn released(&self) -> bool {
        self.slot.is_none() && self.handles.is_empty()
    }
}
impl Drop for Resources {
    fn drop(&mut self) {
        if self.release().is_ok() {
            return;
        }
        FAULTED.store(true, Ordering::Release);
        let Some(slot) = self.slot.take() else {
            return;
        };
        // Poisoned metadata remains accessible for negative retention only; no
        // recovery, native reuse or positive release is inferred from poison.
        let mut ledger = RETAINED.lock().unwrap_or_else(|poison| poison.into_inner());
        let target = &mut ledger[usize::from(slot)];
        assert_eq!(target.count, 0, "unique physical retention slot");
        for owned in &self.handles {
            target.handles[target.count] = owned.handle.0 as usize;
            target.count += 1;
        }
        self.handles.clear(); // raw capabilities now belong to fixed ledger
        // Original claim bit deliberately remains set forever.
    }
}

pub(super) fn identity() -> Result<InstallIdentity> {
    let mut resources = Resources::empty()?;
    let sid = resources.token_sid(unsafe { GetCurrentProcess() })?;
    require_user(&sid)?;
    let local = crate::com_task::with_task_memory(|memory| unsafe {
        let raw = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None)?;
        let value = raw.to_string();
        memory.free(raw.0.cast());
        Ok(PathBuf::from(value?))
    })?;
    valid_path(&local)?;
    resources.release()?;
    Ok(InstallIdentity {
        local,
        sid,
        #[cfg(test)]
        leaf: None,
    })
}
fn valid_path(path: &Path) -> WinResult<()> {
    let text = path.to_str().ok_or_else(invalid)?;
    let raw = text.as_bytes();
    if raw.len() < 3
        || !raw[0].is_ascii_alphabetic()
        || &raw[1..3] != b":\\"
        || text[2..].contains([':', '\0', '/'])
        || text.encode_utf16().count() >= PATH_LIMIT
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid());
    }
    let drive = HSTRING::from(&text[..3]);
    if unsafe { GetDriveTypeW(&drive) } != DRIVE_FIXED {
        return Err(denied());
    }
    Ok(())
}
fn native_path(path: &Path) -> WinResult<HSTRING> {
    valid_path(path)?;
    // Fixed absolute paths only, no user text/property. Extended prefix avoids
    // Win7 MAX_PATH truncation; reject DOS trailing-dot/space alias components.
    if path
        .components()
        .any(|c| c.as_os_str().to_string_lossy().ends_with([' ', '.']))
    {
        return Err(invalid());
    }
    Ok(HSTRING::from(format!("\\\\?\\{}", path.display())))
}
fn verify_info(handle: HANDLE, directory: bool) -> WinResult<(u32, u64)> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        GetFileInformationByHandle(handle, &mut info)?;
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0) != directory
        || (!directory && info.nNumberOfLinks != 1)
    {
        return Err(denied());
    }
    Ok((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}
fn system_sid(buffer: &mut [u64; 16]) -> WinResult<PSID> {
    let sid = PSID(buffer.as_mut_ptr().cast());
    let mut len = std::mem::size_of_val(buffer) as u32;
    unsafe {
        CreateWellKnownSid(WinLocalSystemSid, None, Some(sid), &mut len)?;
    }
    Ok(sid)
}
fn with_security<T>(
    sid: &[u8],
    directory: bool,
    operation: impl FnOnce(*const SECURITY_ATTRIBUTES) -> WinResult<T>,
) -> WinResult<T> {
    let mut descriptor = SECURITY_DESCRIPTOR::default();
    let sd = PSECURITY_DESCRIPTOR((&raw mut descriptor).cast());
    let mut acl_storage = [0u64; 64];
    let acl = acl_storage.as_mut_ptr().cast::<ACL>();
    let mut system = [0u64; 16];
    let system = system_sid(&mut system)?;
    let user = PSID(sid.as_ptr().cast_mut().cast());
    let ace_flags = if directory {
        OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
    } else {
        ACE_FLAGS(0)
    };
    unsafe {
        InitializeSecurityDescriptor(sd, 1)?;
        InitializeAcl(
            acl,
            std::mem::size_of_val(&acl_storage) as u32,
            ACL_REVISION,
        )?;
        AddAccessAllowedAceEx(acl, ACL_REVISION, ace_flags, FILE_ALL_ACCESS.0, user)?;
        AddAccessAllowedAceEx(acl, ACL_REVISION, ace_flags, FILE_ALL_ACCESS.0, system)?;
        SetSecurityDescriptorOwner(sd, Some(user), false)?;
        SetSecurityDescriptorDacl(sd, true, Some(acl), false)?;
        SetSecurityDescriptorControl(sd, SE_DACL_PROTECTED, SE_DACL_PROTECTED)?;
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    operation(&attributes)
}
// SID equality is a predicate, not a fallible Win32 operation: EqualSid
// false need not set LastError. Never propagate its binding error (HRESULT 0)
// as a native failure. The system-known parent shares the same trusted owner
// boundary as its DACL grants; our own namespace remains current-SID only.
unsafe fn verify_owner(
    owner: PSID,
    user: PSID,
    system: PSID,
    admin: PSID,
    exact: bool,
) -> WinResult<()> {
    // SAFETY: callers retain actual OS descriptor and validated SID buffers.
    unsafe {
        if owner.0.is_null() || !IsValidSid(owner).as_bool() {
            return Err(denied());
        }
        if EqualSid(owner, user).is_ok()
            || (!exact && (EqualSid(owner, system).is_ok() || EqualSid(owner, admin).is_ok()))
        {
            Ok(())
        } else {
            Err(denied())
        }
    }
}

fn verify_acl(handle: HANDLE, sid: &[u8], exact: bool) -> WinResult<()> {
    let mut storage = [0u64; 512];
    let mut len = 0;
    let sd = PSECURITY_DESCRIPTOR(storage.as_mut_ptr().cast());
    // Handle-based query avoids following a new path between ACL check/open.
    unsafe {
        GetKernelObjectSecurity(
            handle,
            (OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION).0,
            Some(sd),
            std::mem::size_of_val(&storage) as u32,
            &mut len,
        )?;
    }
    if len as usize > std::mem::size_of_val(&storage) {
        return Err(denied());
    }
    let mut owner = PSID::default();
    let mut defaulted = windows::core::BOOL::default();
    let mut present = windows::core::BOOL::default();
    let mut acl = std::ptr::null_mut();
    let user = PSID(sid.as_ptr().cast_mut().cast());
    unsafe {
        GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted)?;
        let mut system = [0u64; 16];
        let system = system_sid(&mut system)?;
        let mut admins = [0u64; 16];
        let mut admin_len = std::mem::size_of_val(&admins) as u32;
        let admin = PSID(admins.as_mut_ptr().cast());
        CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            None,
            Some(admin),
            &mut admin_len,
        )?;
        verify_owner(owner, user, system, admin, exact)?;
        GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted)?;
        if !present.as_bool() || acl.is_null() || !IsValidAcl(acl).as_bool() {
            return Err(denied());
        }
        if !exact {
            // Parent is the OS-resolved LocalAppData, not ours to repair. It
            // may inherit read ACEs, but must not give another ordinary SID
            // write/delete/ACL-owner rights to the fixed coordination child.
            let dangerous = GENERIC_ALL.0
                | GENERIC_WRITE.0
                | DELETE.0
                | WRITE_DAC.0
                | WRITE_OWNER.0
                | FILE_WRITE_DATA.0
                | FILE_APPEND_DATA.0
                | FILE_DELETE_CHILD.0
                | FILE_WRITE_EA.0
                | FILE_WRITE_ATTRIBUTES.0;
            for index in 0..u32::from((*acl).AceCount) {
                let mut raw = std::ptr::null_mut();
                GetAce(acl, index, &mut raw)?;
                let header = std::ptr::read_unaligned(raw.cast::<ACE_HEADER>());
                if header.AceType != 0 || header.AceSize < 16 {
                    return Err(denied());
                }
                let bytes = raw.cast::<u8>();
                if 16 + 4 * usize::from(*bytes.add(9)) > usize::from(header.AceSize) {
                    return Err(denied());
                }
                let target = PSID(bytes.add(8).cast());
                if !IsValidSid(target).as_bool() {
                    return Err(denied());
                }
                let mask = (*raw.cast::<ACCESS_ALLOWED_ACE>()).Mask;
                if header.AceFlags & INHERIT_ONLY_ACE.0 as u8 == 0
                    && mask & dangerous != 0
                    && EqualSid(target, user).is_err()
                    && EqualSid(target, system).is_err()
                    && EqualSid(target, admin).is_err()
                {
                    return Err(denied());
                }
            }
            return Ok(());
        }
        let mut control = 0;
        let mut revision = 0;
        GetSecurityDescriptorControl(sd, &mut control, &mut revision)?;
        if control & SE_DACL_PROTECTED.0 == 0 || (*acl).AceCount != 2 {
            return Err(denied());
        }
        let mut sys = [0u64; 16];
        let sys = system_sid(&mut sys)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        GetFileInformationByHandle(handle, &mut info)?;
        let expected_flags = if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
            (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE).0 as u8
        } else {
            0
        };
        let mut user_seen = false;
        let mut system_seen = false;
        for index in 0..2 {
            let mut ace = std::ptr::null_mut();
            GetAce(acl, index, &mut ace)?;
            let header = std::ptr::read_unaligned(ace.cast::<ACE_HEADER>());
            if header.AceType != 0 /* ACCESS_ALLOWED_ACE_TYPE */ || header.AceSize < 16 {
                return Err(denied());
            }
            let bytes = ace.cast::<u8>();
            let count = usize::from(*bytes.add(9));
            if 16 + 4 * count > usize::from(header.AceSize) {
                return Err(denied());
            }
            let entry = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            let target = PSID(bytes.add(8).cast());
            if !IsValidSid(target).as_bool()
                || entry.Mask != FILE_ALL_ACCESS.0
                || header.AceFlags != expected_flags
            {
                return Err(denied());
            }
            if EqualSid(target, user).is_ok() && !user_seen {
                user_seen = true;
            } else if EqualSid(target, sys).is_ok() && !system_seen {
                system_seen = true;
            } else {
                return Err(denied());
            }
        }
        if !user_seen || !system_seen {
            return Err(denied());
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn retained_count() -> usize {
    RETAINED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .map(|record| record.count)
        .sum()
}

#[cfg(test)]
pub(super) fn make_unprotected_for_test(path: &Path) -> Result<()> {
    let mut resources = Resources::empty()?;
    resources.sid = resources.token_sid(unsafe { GetCurrentProcess() })?;
    let h = resources.open_file(
        path,
        WRITE_DAC.0 | READ_CONTROL.0,
        FILE_SHARE_MODE(0),
        OPEN_EXISTING,
        false,
        false,
    )?;
    resources.add(h, Role::Parent);
    let mut descriptor = SECURITY_DESCRIPTOR::default();
    let sd = PSECURITY_DESCRIPTOR((&raw mut descriptor).cast());
    // Private test inode: explicitly install a NULL DACL to verify that an
    // existing namespace with broad access is rejected, never silently repaired.
    unsafe {
        InitializeSecurityDescriptor(sd, 1)?;
        SetSecurityDescriptorDacl(sd, true, None, false)?;
        SetKernelObjectSecurity(
            h,
            DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
            sd,
        )?;
    }
    resources.release()
}

fn require_user(sid: &[u8]) -> WinResult<()> {
    let user = PSID(sid.as_ptr().cast_mut().cast());
    // A service/SYSTEM deployment must not silently acquire a different user's
    // namespace. Administrators running as their own ordinary SID remain valid.
    for kind in [WinLocalSystemSid, WinLocalServiceSid, WinNetworkServiceSid] {
        let mut storage = [0u64; 16];
        let mut len = std::mem::size_of_val(&storage) as u32;
        let service = PSID(storage.as_mut_ptr().cast());
        unsafe {
            CreateWellKnownSid(kind, None, Some(service), &mut len)?;
            if EqualSid(user, service).is_ok() {
                return Err(denied());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn isolated_delete_child_acl_is_rejected(path: &Path) -> Result<bool> {
    let mut resources = Resources::empty()?;
    resources.sid = resources.token_sid(unsafe { GetCurrentProcess() })?;
    let handle = resources.open_file(
        path,
        WRITE_DAC.0 | READ_CONTROL.0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        OPEN_EXISTING,
        true,
        false,
    )?;
    resources.add(handle, Role::Parent);
    with_security(&resources.sid, true, |attributes| unsafe {
        let descriptor = PSECURITY_DESCRIPTOR((*attributes).lpSecurityDescriptor);
        let mut present = windows::core::BOOL::default();
        let mut defaulted = windows::core::BOOL::default();
        let mut acl = std::ptr::null_mut();
        GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted)?;
        let mut world = [0u64; 16];
        let mut len = std::mem::size_of_val(&world) as u32;
        let world = PSID(world.as_mut_ptr().cast());
        CreateWellKnownSid(WinWorldSid, None, Some(world), &mut len)?;
        AddAccessAllowedAceEx(acl, ACL_REVISION, ACE_FLAGS(0), FILE_DELETE_CHILD.0, world)?;
        SetKernelObjectSecurity(
            handle,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    })?;
    let rejected = verify_acl(handle, &resources.sid, false).is_err();
    resources.release()?;
    Ok(rejected)
}

#[cfg(test)]
pub(super) fn owner_boundary_for_test() -> Result<()> {
    let mut resources = Resources::empty()?;
    resources.sid = resources.token_sid(unsafe { GetCurrentProcess() })?;
    let user = PSID(resources.sid.as_ptr().cast_mut().cast());
    let mut system = [0u64; 16];
    let system = system_sid(&mut system)?;
    let mut admin_storage = [0u64; 16];
    let mut admin_len = std::mem::size_of_val(&admin_storage) as u32;
    let admin = PSID(admin_storage.as_mut_ptr().cast());
    let mut world_storage = [0u64; 16];
    let mut world_len = std::mem::size_of_val(&world_storage) as u32;
    let world = PSID(world_storage.as_mut_ptr().cast());
    unsafe {
        CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            None,
            Some(admin),
            &mut admin_len,
        )?;
        CreateWellKnownSid(WinWorldSid, None, Some(world), &mut world_len)?;
        // Valid parent owners, including the actual Win11 LocalAppData case.
        for owner in [user, system, admin] {
            verify_owner(owner, user, system, admin, false)?;
        }
        verify_owner(user, user, system, admin, true)?;
        // Root/Gate/Pending do not inherit the broader parent-owner allowance.
        for owner in [system, admin, world] {
            for exact in [false, true] {
                if !exact && (EqualSid(owner, system).is_ok() || EqualSid(owner, admin).is_ok()) {
                    continue;
                }
                let error = verify_owner(owner, user, system, admin, exact)
                    .expect_err("nonmatching owner must refuse");
                assert_eq!(error.code(), E_ACCESSDENIED, "inequality never HRESULT 0");
            }
        }
    }
    resources.release()
}
