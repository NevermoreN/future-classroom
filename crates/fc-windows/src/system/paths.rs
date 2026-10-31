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
}
impl DataPaths {
    pub fn program_data() -> Result<Self> {
        Ok(Self {
            root: crate::program_data()?.join("FutureClassroom"),
        })
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
        Ok(Self { root })
    }
    pub fn root(&self) -> &Path {
        &self.root
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
        check_local_ancestors(&self.root)?;
        verify_acl(&self.root, &Descriptor::for_user(&current_user_sid()?)?)
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
    /// Explicit deployment operation. Creates only this project's directories with
    /// a protected DACL present at creation: SYSTEM, Administrators, deployment user.
    /// Existing roots are verified, never silently repaired or widened.
    pub fn prepare_for_current_user(&self) -> Result<DeploymentDirectory> {
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
    fn for_user(sid: &str) -> Result<Self> {
        // SID came from the process token, never from user-controlled SDDL text.
        let sddl = HSTRING::from(format!(
            "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{sid})"
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
