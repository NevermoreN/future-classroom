//! The local pipe transports facts only. Same SID/image is necessary but not
//! sufficient: Begin is the original server parent; the supervisor pins the same
//! engine and its guardian must have that exact supervisor parent. MSI itself
//! validates actual child Join ownership; only the original exit settles it.
use super::super::broker::{
    manifest::{self, Manifest},
    Role as EntryRole,
};
use super::*;
use std::{collections::BTreeSet, ffi::OsString, os::windows::ffi::OsStringExt, time::Duration};
use windows::Win32::{
    Security::Cryptography::*,
    System::{
        ApplicationInstallationAndServicing::*, Diagnostics::ToolHelp::*, Pipes::*, Registry::*,
        SystemInformation::GetWindowsDirectoryW,
    },
};

#[cfg(test)]
mod context_probe;
#[allow(dead_code)] // Fixed Setup entry/inner guard are not released by this slice.
pub(crate) mod controlled_owner;
mod effects;
#[allow(dead_code)] // Read-only capture foundation; not wired to an upgrade permit.
mod installed_release;
mod retirement;
mod supervisor;

const FRAME: usize = 32;
const BEGIN: u8 = 1;
const GUARDIAN: u8 = 2;
const ENDED: u8 = 3;
const SUPERVISOR: u8 = 4;
const ACK: u8 = 128;
const DECISION: u8 = 129;
const FINISHED: u8 = 130;
const RELEASED: u8 = 131;
const RECEIVED: u8 = 132;
const MAX_CONNECTIONS: usize = 12;

#[derive(Clone, Copy, Debug)]
struct Packet {
    kind: u8,
    outcome: u8,
    transaction: u32,
    join: u32,
    end: u32,
}
impl Packet {
    fn new(kind: u8) -> Self {
        Self {
            kind,
            outcome: 0,
            transaction: 0,
            join: 0,
            end: 0,
        }
    }
    fn encode(self) -> [u8; FRAME] {
        let mut b = [0; FRAME];
        b[..4].copy_from_slice(b"FCI1");
        b[4] = self.kind;
        b[5] = self.outcome;
        b[8..12].copy_from_slice(&self.transaction.to_le_bytes());
        b[12..16].copy_from_slice(&self.join.to_le_bytes());
        b[16..20].copy_from_slice(&self.end.to_le_bytes());
        b
    }
    fn decode(b: [u8; FRAME]) -> Result<Self> {
        if &b[..4] != b"FCI1" || b[6..8] != [0, 0] || b[20..].iter().any(|&x| x != 0) {
            return Err(LifecycleError::Invalid);
        }
        let p = Self {
            kind: b[4],
            outcome: b[5],
            transaction: u32::from_le_bytes(b[8..12].try_into().unwrap()),
            join: u32::from_le_bytes(b[12..16].try_into().unwrap()),
            end: u32::from_le_bytes(b[16..20].try_into().unwrap()),
        };
        let good = match p.kind {
            BEGIN | ACK | FINISHED | RELEASED | RECEIVED => {
                p.outcome == 0 && p.transaction == 0 && p.join == 0 && p.end == 0
            }
            GUARDIAN | SUPERVISOR => {
                p.outcome == 0 && p.transaction != 0 && p.join == 0 && p.end == 0
            }
            ENDED => matches!(p.outcome, 1 | 2) && p.transaction != 0,
            DECISION => {
                matches!(p.outcome, 1 | 2) && p.transaction != 0 && p.join == 0 && p.end == 0
            }
            _ => false,
        };
        if good {
            Ok(p)
        } else {
            Err(LifecycleError::Invalid)
        }
    }
}

fn engine_parent_sid_allowed(engine_sid: &[u8], user_sid: &[u8]) -> bool {
    // Binary S-1-5-18, including the exact revision/count/authority/length.
    const LOCAL_SYSTEM: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
    engine_sid == user_sid || engine_sid == LOCAL_SYSTEM
}

fn current(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        Err(LifecycleError::Expired)
    } else if FAULTED.load(Ordering::Acquire) {
        Err(LifecycleError::Faulted)
    } else {
        Ok(())
    }
}
fn pause(deadline: Instant) -> Result<()> {
    current(deadline)?;
    std::thread::sleep(Duration::from_millis(2));
    current(deadline)
}
fn process_identity(process: HANDLE) -> Result<GuardianIdentity> {
    let mut c = FILETIME::default();
    let mut e = FILETIME::default();
    let mut k = FILETIME::default();
    let mut u = FILETIME::default();
    unsafe {
        GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u)?;
    }
    let process_id = unsafe { GetProcessId(process) };
    if process_id == 0 {
        return Err(LifecycleError::Invalid);
    }
    Ok(GuardianIdentity {
        process_id,
        creation_filetime: u64::from(c.dwLowDateTime) | (u64::from(c.dwHighDateTime) << 32),
    })
}
fn live(process: HANDLE) -> Result<()> {
    match unsafe { WaitForSingleObject(process, 0) } {
        WAIT_TIMEOUT => Ok(()),
        _ => Err(LifecycleError::Unsettled),
    }
}
fn duplicate_process(resources: &mut Resources, process: HANDLE, role: Role) -> Result<HANDLE> {
    if resources.handles.len() >= HANDLE_LIMIT {
        return Err(LifecycleError::Invalid);
    }
    let mut duplicate = HANDLE::default();
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            process,
            GetCurrentProcess(),
            &mut duplicate,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )?;
    }
    Ok(resources.add(duplicate, role))
}
fn returned_zero(process: HANDLE, deadline: Instant) -> Result<()> {
    loop {
        current(deadline)?;
        match unsafe { WaitForSingleObject(process, 0) } {
            WAIT_OBJECT_0 => {
                let mut code = u32::MAX;
                unsafe {
                    GetExitCodeProcess(process, &mut code)?;
                }
                return if code == 0 {
                    Ok(())
                } else {
                    Err(LifecycleError::Unsettled)
                };
            }
            WAIT_TIMEOUT => pause(deadline)?,
            _ => return Err(LifecycleError::Unsettled),
        }
    }
}
fn image_path(process: HANDLE) -> Result<PathBuf> {
    let mut b = [0u16; PATH_LIMIT];
    let mut n = b.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(b.as_mut_ptr()),
            &mut n,
        )?;
    }
    if n == 0 || n as usize >= b.len() {
        return Err(LifecycleError::Invalid);
    }
    let path = PathBuf::from(OsString::from_wide(&b[..n as usize]));
    valid_path(&path)?;
    Ok(path)
}

