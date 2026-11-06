use std::{
    marker::PhantomData,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::{
    core::{Error, Result, HSTRING, PWSTR},
    Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::Threading::*,
    },
};

const CHILDREN: &[&str] = &[
    "state",
    "config",
    "runtime",
    "teachers",
    "open-copies",
    "quarantine",
    "cache",
    "exports",
];

/// Paths are constructed by the local bootstrap, never from network DTOs.
/// Discovery has no filesystem side effect and does not loosen ProgramData ACLs.
#[derive(Clone, Debug)]
pub struct DataPaths {
    root: PathBuf,
    origin: RootOrigin,
}
#[derive(Clone, Copy, Debug)]
enum RootOrigin {
    Canonical,
    ExistingLegacy { identity: (u32, u64) },
    ExistingLocal { identity: (u32, u64) },
}
impl DataPaths {
    pub fn program_data() -> Result<Self> {
        // The native KnownFolder adapter already appends the application name.
        Self::discover_program_data_at(crate::program_data()?)
    }
    fn discover_program_data_at(canonical: PathBuf) -> Result<Self> {
        let legacy = canonical.join("FutureClassroom");
        match std::fs::symlink_metadata(&legacy) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                root: canonical,
                origin: RootOrigin::Canonical,
            }),
            Err(_) => Err(Error::new(
                E_ACCESSDENIED,
                "无法确认旧版资料目录；未创建或修改资料。",
            )),
            Ok(_) => {
                let identity = verify_legacy_envelope(&legacy)?;
                Ok(Self {
                    root: legacy,
                    origin: RootOrigin::ExistingLegacy { identity },
                })
            }
        }
    }
    /// Test harness must explicitly choose an isolated direct child of an existing
    /// local directory. The prefix prevents accidentally selecting production data.
    pub fn isolated_test(root: PathBuf) -> Result<Self> {
        if !root.is_absolute()
            || root
                .file_name()
                .and_then(|n| n.to_str())
                .is_none_or(|n| !n.starts_with("FutureClassroom-Test-"))
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        Ok(Self {
            root,
            origin: RootOrigin::Canonical,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// A selected old root can only open an existing deployment or run its
    /// original cold recovery. Missing credentials never authorize fresh setup.
    /// Bootstrap must enforce this before opening/creating any database or key.
    pub fn requires_existing_deployment(&self) -> bool {
        matches!(
            self.origin,
            RootOrigin::ExistingLegacy { .. } | RootOrigin::ExistingLocal { .. }
        )
    }
    pub fn database(&self) -> PathBuf {
        self.root.join("state").join("classroom.sqlite3")
    }
    pub fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }
    pub fn teacher_materials(&self) -> PathBuf {
        self.root.join("teachers")
    }
    pub fn open_copies(&self) -> PathBuf {
        self.root.join("open-copies")
    }
    /// Before recovery may inspect missing/moved children, verify the existing
    /// deployment root without creating or silently repairing any directory.
    pub fn verify_root_for_current_user(&self) -> Result<()> {
        if matches!(self.origin, RootOrigin::ExistingLocal { .. }) {
            self.local_migration_retired()?;
            return Ok(());
        }
        check_local_ancestors(&self.root)?;
        verify_acl(&self.root, &Descriptor::for_user(&current_user_sid()?)?)?;
        if let RootOrigin::ExistingLegacy { identity } = self.origin {
            if verify_legacy_envelope(&self.root)? != identity {
                return Err(Error::new(
                    E_ACCESSDENIED,
                    "旧版资料目录已改变；未创建或修改资料。",
                ));
            }
        }
        Ok(())
    }
    /// Verify or create only the fixed instance-lock parent before acquiring the
    /// lock. A pre-existing junction or mismatching ACL is never followed/fixed.
    pub fn prepare_instance_runtime(&self) -> Result<()> {
        self.verify_root_for_current_user()?;
        let _root = crate::materials::files::PinnedDirectory::open(&self.root)?;
        self.verify_root_for_current_user()?;
        let runtime = self.runtime();
        check_local_ancestors(&runtime)?;
        ensure_directory(&runtime, &Descriptor::for_user(&current_user_sid()?)?)
    }
    /// Fixed destructive-reinitialization journal namespace. No live data
    /// directory is created or repaired by this operation.
    pub fn prepare_reinitialization_namespace(&self) -> Result<()> {
        self.verify_root_for_current_user()?;
        let _root = crate::materials::files::PinnedDirectory::open(&self.root)?;
        self.verify_root_for_current_user()?;
        let expected = Descriptor::for_user(&current_user_sid()?)?;
        ensure_reinitialization_directory(&self.root.join("reinitialization"), &expected)
    }
    /// Create one fresh protected operation, before its owner receipt is written.
    /// An existing operation is never adopted, including a nonce collision.
    pub fn prepare_reinitialization_operation(&self, operation: &Path) -> Result<()> {
        self.verify_reinitialization_path(operation, false)?;
        self.prepare_reinitialization_namespace()?;
        let _namespace =
            crate::materials::files::PinnedDirectory::open(&self.root.join("reinitialization"))?;
        check_local_ancestors(operation)?;
        let expected = Descriptor::for_user(&current_user_sid()?)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: expected.0 .0,
            bInheritHandle: false.into(),
        };
        // SAFETY: fresh fixed-shape operation and live owned descriptor. Unlike
        // ensure_directory, AlreadyExists is an error before any contents exist.
        let native_name = reinitialization_native_name(operation)?;
        unsafe {
            CreateDirectoryW(&native_name, Some(&attributes))?;
        }
        verify_acl_named(operation, &expected, &native_name)
    }
    /// Create only a fresh reset candidate's fixed empty teaching namespaces.
    /// The caller has already created the protected operation and persisted its
    /// owner receipt; old credentials/database are never opened here.
    pub fn prepare_reinitialization_candidate(&self, candidate: &Path) -> Result<()> {
        self.verify_reinitialization_path(candidate, true)?;
        self.verify_root_for_current_user()?;
        let expected = Descriptor::for_user(&current_user_sid()?)?;
        let operation = candidate
            .parent()
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        let _operation = crate::materials::files::PinnedDirectory::open(operation)?;
        self.verify_root_for_current_user()?;
        verify_acl_named(
            &self.root.join("reinitialization"),
            &expected,
            &reinitialization_native_name(&self.root.join("reinitialization"))?,
        )?;
        verify_acl_named(
            operation,
            &expected,
            &reinitialization_native_name(operation)?,
        )?;
        check_local_ancestors(candidate)?;
        ensure_reinitialization_directory(candidate, &expected)?;
        let _candidate = crate::materials::files::PinnedDirectory::open(candidate)?;
        ensure_reinitialization_directory(&operation.join("old"), &expected)?;
        ensure_reinitialization_directory(&operation.join("manifests"), &expected)?;
        // Keep synchronized with bootstrap's versioned reset-owned tree set.
        for name in [
            "state",
            "config",
            "teachers",
            "profile-content",
            "voice",
            "staging",
            "launch-copies",
            "web-material-staging",
            "exports",
            "open-copies",
            "quarantine",
            "cache",
            "recovery",
        ] {
            ensure_reinitialization_directory(&candidate.join(name), &expected)?;
        }
        Ok(())
    }
    fn verify_reinitialization_path(&self, path: &Path, candidate: bool) -> Result<()> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        let parts: Vec<_> = relative.components().collect();
        if parts.len() != if candidate { 3 } else { 2 }
            || parts[0].as_os_str() != "reinitialization"
            || candidate && parts[2].as_os_str() != "new"
            || parts[1].as_os_str().to_str().is_none_or(|s| {
                s.len() != 35
                    || !s.starts_with("op-")
                    || !s[3..]
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        Ok(())
    }
    /// Trusted recovery composition only. The candidate is a fixed `new` child
    /// of one opaque operation below this deployment's `recovery` namespace.
    /// Protected ACLs are installed at creation, including the three trees that
    /// may later be renamed into the live deployment.
    pub fn prepare_recovery_candidate(&self, candidate: &Path) -> Result<DeploymentDirectory> {
        self.verify_root_for_current_user()?;
        let relative = candidate
            .strip_prefix(&self.root)
            .map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        let parts: Vec<_> = relative.components().collect();
        if parts.len() != 3
            || parts[0].as_os_str() != "recovery"
            || parts[2].as_os_str() != "new"
            || parts[1].as_os_str().to_str().is_none_or(|s| {
                s.len() != 35
                    || !s.starts_with("op-")
                    || !s[3..]
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        check_local_ancestors(candidate)?;
        let user_sid = current_user_sid()?;
        let expected = Descriptor::for_user(&user_sid)?;
        ensure_directory(candidate, &expected)?;
        for child in CHILDREN.iter().copied().chain(["profile-content"]) {
            ensure_directory(&candidate.join(child), &expected)?;
        }
        let prepared = DeploymentDirectory {
            root: candidate.to_owned(),
            user_sid,
            _thread: PhantomData,
        };
        prepared.read_back()?;
        verify_acl(&candidate.join("profile-content"), &expected)?;
        Ok(prepared)
    }
    /// Hidden update verification owns a fresh synthetic production deployment.
    /// Never adopt an existing root, even if its ACL would otherwise be valid.
    pub fn prepare_fresh_for_current_user(&self) -> Result<DeploymentDirectory> {
        if self.requires_existing_deployment() || self.root != crate::program_data()? {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        self.prepare_fresh_root()
    }
    fn prepare_fresh_root(&self) -> Result<DeploymentDirectory> {
        if self.requires_existing_deployment() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        check_local_ancestors(&self.root)?;
        let user_sid = current_user_sid()?;
        let expected = Descriptor::for_user(&user_sid)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: expected.0 .0,
            bInheritHandle: false.into(),
        };
        // SAFETY: the OS creates this exact fixed root with its protected DACL.
        // AlreadyExists is deliberately propagated, never accepted as ownership.
        unsafe {
            CreateDirectoryW(&HSTRING::from(self.root.as_os_str()), Some(&attributes))?;
        }
        let _root = crate::materials::files::PinnedDirectory::open(&self.root)?;
        verify_acl(&self.root, &expected)?;
        for child in CHILDREN {
            ensure_directory(&self.root.join(child), &expected)?;
        }
        let deployment = DeploymentDirectory {
            root: self.root.clone(),
            user_sid,
            _thread: PhantomData,
        };
        deployment.read_back()?;
        Ok(deployment)
    }
    /// Fixed actual installed GUI destination, discovered without environment
    /// variable input. This is path discovery, not an installation permission.
    pub fn installed_bootstrap_for_current_user() -> Result<PathBuf> {
        use windows::Win32::UI::Shell::{
            FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
        };
        crate::com_task::with_task_memory(|memory| unsafe {
            let raw = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None)?;
            let value = raw.to_string();
            memory.free(raw.0.cast());
            let local = PathBuf::from(value?);
            installation_path_at(&local)
        })
    }
    /// Explicit deployment operation. Creates only this project's directories with
    /// a protected DACL present at creation: SYSTEM, Administrators, deployment user.
    /// Existing roots are verified, never silently repaired or widened.
    pub fn prepare_for_current_user(&self) -> Result<DeploymentDirectory> {
        if matches!(self.origin, RootOrigin::ExistingLocal { .. }) {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        if self.requires_existing_deployment() {
            // A vanished selected old root must not become a new empty school.
            // Cold recovery runs before bootstrap asks to fill missing children.
            self.verify_root_for_current_user()?;
        }
        check_local_ancestors(&self.root)?;
        let user_sid = current_user_sid()?;
        let expected = Descriptor::for_user(&user_sid)?;
        ensure_directory(&self.root, &expected)?;
        for child in CHILDREN {
            ensure_directory(&self.root.join(child), &expected)?;
        }
        Ok(DeploymentDirectory {
            root: self.root.clone(),
            user_sid,
            _thread: PhantomData,
        })
    }

    /// Fixed current-user legacy location only; no environment/path argument.
    /// A known LocalAppData deployment is never opened as a second live writer.
    pub fn local_migration_source() -> Result<Option<Self>> {
        use windows::Win32::UI::Shell::{
            FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
        };
        let root = crate::com_task::with_task_memory(|memory| unsafe {
            let raw = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None)?;
            let value = raw.to_string();
            memory.free(raw.0.cast());
            Ok(PathBuf::from(value?).join("FutureClassroom"))
        })?;
        match std::fs::symlink_metadata(&root) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        check_local_ancestors(&root)?;
        let pin = crate::materials::files::PinnedDirectory::open(&root)?;
        let paths = Self {
            root,
            origin: RootOrigin::ExistingLocal {
                identity: pin.identity()?,
            },
        };
        paths.local_migration_retired()?;
        Ok(Some(paths))
    }
    /// The exact original writable DACL or this migration's read-only DACL.
    /// Unknown ACLs/objects are not repaired or adopted.
    pub fn local_migration_retired(&self) -> Result<bool> {
        let RootOrigin::ExistingLocal { identity } = self.origin else {
            return Err(Error::from_hresult(E_INVALIDARG));
        };
        check_local_ancestors(&self.root)?;
        let pin = crate::materials::files::PinnedDirectory::open(&self.root)?;
        if pin.identity()? != identity {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let sid = current_user_sid()?;
        if verify_acl(&self.root, &Descriptor::for_user(&sid)?).is_ok() {
            return Ok(false);
        }
        verify_acl(&self.root, &Descriptor::for_retired_user(&sid)?)?;
        Ok(true)
    }
    /// Existing isolated-test constructor's controlled namespace only. Capture
    /// the original object identity exactly as KnownFolder discovery does;
    /// ordinary canonical ProgramData can never become a local source here.
    pub fn isolated_migration_source(&self) -> Result<Self> {
        if !matches!(self.origin, RootOrigin::Canonical)
            || self
                .root
                .file_name()
                .and_then(|n| n.to_str())
                .is_none_or(|n| !n.starts_with("FutureClassroom-Test-"))
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let pin = crate::materials::files::PinnedDirectory::open(&self.root)?;
        let selected = Self {
            root: self.root.clone(),
            origin: RootOrigin::ExistingLocal {
                identity: pin.identity()?,
            },
        };
        selected.local_migration_retired()?;
        Ok(selected)
    }
    /// Only the recognized project root changes. Known old startup requires its
    /// exact writable root ACL before it can acquire/open the deployment; the
    /// retired ACL fails that contract, including elevated launches. A root DACL
    /// is not a recursive write revocation: protected descendants keep their own
    /// DACL. The original instance lock and known application's root check are
    /// the contract, not an OS sandbox against programs bypassing that check.
    pub fn set_local_migration_retired(&self, retired: bool) -> Result<()> {
        self.local_migration_retired()?;
        let _root = crate::materials::files::PinnedDirectory::open(&self.root)?;
        let sid = current_user_sid()?;
        let descriptor = if retired {
            Descriptor::for_retired_user(&sid)?
        } else {
            Descriptor::for_user(&sid)?
        };
        set_exact_dacl(&self.root, &descriptor)?;
        verify_acl(&self.root, &descriptor)
    }
    /// Fresh controlled migration outputs only. The caller creates every file
    /// with create_new and retains its owner; existing teaching files are never
    /// repaired by this operation.
    pub fn protect_migration_object(&self, path: &Path, directory: bool) -> Result<()> {
        if !matches!(self.origin, RootOrigin::Canonical)
            || !path.starts_with(&self.root)
            || path == self.root
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let _pin = if directory {
            crate::materials::files::OwnedObjectPin::directory(path)?
        } else {
            crate::materials::files::OwnedObjectPin::file_allow_empty(path)?
        };
        let descriptor = Descriptor::for_user_object(&current_user_sid()?, directory)?;
        set_exact_dacl(path, &descriptor)?;
        verify_acl_contents(path, &descriptor, &HSTRING::from(path.as_os_str()))
    }
}

fn verify_legacy_envelope(legacy: &Path) -> Result<(u32, u64)> {
    use crate::materials::files::PinnedDirectory;
    let canonical = legacy
        .parent()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    check_local_ancestors(legacy)?;
    let _parent = PinnedDirectory::open(canonical)?;
    let root = PinnedDirectory::open(legacy)?;
    let expected = Descriptor::for_user(&current_user_sid()?)?;
    verify_acl(legacy, &expected)?;
    // The outer directory is only a pinned ancestor. Never repair its inherited
    // ACL, adopt it as a second teaching root, or choose between two deployments.
    let outside = legacy_directory_entries(canonical, 2)?;
    if outside.len() != 1 || outside[0] != legacy {
        return Err(Error::new(
            E_INVALIDARG,
            "发现并存或未知资料目录；未选择或修改资料。",
        ));
    }
    // Physical namespaces only. A torn reset can have moved config/state away;
    // only bootstrap's original journal reconcilers may interpret that state.
    const NAMESPACES: &[&str] = &[
        "state",
        "config",
        "runtime",
        "teachers",
        "open-copies",
        "quarantine",
        "cache",
        "exports",
        "profile-content",
        "voice",
        "staging",
        "launch-copies",
        "web-material-staging",
        "recovery",
        "reinitialization",
    ];
    for entry in legacy_directory_entries(legacy, NAMESPACES.len())? {
        let name = entry
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        if !NAMESPACES.contains(&name) {
            return Err(Error::new(
                E_INVALIDARG,
                "旧版资料目录含未知对象；原件已保留。",
            ));
        }
        let _directory = PinnedDirectory::open(&entry)?;
        if CHILDREN.contains(&name) {
            verify_acl(&entry, &expected)?;
        }
    }
    root.identity()
}
fn legacy_directory_entries(path: &Path, limit: usize) -> Result<Vec<PathBuf>> {
    let _pin = crate::materials::files::PinnedDirectory::open(path)?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        if entries.len() == limit {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let entry = entry?.path();
        if std::fs::symlink_metadata(&entry)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0
            != 0
        {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        entries.push(entry);
    }
    Ok(entries)
}

pub struct DeploymentDirectory {
    root: PathBuf,
    user_sid: String,
    _thread: PhantomData<Rc<()>>,
}
impl DeploymentDirectory {
    pub fn user_sid(&self) -> &str {
        &self.user_sid
    }
    pub fn read_back(&self) -> Result<DirectoryAclReadback> {
        check_local_ancestors(&self.root)?;
        if current_user_sid()? != self.user_sid {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let expected = Descriptor::for_user(&self.user_sid)?;
        verify_acl(&self.root, &expected)?;
        for child in CHILDREN {
            verify_acl(&self.root.join(child), &expected)?;
        }
        Ok(DirectoryAclReadback {
            protected: true,
            verified_directories: CHILDREN.len() + 1,
            deployment_sid: self.user_sid.clone(),
        })
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct DirectoryAclReadback {
    pub protected: bool,
    pub verified_directories: usize,
    pub deployment_sid: String,
}

struct Descriptor(PSECURITY_DESCRIPTOR);
impl Descriptor {
    fn for_retired_user(sid: &str) -> Result<Self> {
        let sddl = HSTRING::from(format!(
            "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FRFX;;;{sid})"
        ));
        let mut descriptor = Self(PSECURITY_DESCRIPTOR::default());
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &mut descriptor.0,
                None,
            )?;
        }
        Ok(descriptor)
    }
    fn for_user(sid: &str) -> Result<Self> {
        Self::for_user_object(sid, true)
    }
    fn for_user_object(sid: &str, directory: bool) -> Result<Self> {
        // SID came from the process token, never from user-controlled SDDL text.
        // Windows removes directory inheritance flags from regular-file ACEs;
        // verify files against their actual closed file descriptor instead.
        let inherit = if directory { "OICI" } else { "" };
        let sddl = HSTRING::from(format!(
            "D:P(A;{inherit};FA;;;SY)(A;{inherit};FA;;;BA)(A;{inherit};FA;;;{sid})"
        ));
        let mut descriptor = Self(PSECURITY_DESCRIPTOR::default());
        // SAFETY: bounded NUL-terminated owned string, allocated output is uniquely
        // transferred into Descriptor and freed with LocalFree even on failure.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &mut descriptor.0,
                None,
            )?;
        }
        Ok(descriptor)
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0 .0.is_null() {
            // SAFETY: uniquely owned LocalAlloc security descriptor from Win32.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }
}

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: unique successful token handle; no borrowed SID survives this drop.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub fn current_user_sid() -> Result<String> {
    let mut handle = HANDLE::default();
    // SAFETY: pseudohandle does not transfer ownership; returned token does.
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle)?;
    }
    let token = Token(handle);
    // u64 alignment satisfies TOKEN_USER on both x86 and x64, fixed 4096-byte bound.
    let mut storage = [0u64; 512];
    let mut length = 0;
    // SAFETY: aligned exclusive storage valid throughout call and SID conversion.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(storage.as_mut_ptr().cast()),
            std::mem::size_of_val(&storage) as u32,
            &mut length,
        )?;
        if length as usize > std::mem::size_of_val(&storage)
            || (length as usize) < std::mem::size_of::<TOKEN_USER>()
        {
            return Err(Error::from_hresult(E_UNEXPECTED));
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text)?;
        let result = text
            .to_string()
            .map_err(|_| Error::from_hresult(E_UNEXPECTED));
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        result
    }
}

fn installation_path_at(local: &Path) -> Result<PathBuf> {
    // Discovery is read-only and must also work before the first installation
    // and after uninstall. Validate every existing parent, not a missing suffix.
    check_local_ancestors(&local.join(".install-parent"))?;
    let root = local.join("Programs").join("FutureClassroom");
    for directory in [local.join("Programs"), root.clone()] {
        match std::fs::symlink_metadata(directory) {
            Ok(m) if m.is_dir() && m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            _ => return Err(Error::from_hresult(E_ACCESSDENIED)),
        }
    }
    Ok(root.join("fc-bootstrap.exe"))
}

fn check_local_ancestors(root: &Path) -> Result<()> {
    let text = root
        .to_str()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    let raw = text.as_bytes();
    if raw.len() < 4
        || !raw[0].is_ascii_alphabetic()
        || raw[1] != b':'
        || raw[2] != b'\\'
        || text[2..].contains(':')
        || text.chars().any(|c| c == '"' || c.is_control())
        || root
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    // Never follow existing directory junctions/symlinks to another deployment.
    // The parent must already exist: this operation never creates arbitrary trees.
    for ancestor in root.ancestors().skip(1) {
        no_reparse_directory(ancestor)?;
    }
    Ok(())
}
fn no_reparse_directory(path: &Path) -> Result<()> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| Error::from_hresult(E_ACCESSDENIED))?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Error::from_hresult(E_ACCESSDENIED));
    }
    Ok(())
}
/// Reset-only Win7 extended path. Resolve the already checked parent first;
/// blindly prefixing an input could change DOS dot/space alias semantics.
fn reinitialization_native_name(path: &Path) -> Result<HSTRING> {
    check_local_ancestors(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    let leaf = path
        .file_name()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    let mut canonical = parent.canonicalize()?;
    if !matches!(canonical.components().next(), Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_)))
    {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    canonical.push(leaf);
    use std::os::windows::ffi::OsStrExt;
    if canonical.as_os_str().encode_wide().count() >= 32767 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    Ok(HSTRING::from(canonical.as_os_str()))
}
fn ensure_reinitialization_directory(path: &Path, expected: &Descriptor) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    let _parent = crate::materials::files::PinnedDirectory::open(parent)?;
    let native_name = reinitialization_native_name(path)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: expected.0 .0,
        bInheritHandle: false.into(),
    };
    // SAFETY: the pinned local parent is canonicalized to an extended DOS path;
    // owned descriptor and string remain valid for the synchronous Win7 API.
    if let Err(error) = unsafe { CreateDirectoryW(&native_name, Some(&attributes)) } {
        if error.code() != ERROR_ALREADY_EXISTS.to_hresult() {
            return Err(error);
        }
    }
    let _directory = crate::materials::files::PinnedDirectory::open(path)?;
    verify_acl_named(path, expected, &native_name)
}
fn ensure_directory(path: &Path, expected: &Descriptor) -> Result<()> {
    let path_string = HSTRING::from(path.as_os_str());
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: expected.0 .0,
        bInheritHandle: false.into(),
    };
    // SAFETY: descriptor is valid for synchronous CreateDirectory; ACL is installed
    // atomically with creation, so no broadly accessible initialization window exists.
    let created = unsafe { CreateDirectoryW(&path_string, Some(&attributes)) };
    if let Err(error) = created {
        if error.code() != ERROR_ALREADY_EXISTS.to_hresult() {
            return Err(error);
        }
    }
    verify_acl(path, expected)
}
fn verify_acl(path: &Path, expected: &Descriptor) -> Result<()> {
    verify_acl_named(path, expected, &HSTRING::from(path.as_os_str()))
}
fn verify_acl_named(path: &Path, expected: &Descriptor, name: &HSTRING) -> Result<()> {
    no_reparse_directory(path)?;
    verify_acl_contents(path, expected, name)
}
fn set_exact_dacl(path: &Path, expected: &Descriptor) -> Result<()> {
    unsafe {
        SetNamedSecurityInfoW(
            &HSTRING::from(path.as_os_str()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(acl(expected.0)?),
            None,
        )
        .ok()?;
    }
    Ok(())
}
fn verify_acl_contents(_path: &Path, expected: &Descriptor, name: &HSTRING) -> Result<()> {
    let mut actual = Descriptor(PSECURITY_DESCRIPTOR::default());
    let mut owner = PSID::default();
    // SAFETY: synchronous query; OS returns a valid LocalAlloc descriptor with
    // internal ACL/SID pointers owned by `actual` until all comparisons finish.
    unsafe {
        GetNamedSecurityInfoW(
            name,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            &mut actual.0,
        )
        .ok()?;
        let mut control = 0;
        let mut revision = 0;
        GetSecurityDescriptorControl(actual.0, &mut control, &mut revision)?;
        if control & SE_DACL_PROTECTED.0 == 0 {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let actual_acl = acl(actual.0)?;
        let expected_acl = acl(expected.0)?;
        if (*actual_acl).AceCount != (*expected_acl).AceCount {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        if owner.0.is_null() || !IsValidSid(owner).as_bool() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        let mut trusted_owner = false;
        for index in 0..u32::from((*expected_acl).AceCount) {
            let mut a = std::ptr::null_mut();
            let mut e = std::ptr::null_mut();
            GetAce(actual_acl, index, &mut a)?;
            GetAce(expected_acl, index, &mut e)?;
            // IsValidAcl/GetAce establish a complete ACE_HEADER. Check the
            // variable-length allow ACE before creating its larger typed view.
            let header = std::ptr::read_unaligned(a.cast::<ACE_HEADER>());
            let expected_header = std::ptr::read_unaligned(e.cast::<ACE_HEADER>());
            if header.AceType != expected_header.AceType || header.AceSize < 16 {
                return Err(Error::from_hresult(E_ACCESSDENIED));
            }
            let sid_bytes = a.cast::<u8>().add(8);
            let subauthorities = usize::from(*sid_bytes.add(1));
            if 16 + subauthorities * 4 > usize::from(header.AceSize)
                || !IsValidSid(PSID(sid_bytes.cast())).as_bool()
            {
                return Err(Error::from_hresult(E_ACCESSDENIED));
            }
            let a = &*a.cast::<ACCESS_ALLOWED_ACE>();
            let e = &*e.cast::<ACCESS_ALLOWED_ACE>();
            if a.Header.AceSize < std::mem::size_of::<ACCESS_ALLOWED_ACE>() as u16
                || a.Header.AceType != e.Header.AceType
                || a.Header.AceFlags != e.Header.AceFlags
                || a.Mask != e.Mask
            {
                return Err(Error::from_hresult(E_ACCESSDENIED));
            }
            EqualSid(
                PSID((&raw const a.SidStart).cast_mut().cast()),
                PSID((&raw const e.SidStart).cast_mut().cast()),
            )?;
            // Reject a pre-created project tree owned by another ordinary user:
            // its owner could otherwise later replace this seemingly exact DACL.
            trusted_owner |=
                EqualSid(owner, PSID((&raw const e.SidStart).cast_mut().cast())).is_ok();
        }
        if !trusted_owner {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
    }
    Ok(())
}
unsafe fn acl(descriptor: PSECURITY_DESCRIPTOR) -> Result<*mut ACL> {
    let mut present = windows::core::BOOL::default();
    let mut defaulted = windows::core::BOOL::default();
    let mut acl = std::ptr::null_mut();
    // SAFETY: caller retains valid self-relative descriptor from the OS throughout.
    unsafe {
        GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted)?;
        if !present.as_bool() || acl.is_null() || !IsValidAcl(acl).as_bool() {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
    }
    Ok(acl)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn scratch(label: &str) -> Scratch {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Scratch(std::env::temp_dir().join(format!(
            "FutureClassroom-Test-{label}-{}-{nonce}",
            std::process::id()
        )))
    }
    fn legacy_fixture() -> (Scratch, PathBuf, PathBuf) {
        let scratch = scratch("LegacyLayout");
        let canonical = scratch.0.join("canonical");
        std::fs::create_dir_all(&canonical).unwrap();
        let legacy = canonical.join("FutureClassroom");
        DataPaths {
            root: legacy.clone(),
            origin: RootOrigin::Canonical,
        }
        .prepare_for_current_user()
        .unwrap()
        .read_back()
        .unwrap();
        (scratch, canonical, legacy)
    }
    #[test]
    fn installation_path_discovery_survives_uninstall_without_creating_directories() {
        let scratch = scratch("InstallDiscovery");
        std::fs::create_dir_all(&scratch.0).unwrap();
        let programs = scratch.0.join("Programs");
        let root = programs.join("FutureClassroom");
        let expected = root.join("fc-bootstrap.exe");
        assert_eq!(installation_path_at(&scratch.0).unwrap(), expected);
        assert!(!programs.exists());
        std::fs::create_dir(&programs).unwrap();
        assert_eq!(installation_path_at(&scratch.0).unwrap(), expected);
        assert!(!root.exists());
        std::fs::create_dir(&root).unwrap();
        assert_eq!(installation_path_at(&scratch.0).unwrap(), expected);
        std::fs::remove_dir(&root).unwrap();
        assert_eq!(installation_path_at(&scratch.0).unwrap(), expected);
        assert!(!root.exists());
        std::fs::write(&root, b"not an installation directory").unwrap();
        assert!(installation_path_at(&scratch.0).is_err());
    }
    #[test]
    fn absent_application_root_discovery_is_canonical_and_read_only() {
        let scratch = scratch("CanonicalDiscovery");
        let paths = DataPaths::discover_program_data_at(scratch.0.clone()).unwrap();
        assert_eq!(paths.root(), scratch.0);
        assert!(!paths.requires_existing_deployment());
        assert!(!scratch.0.exists());
    }
    #[test]
    fn unknown_nested_root_is_refused_without_modifying_or_repairing_data() {
        let scratch = scratch("UnknownLegacy");
        let legacy = scratch.0.join("FutureClassroom");
        std::fs::create_dir_all(&legacy).unwrap();
        let sentinel = legacy.join("sentinel.txt");
        std::fs::write(&sentinel, b"preserved synthetic old deployment").unwrap();
        assert!(DataPaths::discover_program_data_at(scratch.0.clone()).is_err());
        assert_eq!(
            std::fs::read(&sentinel).unwrap(),
            b"preserved synthetic old deployment"
        );
        assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 1);
        assert!(!scratch.0.join("state").exists());
    }
    #[test]
    fn existing_legacy_keeps_original_lock_and_missing_cold_trees_are_not_created() {
        let (_scratch, canonical, legacy) = legacy_fixture();
        for name in ["config", "state", "teachers"] {
            std::fs::remove_dir(legacy.join(name)).unwrap();
        }
        let expected = Descriptor::for_user(&current_user_sid().unwrap()).unwrap();
        assert!(verify_acl(&canonical, &expected).is_err());
        let paths = DataPaths::discover_program_data_at(canonical.clone()).unwrap();
        assert_eq!(paths.root(), legacy);
        assert!(paths.requires_existing_deployment());
        assert!(paths.prepare_fresh_root().is_err());
        assert!(paths.prepare_fresh_for_current_user().is_err());
        let original = crate::InstanceGuard::acquire(&legacy).unwrap();
        assert!(crate::InstanceGuard::acquire(paths.root()).is_err());
        drop(original);
        drop(crate::InstanceGuard::acquire(paths.root()).unwrap());
        for name in ["config", "state", "teachers"] {
            assert!(!legacy.join(name).exists());
            assert!(!canonical.join(name).exists());
        }
        assert!(verify_acl(&canonical, &expected).is_err());
    }
    #[test]
    fn ambiguous_or_replaced_legacy_root_is_not_chosen_or_recreated() {
        let (scratch, canonical, legacy) = legacy_fixture();
        let conflict = canonical.join("state");
        std::fs::create_dir(&conflict).unwrap();
        assert!(DataPaths::discover_program_data_at(canonical.clone()).is_err());
        assert!(conflict.is_dir());
        std::fs::remove_dir(conflict).unwrap();
        let paths = DataPaths::discover_program_data_at(canonical.clone()).unwrap();
        std::fs::rename(&legacy, scratch.0.join("preserved-original")).unwrap();
        assert!(paths.prepare_for_current_user().is_err());
        assert!(!legacy.exists());
        DataPaths {
            root: legacy.clone(),
            origin: RootOrigin::Canonical,
        }
        .prepare_for_current_user()
        .unwrap();
        let sentinel = legacy.join("config/sentinel");
        std::fs::write(&sentinel, b"replacement survives").unwrap();
        assert!(paths.verify_root_for_current_user().is_err());
        assert!(paths.prepare_for_current_user().is_err());
        assert_eq!(std::fs::read(sentinel).unwrap(), b"replacement survives");
    }
    #[test]
    fn fresh_protected_root_refuses_existing_sentinel_without_repair() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "FutureClassroom-Test-Fresh-{}-{nonce}",
            std::process::id()
        )));
        let paths = DataPaths::isolated_test(scratch.0.clone()).unwrap();
        let prepared = paths.prepare_fresh_root().unwrap();
        assert!(prepared.read_back().unwrap().protected);
        let sentinel = paths.root().join("sentinel.txt");
        std::fs::write(&sentinel, b"owned synthetic sentinel").unwrap();
        assert_eq!(
            paths.prepare_fresh_root().err().unwrap().code(),
            ERROR_ALREADY_EXISTS.to_hresult()
        );
        assert_eq!(
            std::fs::read(&sentinel).unwrap(),
            b"owned synthetic sentinel"
        );
        paths.verify_root_for_current_user().unwrap();
        // The public production-only entry cannot adopt even an owned test root.
        assert!(paths.prepare_fresh_for_current_user().is_err());
    }
    #[test]
    fn isolated_deployment_acl_and_cross_handle_exclusion() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "FutureClassroom-Test-{}-{nonce}",
            std::process::id()
        )));
        let paths = DataPaths::isolated_test(scratch.0.clone()).unwrap();
        assert!(!paths.root().exists());
        let prepared = paths.prepare_for_current_user().unwrap();
        let readback = prepared.read_back().unwrap();
        assert!(readback.protected);
        assert_eq!(readback.verified_directories, 9);
        assert_eq!(readback.deployment_sid, current_user_sid().unwrap());
        let lock = crate::InstanceGuard::acquire(paths.root()).unwrap();
        assert!(crate::InstanceGuard::acquire(paths.root()).is_err());
        drop(lock);
        drop(crate::InstanceGuard::acquire(paths.root()).unwrap());
        drop(prepared);
        paths
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        drop(scratch);
        assert!(!paths.root().exists());
        // A pre-existing inherited ACL must be rejected rather than widened or
        // silently claimed as a verified fixed-account installation.
        std::fs::create_dir(paths.root()).unwrap();
        assert!(paths.prepare_for_current_user().is_err());
        std::fs::remove_dir(paths.root()).unwrap();
        assert!(DataPaths::isolated_test(PathBuf::from(r"C:\Users")).is_err());
    }
}