impl Resources {
    fn close_handle(&mut self, h: HANDLE) -> Result<()> {
        let index = self
            .handles
            .iter()
            .position(|v| v.handle == h)
            .ok_or(LifecycleError::Invalid)?;
        self.close(index)
    }
    fn process(&mut self, pid: u32, role: Role) -> Result<HANDLE> {
        if pid == 0 || self.handles.len() >= HANDLE_LIMIT {
            return Err(LifecycleError::Invalid);
        }
        let h = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )?
        };
        self.add(h, role);
        Ok(h)
    }
    fn parent(&mut self, child: HANDLE) -> Result<HANDLE> {
        let child_id = process_identity(child)?;
        if self.handles.len() >= HANDLE_LIMIT {
            return Err(LifecycleError::Invalid);
        }
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? };
        self.add(snapshot, Role::Control);
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        let mut next = unsafe { Process32FirstW(snapshot, &mut entry) };
        for _ in 0..8192 {
            if next.is_err() {
                break;
            }
            if entry.th32ProcessID == child_id.process_id {
                found = Some(entry.th32ParentProcessID);
                break;
            }
            next = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        self.close_handle(snapshot)?;
        let parent = self.process(found.ok_or(LifecycleError::Invalid)?, Role::Engine)?;
        let identity = process_identity(parent)?;
        if identity.creation_filetime >= child_id.creation_filetime {
            return Err(LifecycleError::Invalid);
        }
        live(parent)?;
        Ok(parent)
    }
    fn read_at(&self, h: HANDLE, offset: i64, bytes: &mut [u8]) -> Result<()> {
        let mut read = 0;
        unsafe {
            SetFilePointerEx(h, offset, None, FILE_BEGIN)?;
            ReadFile(h, Some(bytes), Some(&mut read), None)?;
        }
        if read as usize != bytes.len() {
            return Err(LifecycleError::Unsettled);
        }
        Ok(())
    }
    fn digest(&mut self, h: HANDLE, limit: u64, deadline: Instant) -> Result<[u8; 32]> {
        self.digest_file(h, limit, deadline, false)
    }
    // Only the private damaged-preimage reader accepts an actually zero-byte
    // regular file. Existing image/manifest callers keep their nonempty guard.
    fn digest_preimage(&mut self, h: HANDLE, limit: u64, deadline: Instant) -> Result<[u8; 32]> {
        self.digest_file(h, limit, deadline, true)
    }
    fn digest_file(
        &mut self,
        h: HANDLE,
        limit: u64,
        deadline: Instant,
        allow_empty: bool,
    ) -> Result<[u8; 32]> {
        if self.handles.len() + 2 > HANDLE_LIMIT {
            return Err(LifecycleError::Invalid);
        }
        let mut size = 0;
        unsafe {
            GetFileSizeEx(h, &mut size)?;
        }
        if size < 0 || (!allow_empty && size == 0) || size as u64 > limit {
            return Err(LifecycleError::Invalid);
        }
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        unsafe {
            BCryptOpenAlgorithmProvider(
                &mut algorithm,
                BCRYPT_SHA256_ALGORITHM,
                None,
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
            .ok()?;
        }
        let algorithm_handle = self.add(HANDLE(algorithm.0), Role::Algorithm);
        let mut hash = BCRYPT_HASH_HANDLE::default();
        // Windows 7+ CNG owns the hash-object storage when pbHashObject=NULL.
        unsafe {
            BCryptCreateHash(algorithm, &mut hash, None, None, 0).ok()?;
        }
        let hash_handle = self.add(HANDLE(hash.0), Role::Hash);
        unsafe {
            SetFilePointerEx(h, 0, None, FILE_BEGIN)?;
        }
        let mut buffer = [0u8; 16384];
        let mut remaining = size as usize;
        while remaining != 0 {
            current(deadline)?;
            let mut n = 0;
            let count = buffer.len().min(remaining);
            unsafe {
                ReadFile(h, Some(&mut buffer[..count]), Some(&mut n), None)?;
            }
            if n == 0 || n as usize > remaining {
                return Err(LifecycleError::Unsettled);
            }
            unsafe {
                BCryptHashData(hash, &buffer[..n as usize], 0).ok()?;
            }
            remaining -= n as usize;
        }
        let mut result = [0; 32];
        unsafe {
            BCryptFinishHash(hash, &mut result, 0).ok()?;
        }
        // Crypto handles have their own namespace; never identify one only by
        // an integer that could coincide with an unrelated kernel handle.
        let index = self
            .handles
            .iter()
            .position(|v| v.role == Role::Hash && v.handle == hash_handle)
            .ok_or(LifecycleError::Invalid)?;
        self.close(index)?;
        let index = self
            .handles
            .iter()
            .position(|v| v.role == Role::Algorithm && v.handle == algorithm_handle)
            .ok_or(LifecycleError::Invalid)?;
        self.close(index)?;
        current(deadline)?;
        Ok(result)
    }
}

struct Session {
    resources: Resources,
    manifest: Manifest,
    image: PathBuf,
    image_sha: [u8; 32],
    pipe_name: HSTRING,
    deadline: Instant,
    peer_images: Vec<(HANDLE, HANDLE)>,
    sealed_package_sha: Option<[u8; 32]>,
}
impl Session {
    fn new(deadline: Instant) -> Result<Self> {
        let mut resources = Resources::empty()?;
        resources.sid = resources.token_sid(unsafe { GetCurrentProcess() })?;
        require_user(&resources.sid)?;
        let image = image_path(unsafe { GetCurrentProcess() })?;
        let mut parents: Vec<_> = image.ancestors().skip(1).collect();
        parents.reverse();
        if parents.len() > 40 {
            return Err(LifecycleError::Invalid);
        }
        for path in parents {
            let h = resources.open_file(
                path,
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                OPEN_EXISTING,
                true,
                false,
            )?;
            resources.add(h, Role::Parent);
            verify_info(h, true)?;
        }
        let h = resources.open_file(
            &image,
            GENERIC_READ.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        resources.add(h, Role::Parent);
        verify_info(h, false)?;
        let mut size = 0;
        unsafe {
            GetFileSizeEx(h, &mut size)?;
        }
        if !(21..=64 * 1024 * 1024).contains(&size) {
            return Err(LifecycleError::Invalid);
        }
        let mut footer = [0u8; 20];
        resources.read_at(h, size - 20, &mut footer)?;
        if &footer[4..] != manifest::FOOTER {
            return Err(LifecycleError::Invalid);
        }
        let n = u32::from_le_bytes(footer[..4].try_into().unwrap()) as usize;
        if n == 0 || n > manifest::MAX_MANIFEST || n as i64 + 20 >= size {
            return Err(LifecycleError::Invalid);
        }
        let mut raw = vec![0; n];
        resources.read_at(h, size - 20 - n as i64, &mut raw)?;
        let manifest = manifest::parse(&raw)?;
        let sealed_package_sha = if size - 20 - n as i64 >= 48 {
            let mut seal = [0; 48];
            resources.read_at(h, size - 20 - n as i64 - 48, &mut seal)?;
            let package = controlled_owner::package_seal(seal)?;
            if package.is_some() && size - 20 - n as i64 >= 96 {
                let mut prior = [0; 16];
                resources.read_at(h, size - 20 - n as i64 - 96, &mut prior)?;
                if &prior == controlled_owner::PACKAGE_SEAL_MAGIC {
                    return Err(LifecycleError::Invalid);
                }
            }
            package
        } else {
            None
        };
        let image_sha = resources.digest(h, 64 * 1024 * 1024, deadline)?;
        let sid_hex: String = resources.sid.iter().map(|b| format!("{b:02X}")).collect();
        let pipe_name = HSTRING::from(format!(
            r"\\.\pipe\FutureClassroom-Installer-{}-{}",
            INSTALL_FAMILY, sid_hex
        ));
        Ok(Self {
            resources,
            manifest,
            image,
            image_sha,
            pipe_name,
            deadline,
            peer_images: Vec::new(),
            sealed_package_sha,
        })
    }
    fn authenticate_image(&mut self, process: HANDLE) -> Result<(GuardianIdentity, HANDLE)> {
        live(process)?;
        let sid = self.resources.token_sid(process)?;
        if sid != self.resources.sid {
            return Err(LifecycleError::Invalid);
        }
        // Token capability is no longer needed after actual SID comparison.
        let token = self
            .resources
            .handles
            .iter()
            .rposition(|h| h.role == Role::Token)
            .ok_or(LifecycleError::Invalid)?;
        self.resources.close(token)?;
        let path = image_path(process)?;
        let image = self.resources.open_file(
            &path,
            GENERIC_READ.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        self.resources.add(image, Role::Control);
        verify_info(image, false)?;
        if self
            .resources
            .digest(image, 64 * 1024 * 1024, self.deadline)?
            != self.image_sha
        {
            return Err(LifecycleError::Invalid);
        }
        // A short peer image remains pinned until FINISHED and checked close;
        // persistent parent authentication stays pinned for the whole session.
        Ok((process_identity(process)?, image))
    }
    fn peer(&mut self, pipe: HANDLE, server: bool) -> Result<HANDLE> {
        let mut pid = 0;
        unsafe {
            if server {
                GetNamedPipeClientProcessId(pipe, &mut pid)?;
            } else {
                GetNamedPipeServerProcessId(pipe, &mut pid)?;
            }
        }
        if server {
            // Actual connected security context, not a packet PID/SID. This is
            // identification only; no operation uses the impersonated authority.
            unsafe {
                ImpersonateNamedPipeClient(pipe)?;
            }
            let checked = (|| -> Result<()> {
                if self.resources.handles.len() >= HANDLE_LIMIT {
                    return Err(LifecycleError::Invalid);
                }
                let mut token = HANDLE::default();
                unsafe {
                    OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token)?;
                }
                self.resources.add(token, Role::Token);
                let sid = Resources::sid_from_token(token)?;
                if sid != self.resources.sid {
                    return Err(LifecycleError::Invalid);
                }
                self.resources.close_handle(token)
            })();
            // A revert failure is process-fatal negative state; never continue
            // filesystem work under an unknown caller token.
            if unsafe { RevertToSelf() }.is_err() {
                self.resources.retain_until_exit();
                return Err(LifecycleError::Faulted);
            }
            checked?;
        }
        let process = self.resources.process(pid, Role::Control)?;
        let (_, image) = self.authenticate_image(process)?;
        self.peer_images.push((process, image));
        Ok(process)
    }
    fn close_peer(&mut self, process: HANDLE) -> Result<()> {
        let index = self
            .peer_images
            .iter()
            .position(|(p, _)| *p == process)
            .ok_or(LifecycleError::Invalid)?;
        let image = self.peer_images[index].1;
        // No ACK if either original close is unknown. Keep the ledger entry on
        // failure; Resources' sticky fault prohibits retry/replacement.
        self.resources.close_handle(image)?;
        self.resources.close_handle(process)?;
        self.peer_images.remove(index);
        Ok(())
    }
    fn finish_request(&mut self, pipe: HANDLE, peer: HANDLE) -> Result<()> {
        self.send(pipe, Packet::new(FINISHED))?;
        if self.receive(pipe)?.kind != RELEASED {
            return Err(LifecycleError::Unsettled);
        }
        self.close_peer(peer)?;
        self.send(pipe, Packet::new(RECEIVED))?;
        self.resources.close_handle(pipe)
    }
    fn release_connection(&mut self, pipe: HANDLE, peer: HANDLE) -> Result<()> {
        if self.receive(pipe)?.kind != FINISHED {
            return Err(LifecycleError::Unsettled);
        }
        self.close_peer(peer)?;
        // The client waits for RELEASED before exiting. Send before disconnect,
        // after every short peer image/process pin has actually returned.
        self.send(pipe, Packet::new(RELEASED))?;
        // Disconnect discards unread outbound data. This bounded final receipt
        // proves the peer actually consumed RELEASED; no unbounded pipe flush.
        if self.receive(pipe)?.kind != RECEIVED {
            return Err(LifecycleError::Unsettled);
        }
        if let Err(error) = unsafe { DisconnectNamedPipe(pipe) } {
            if error.code() != ERROR_PIPE_NOT_CONNECTED.to_hresult() {
                return Err(error.into());
            }
        }
        Ok(())
    }
    fn server(&mut self) -> Result<HANDLE> {
        if self.resources.handles.len() >= HANDLE_LIMIT {
            return Err(LifecycleError::Invalid);
        }
        let pipe = with_security(&self.resources.sid, false, |sa| {
            let h = unsafe {
                CreateNamedPipeW(
                    &self.pipe_name,
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    4096,
                    4096,
                    0,
                    Some(sa),
                )
            };
            if h == INVALID_HANDLE_VALUE {
                Err(Error::from_thread())
            } else {
                Ok(h)
            }
        })?;
        self.resources.add(pipe, Role::Control);
        Ok(pipe)
    }
    fn connect(&mut self) -> Result<HANDLE> {
        loop {
            current(self.deadline)?;
            if self.resources.handles.len() >= HANDLE_LIMIT {
                return Err(LifecycleError::Invalid);
            }
            let result = unsafe {
                CreateFileW(
                    &self.pipe_name,
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_MODE(0),
                    None,
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    None,
                )
            };
            match result {
                Ok(h) => {
                    self.resources.add(h, Role::Control);
                    let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
                    unsafe {
                        SetNamedPipeHandleState(h, Some(&mode), None, None)?;
                    }
                    return Ok(h);
                }
                Err(e)
                    if e.code() == ERROR_PIPE_BUSY.to_hresult()
                        || e.code() == ERROR_FILE_NOT_FOUND.to_hresult() =>
                {
                    pause(self.deadline)?
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    fn accept(&self, pipe: HANDLE) -> Result<()> {
        loop {
            current(self.deadline)?;
            match unsafe { ConnectNamedPipe(pipe, None) } {
                Ok(()) => return Ok(()),
                Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => return Ok(()),
                Err(e) if e.code() == ERROR_PIPE_LISTENING.to_hresult() => pause(self.deadline)?,
                Err(e) => return Err(e.into()),
            }
        }
    }
    fn send(&self, pipe: HANDLE, packet: Packet) -> Result<()> {
        current(self.deadline)?;
        let bytes = packet.encode();
        let mut n = 0;
        unsafe {
            WriteFile(pipe, Some(&bytes), Some(&mut n), None)?;
        }
        if n as usize != FRAME {
            return Err(LifecycleError::Unsettled);
        }
        current(self.deadline)
    }
    fn receive(&self, pipe: HANDLE) -> Result<Packet> {
        let mut bytes = [0; FRAME];
        let mut offset = 0;
        while offset < FRAME {
            current(self.deadline)?;
            let mut n = 0;
            match unsafe { ReadFile(pipe, Some(&mut bytes[offset..]), Some(&mut n), None) } {
                Ok(()) if n != 0 => offset += n as usize,
                Ok(()) => pause(self.deadline)?,
                Err(e) if e.code() == ERROR_NO_DATA.to_hresult() => pause(self.deadline)?,
                Err(e) => return Err(e.into()),
            }
        }
        Packet::decode(bytes)
    }
    fn request(&mut self, packet: Packet) -> Result<Packet> {
        let pipe = self.connect()?;
        self.send(pipe, packet)?;
        let peer = self.peer(pipe, false)?;
        let response = self.receive(pipe)?;
        self.finish_request(pipe, peer)?;
        Ok(response)
    }
    fn engine(&mut self, child: HANDLE) -> Result<HANDLE> {
        let engine = self.resources.parent(child)?;
        let path = image_path(engine)?;
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
        // The original OS msiexec parent may be the LocalSystem service even
        // though this helper runs as the current user. This exception applies
        // only to that pinned engine capability: resources.sid and all helper,
        // pipe-peer and effect identities remain the current user's SID.
        // A shared service PID/creation is not a transaction/session identity;
        // the original guardian Join/End/exit proofs remain separate.
        if !engine_parent_sid_allowed(&self.resources.token_sid(engine)?, &self.resources.sid) {
            return Err(LifecycleError::Invalid);
        }
        // Keep the actual OS engine image fixed, too. Serviced system binaries
        // can legitimately have WinSxS hard links, so do not pretend the payload
        // single-link policy applies to Windows' own system image.
        let image = self.resources.open_file(
            &path,
            GENERIC_READ.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        self.resources.add(image, Role::Control);
        let mut details = BY_HANDLE_FILE_INFORMATION::default();
        unsafe {
            GetFileInformationByHandle(image, &mut details)?;
        }
        if details.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0)
            != 0
        {
            return Err(LifecycleError::Invalid);
        }
        Ok(engine)
    }
}

pub(crate) fn run(role: EntryRole, deadline: Instant, resolved_target: Option<&str>) -> Result<()> {
    let mut session = Session::new(deadline)?;
    let result = match role {
        EntryRole::Begin => begin(
            &mut session,
            resolved_target.ok_or(LifecycleError::Invalid)?,
        ),
        EntryRole::Broker => server(&mut session),
        EntryRole::Guardian(transaction) => guardian(&mut session, transaction),
        EntryRole::Supervisor(transaction) => supervisor::run(&mut session, transaction),
        EntryRole::OwnedGuardian => controlled_owner::guardian(&mut session),
    };
    if result.is_err() && matches!(role, EntryRole::Broker | EntryRole::Supervisor(_)) {
        // A supervisor failure retains only its own native capabilities until
        // its exit. After broker exit0 this does NOT recreate old Pending or
        // change the already reconciled business outcome.
        session.resources.retain_until_exit();
    }
    result?;
    session.resources.release()
}

fn begin(session: &mut Session, resolved_target: &str) -> Result<()> {
    // Unsupported/damaged/foreign state is rejected before
    // spawning a broker or creating durable Pending, independent of MSI flags.
    effects::target(resolved_target)?;
    effects::admit(session)?;
    session.engine(unsafe { GetCurrentProcess() })?;
    if session.resources.handles.len() + 2 > HANDLE_LIMIT {
        return Err(LifecycleError::Invalid);
    }
    let application = HSTRING::from(session.image.as_os_str());
    let mut command: Vec<u16> = format!("\"{}\" broker", session.image.display())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut child = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            &application,
            Some(windows::core::PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            PROCESS_CREATION_FLAGS(0),
            None,
            None,
            &startup,
            &mut child,
        )?;
    }
    session.resources.add(child.hProcess, Role::Control);
    session.resources.add(child.hThread, Role::Control);
    session.resources.close_handle(child.hThread)?;
    let pipe = session.connect()?;
    session.send(pipe, Packet::new(BEGIN))?;
    let server = session.peer(pipe, false)?;
    let expected = process_identity(child.hProcess)?;
    if process_identity(server)? != expected {
        return Err(LifecycleError::Invalid);
    }
    if session.receive(pipe)?.kind != ACK {
        return Err(LifecycleError::Unsettled);
    }
    session.finish_request(pipe, server)?;
    // No wait/kill: the exact broker intentionally outlives this synchronous CA.
    // Every native launch control is returned through checked Resources close;
    // there is no unchecked std::process::Child destructor on this success path.
    live(child.hProcess)?;
    Ok(())
}

fn guardian(session: &mut Session, transaction: u32) -> Result<()> {
    // The actual child belongs to the authenticated tracked supervisor, whose
    // parent remains the original OS engine. The broker independently binds
    // this same original supervisor before granting any decision.
    let parent = session.resources.parent(unsafe { GetCurrentProcess() })?;
    session.authenticate_image(parent)?;
    session.engine(parent)?;
    supervisor::await_parent(session, parent, transaction)?;
    let mut hello = Packet::new(GUARDIAN);
    hello.transaction = transaction;
    let decision = session.request(hello)?;
    if decision.kind != DECISION || decision.transaction != transaction {
        return Err(LifecycleError::Invalid);
    }
    let mut changed = HANDLE::default();
    let join = unsafe { MsiJoinTransaction(MSIHANDLE(transaction), 0, &mut changed) };
    if !changed.is_invalid() {
        session.resources.add(changed, Role::Control);
    }
    let end = if join == 0 {
        unsafe {
            MsiEndTransaction(if decision.outcome == 1 {
                MSITRANSACTIONSTATE_COMMIT
            } else {
                MSITRANSACTIONSTATE_ROLLBACK
            })
        }
    } else {
        u32::MAX
    };
    if !changed.is_invalid() {
        session.resources.close_handle(changed)?;
    }
    let receipt = Packet {
        kind: ENDED,
        outcome: decision.outcome,
        transaction,
        join,
        end,
    };
    if session.request(receipt)?.kind != ACK || join != 0 || end != 0 {
        return Err(LifecycleError::Unsettled);
    }
    Ok(())
}

fn server(session: &mut Session) -> Result<()> {
    let begin = session.resources.parent(unsafe { GetCurrentProcess() })?;
    session.authenticate_image(begin)?;
    let engine = session.engine(begin)?;
    let original_engine = process_identity(engine)?;
    let admission = effects::admit(session)?;
    let identity = InstallIdentity::for_current_user()?;
    let mut owner =
        LifecycleGate::try_begin_maintenance(&identity, admission.operation(), session.deadline)?;
    let result = (|| -> Result<()> {
        let pipe = session.server()?;
        let mut began = false;
        let mut supervisor = None;
        let mut guardian = None;
        let mut decision = None;
        let mut effects_decision = None;
        let mut ended = false;
        for _ in 0..MAX_CONNECTIONS {
            session.accept(pipe)?;
            let packet = session.receive(pipe)?;
            let peer = session.peer(pipe, true)?;
            let peer_id = process_identity(peer)?;
            match packet.kind {
                BEGIN if !began && peer_id == process_identity(begin)? => {
                    // Revalidate the same original preimage under the exclusive
                    // gate. Its checked pins return before first MSI mutation.
                    effects::revalidate(session, &admission)?;
                    began = true;
                    session.send(pipe, Packet::new(ACK))?;
                }
                SUPERVISOR if began && supervisor.is_none() => {
                    let parent = session.engine(peer)?;
                    if process_identity(parent)? != original_engine {
                        return Err(LifecycleError::Invalid);
                    }
                    live(engine)?;
                    // Keep this original supervisor process and image pinned,
                    // independent of the short hello connection capabilities.
                    let original = duplicate_process(&mut session.resources, peer, Role::Control)?;
                    session.authenticate_image(original)?;
                    supervisor = Some((peer_id, original, packet.transaction));
                    session.send(pipe, Packet::new(ACK))?;
                }
                GUARDIAN if began && guardian.is_none() => {
                    let (expected_supervisor, original, transaction) =
                        supervisor.ok_or(LifecycleError::Invalid)?;
                    let parent = session.resources.parent(peer)?;
                    if process_identity(parent)? != expected_supervisor
                        || packet.transaction != transaction
                    {
                        return Err(LifecycleError::Invalid);
                    }
                    live(original)?;
                    live(engine)?;
                    // Transfer a duplicate of this authenticated original
                    // process object into the ORIGINAL maintenance ledger.
                    owner.watch_attempted = true;
                    let mut duplicate = HANDLE::default();
                    unsafe {
                        DuplicateHandle(
                            GetCurrentProcess(),
                            peer,
                            GetCurrentProcess(),
                            &mut duplicate,
                            0,
                            false,
                            DUPLICATE_SAME_ACCESS,
                        )?;
                    }
                    owner.resources.add(duplicate, Role::Guardian);
                    owner.guardian = Some(peer_id);
                    let observed = effects::staged(session, &admission)?;
                    let mut response = Packet::new(DECISION);
                    response.transaction = packet.transaction;
                    response.outcome = match observed.outcome() {
                        SettledOutcome::Commit => 1,
                        SettledOutcome::Rollback => 2,
                    };
                    guardian = Some(peer_id);
                    decision = Some(response);
                    effects_decision = Some(observed);
                    session.send(pipe, response)?;
                }
                ENDED if guardian == Some(peer_id) => {
                    let expected = decision.ok_or(LifecycleError::Unsettled)?;
                    if packet.transaction != expected.transaction
                        || packet.outcome != expected.outcome
                        || packet.join != 0
                        || packet.end != 0
                    {
                        return Err(LifecycleError::Unsettled);
                    }
                    ended = true;
                    session.send(pipe, Packet::new(ACK))?;
                }
                _ => return Err(LifecycleError::Invalid),
            }
            session.release_connection(pipe, peer)?;
            if ended {
                break;
            }
        }
        if !ended {
            return Err(LifecycleError::Unsettled);
        }
        loop {
            current(session.deadline)?;
            match owner.resources.guardian_returned() {
                Ok(()) => break,
                Err(LifecycleError::GuardianRunning) => pause(session.deadline)?,
                Err(e) => return Err(e),
            }
        }
        let observed = effects_decision.ok_or(LifecycleError::Unsettled)?;
        let outcome = observed.outcome();
        let mut proof = effects::reconcile(session, &admission, observed)?;
        proof.check()?;
        // This receipt is constructed solely here, after actual native facts
        // and independently verified effects. There is no wire Settlement API.
        owner.record_settlement(Settlement {
            operation_id: owner.operation_id(),
            guardian: guardian.ok_or(LifecycleError::Unsettled)?,
            outcome,
            join_return: 0,
            end_return: 0,
            effects_reconciled: true,
        })?;
        proof.check()?;
        // Return every IPC/effects/control capability before the original
        // maintenance owner is allowed its final Pending/gate commitpoint.
        proof.release()?;
        session.resources.release()?;
        owner.try_release()?;
        Ok(())
    })();
    if result.is_err() {
        owner.resources.retain_until_exit();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_parent_sid_policy_accepts_only_same_user_or_exact_local_system() {
        // S-1-5-21-1-2-3-1001: synthetic ordinary user, not a deployment fact.
        let user = [
            1, 5, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 233, 3, 0, 0,
        ];
        let system = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
        assert!(engine_parent_sid_allowed(&user, &user));
        assert!(engine_parent_sid_allowed(&system, &user));
        let mut foreign_user = user;
        foreign_user[24] = 234;
        assert!(!engine_parent_sid_allowed(&foreign_user, &user));
        for rid in [11, 19, 20] {
            let mut other_well_known = system;
            other_well_known[8] = rid;
            assert!(!engine_parent_sid_allowed(&other_well_known, &user));
        }
        let administrators = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];
        assert!(!engine_parent_sid_allowed(&administrators, &user));
        for length in 0..system.len() {
            assert!(!engine_parent_sid_allowed(&system[..length], &user));
        }
        let mut extended = system.to_vec();
        extended.push(0);
        assert!(!engine_parent_sid_allowed(&extended, &user));
        for index in [0, 1, 7] {
            let mut malformed = system;
            malformed[index] ^= 1;
            assert!(!engine_parent_sid_allowed(&malformed, &user));
        }
    }

    #[test]
    fn empty_control_packets_have_no_transaction_or_settlement_fields() {
        for kind in [BEGIN, ACK, FINISHED, RELEASED, RECEIVED] {
            let mut p = Packet::new(kind);
            assert!(Packet::decode(p.encode()).is_ok());
            for forged in [1, 2] {
                p.outcome = forged;
                assert!(Packet::decode(p.encode()).is_err());
            }
            p.outcome = 0;
            p.transaction = 1;
            assert!(Packet::decode(p.encode()).is_err());
            p.transaction = 0;
            p.end = 0x1234;
            assert!(Packet::decode(p.encode()).is_err());
        }
    }

    #[test]
    fn supervisor_hello_is_transaction_only_and_old_notices_are_rejected() {
        for kind in [SUPERVISOR, GUARDIAN] {
            let mut packet = Packet::new(kind);
            assert!(Packet::decode(packet.encode()).is_err());
            packet.transaction = 42;
            assert!(Packet::decode(packet.encode()).is_ok());
            for field in [0, 1, 2] {
                let mut forged = packet;
                match field {
                    0 => forged.outcome = 1,
                    1 => forged.join = 1,
                    _ => forged.end = 1,
                }
                assert!(Packet::decode(forged.encode()).is_err());
            }
        }
        assert!(
            Packet::decode(Packet::new(5).encode()).is_err(),
            "retired advisory commit role is not a protocol command"
        );
    }

    fn fixture(name: &str, deadline: Instant) -> Session {
        let mut resources = Resources::empty().unwrap();
        resources.sid = resources.token_sid(unsafe { GetCurrentProcess() }).unwrap();
        let image = image_path(unsafe { GetCurrentProcess() }).unwrap();
        let handle = resources
            .open_file(
                &image,
                GENERIC_READ.0,
                FILE_SHARE_READ,
                OPEN_EXISTING,
                false,
                false,
            )
            .unwrap();
        resources.add(handle, Role::Parent);
        verify_info(handle, false).unwrap();
        let image_sha = resources
            .digest(handle, 64 * 1024 * 1024, deadline)
            .unwrap();
        Session {
            resources,
            image,
            image_sha,
            manifest: Manifest {
                product: String::new(),
                version: String::new(),
                entries: Vec::new(),
            },
            pipe_name: HSTRING::from(name),
            deadline,
            peer_images: Vec::new(),
            sealed_package_sha: None,
        }
    }
    fn fixture_name() -> String {
        use fc_ports::RandomSource;
        let mut nonce = [0; 16];
        crate::SystemRandom.fill(&mut nonce).unwrap();
        let tail: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            r"\\.\pipe\FutureClassroom-Test-Lifecycle-{}-{tail}",
            std::process::id()
        )
    }
    const CHILD: &str = "installer_lifecycle::native::broker::tests::pipe_process_fixture_child";

    struct FixtureChild(std::process::Child);
    impl Drop for FixtureChild {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                // Only our closed test-role subprocess, never an MSI engine or
                // guardian transaction, can enter this fixture-only cleanup.
                let _ = self.0.kill();
                self.0.wait().expect("owned pipe fixture subprocess reaped");
            }
        }
    }

    // A separate real process gives the pipe an actual OS identity and lets a
    // permanent cleanup fault remain process-local. This role is test-only and
    // never present in the normal helper's parser/manifest/command surface.
    #[test]
    fn pipe_process_fixture_child() {
        let Ok(name) = std::env::var("FC_LIFECYCLE_PIPE_FIXTURE") else {
            return;
        };
        assert!(name.starts_with(r"\\.\pipe\FutureClassroom-Test-Lifecycle-"));
        assert!(
            name.len() < 160
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"\\.-".contains(&b))
        );
        let mut session = fixture(&name, Instant::now() + Duration::from_secs(10));
        if std::env::var("FC_LIFECYCLE_PIPE_RELEASE_FAULT").as_deref() == Ok("1") {
            let pipe = session.server().unwrap();
            session.accept(pipe).unwrap();
            assert_eq!(session.receive(pipe).unwrap().kind, BEGIN);
            let peer = session.peer(pipe, true).unwrap();
            session.send(pipe, Packet::new(ACK)).unwrap();
            session.resources.fail_close_role = Some(Role::Control);
            assert!(session.release_connection(pipe, peer).is_err());
            assert!(session.resources.failed);
            assert!(
                !session.peer_images.is_empty(),
                "unknown original close cannot create RELEASED authority"
            );
            session.resources.retain_until_exit();
            assert!(retained_count() > 0);
            return;
        }
        if std::env::var("FC_LIFECYCLE_PIPE_CLOSE_FAULT").as_deref() == Ok("1") {
            let pipe = session.server().unwrap();
            session.resources.fail_close_role = Some(Role::Control);
            assert_eq!(session.resources.release(), Err(LifecycleError::Faulted));
            let before = session.resources.handles.len();
            assert_eq!(session.resources.release(), Err(LifecycleError::Faulted));
            assert_eq!(
                session.resources.handles.len(),
                before,
                "unknown close must never be retried"
            );
            assert!(!session.resources.released());
            session.resources.retain_until_exit();
            assert!(retained_count() > 0);
            assert!(matches!(Resources::empty(), Err(LifecycleError::Faulted)));
            assert!(
                unsafe { GetNamedPipeInfo(pipe, None, None, None, None) }.is_ok(),
                "original real pipe capability retained"
            );
            return;
        }
        let parent = session
            .resources
            .parent(unsafe { GetCurrentProcess() })
            .unwrap();
        supervisor::await_parent(&mut session, parent, 42).unwrap();
        let response = session.request(Packet::new(BEGIN)).unwrap();
        assert_eq!(response.kind, ACK);
        session.resources.release().unwrap();
    }

    #[test]
    fn actual_local_pipe_binds_original_process_sid_image_and_returns_controls() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let name = fixture_name();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut session = fixture(&name, deadline);
        let pipe = session.server().unwrap();
        let original_name = session.pipe_name.clone();
        session.pipe_name = supervisor::child_pipe_name(
            &original_name,
            process_identity(unsafe { GetCurrentProcess() }).unwrap(),
        );
        let barrier = session.server().unwrap();
        session.pipe_name = original_name;
        let mut child = FixtureChild(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", CHILD, "--nocapture"])
                .env("FC_LIFECYCLE_PIPE_FIXTURE", &name)
                .env_remove("FC_LIFECYCLE_PIPE_CLOSE_FAULT")
                .env_remove("FC_LIFECYCLE_PIPE_RELEASE_FAULT")
                .spawn()
                .unwrap(),
        );
        session.accept(barrier).unwrap();
        let hello = session.receive(barrier).unwrap();
        assert_eq!(hello.kind, GUARDIAN);
        assert_eq!(hello.transaction, 42);
        let original_child = process_identity(HANDLE(child.0.as_raw_handle())).unwrap();
        let barrier_peer = session.peer(barrier, true).unwrap();
        assert_eq!(process_identity(barrier_peer).unwrap(), original_child);
        session.send(barrier, Packet::new(ACK)).unwrap();
        session.release_connection(barrier, barrier_peer).unwrap();
        session.resources.close_handle(barrier).unwrap();
        session.accept(pipe).unwrap();
        assert_eq!(session.receive(pipe).unwrap().kind, BEGIN);
        let peer = session.peer(pipe, true).unwrap();
        assert_eq!(
            process_identity(peer).unwrap(),
            process_identity(HANDLE(child.0.as_raw_handle())).unwrap()
        );
        session.send(pipe, Packet::new(ACK)).unwrap();
        session.release_connection(pipe, peer).unwrap();
        assert!(
            session.peer_images.is_empty(),
            "RELEASED returned the original short-peer image/process"
        );
        assert!(session
            .resources
            .handles
            .iter()
            .all(|owned| owned.handle != peer));
        loop {
            current(deadline).unwrap();
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            pause(deadline).unwrap();
        }
        session.resources.release().unwrap();
        assert!(session.resources.released());
    }

    #[test]
    fn unknown_short_peer_close_never_grants_released_ack() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let name = fixture_name();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut session = fixture(&name, deadline);
        let mut child = FixtureChild(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", CHILD, "--nocapture"])
                .env("FC_LIFECYCLE_PIPE_FIXTURE", &name)
                .env("FC_LIFECYCLE_PIPE_RELEASE_FAULT", "1")
                .env_remove("FC_LIFECYCLE_PIPE_CLOSE_FAULT")
                .spawn()
                .unwrap(),
        );
        assert!(
            session.request(Packet::new(BEGIN)).is_err(),
            "original unknown image close must never receive RELEASED"
        );
        loop {
            current(deadline).unwrap();
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            pause(deadline).unwrap();
        }
        session.resources.release().unwrap();
        assert!(
            session.resources.released(),
            "child fault never resets/replaces an original owner in that process"
        );
    }

    #[test]
    fn unconnected_pipe_timeout_has_no_outstanding_io_or_replacement() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let name = fixture_name();
        let mut session = fixture(&name, Instant::now() + Duration::from_secs(10));
        let pipe = session.server().unwrap();
        session.deadline = Instant::now() + Duration::from_millis(10);
        assert_eq!(session.accept(pipe), Err(LifecycleError::Expired));
        session.resources.release().unwrap();
        assert!(session.resources.released());
    }

    #[test]
    fn real_pipe_cleanup_fault_retains_original_and_faults_only_subprocess() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let name = fixture_name();
        let mut child = FixtureChild(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", CHILD, "--nocapture"])
                .env("FC_LIFECYCLE_PIPE_FIXTURE", name)
                .env("FC_LIFECYCLE_PIPE_CLOSE_FAULT", "1")
                .env_remove("FC_LIFECYCLE_PIPE_RELEASE_FAULT")
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            current(deadline).unwrap();
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            pause(deadline).unwrap();
        }
        // Child process exit is the OS cleanup boundary for that real pipe;
        // parent remains usable and does not reset the child's sticky fault.
        let mut owner = Resources::empty().unwrap();
        owner.release().unwrap();
    }
}
