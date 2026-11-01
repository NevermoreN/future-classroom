//! Trusted, fixed-scope reconciliation. No caller boolean or notification can
//! manufacture this proof. The scope is the approved payload, installer-owned
//! component markers, and MSI product/context/version registration. Installer
//! engine cache/log internals are not deleted or claimed absent here.
use super::*;

const MARKERS: &str = r"Software\FutureClassroom\Installer\Components";
const PIN_GROUPS: usize = 6; // + session + original Maintenance = original 8 slots

fn local() -> Result<PathBuf> {
    Ok(InstallIdentity::for_current_user()?.local)
}
fn root() -> Result<PathBuf> {
    Ok(local()?.join("Programs").join("FutureClassroom"))
}
pub(super) fn target(untrusted: &str) -> Result<()> {
    // Append the fixed filename in the MSI formatted command. Quoting a bare
    // CostFinalize directory (which ends in '\\') would escape its closing
    // quote in the CRT argv grammar and reject every legitimate fresh install.
    let path = PathBuf::from(untrusted);
    // Do not canonicalize/open the caller's directory: comparison never grants
    // it authority. No UNC/verbatim/ADS/relative/traversal or slash alias enters.
    valid_path(&path)?;
    if !path
        .to_string_lossy()
        .eq_ignore_ascii_case(&root()?.join("fc-bootstrap.exe").to_string_lossy())
    {
        return Err(LifecycleError::Invalid);
    }
    Ok(())
}

fn product(manifest: &Manifest, property: &str) -> Result<Option<String>> {
    let code = HSTRING::from(format!("{{{}}}", manifest.product));
    let property = HSTRING::from(property);
    let mut buffer = [0u16; 256];
    let mut n = buffer.len() as u32;
    let result = unsafe {
        MsiGetProductInfoExW(
            &code,
            None,
            MSIINSTALLCONTEXT_USERUNMANAGED,
            &property,
            Some(windows::core::PWSTR(buffer.as_mut_ptr())),
            Some(&mut n),
        )
    };
    if result == ERROR_UNKNOWN_PRODUCT.0 {
        return Ok(None);
    }
    if result != 0 || n as usize >= buffer.len() {
        return Err(LifecycleError::Unsettled);
    }
    String::from_utf16(&buffer[..n as usize])
        .map(Some)
        .map_err(|_| LifecycleError::Invalid)
}
fn no_product(manifest: &Manifest) -> Result<()> {
    let code = HSTRING::from(format!("{{{}}}", manifest.product));
    let mut found = [0u16; 39];
    let mut context = MSIINSTALLCONTEXT_NONE;
    let result = unsafe {
        MsiEnumProductsExW(
            &code,
            None,
            MSIINSTALLCONTEXT_ALL.0 as u32,
            0,
            Some(&mut found),
            Some(&mut context),
            None,
            None,
        )
    };
    if result != ERROR_NO_MORE_ITEMS.0 {
        return Err(LifecycleError::Unsettled);
    }
    let upgrade = HSTRING::from(format!("{{{INSTALL_FAMILY}}}"));
    let result = unsafe {
        MsiEnumRelatedProductsW(&upgrade, None, 0, windows::core::PWSTR(found.as_mut_ptr()))
    };
    if result != ERROR_NO_MORE_ITEMS.0 {
        return Err(LifecycleError::Unsettled);
    }
    Ok(())
}
fn absent(path: &Path) -> Result<()> {
    valid_path(path)?;
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(LifecycleError::Unsettled),
    }
}
fn marker_absent() -> Result<()> {
    let name = HSTRING::from(MARKERS);
    let mut key = HKEY::default();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &name,
            Some(0),
            KEY_READ | KEY_WOW64_32KEY,
            &mut key,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if result == ERROR_SUCCESS {
        let mut owner = Resources::empty()?;
        owner.add(HANDLE(key.0), Role::Registry);
        owner.release()?;
    }
    Err(LifecycleError::Unsettled)
}
pub(super) fn fresh(session: &mut Session) -> Result<()> {
    current(session.deadline)?;
    no_product(&session.manifest)?;
    let local = local()?;
    // Existing Programs must be a real directory. Its exact handle remains
    // pinned through this session; absence is permitted for a fresh MSI only.
    let programs = local.join("Programs");
    match session.resources.open_file(
        &programs,
        FILE_READ_ATTRIBUTES.0 | READ_CONTROL.0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        OPEN_EXISTING,
        true,
        false,
    ) {
        Ok(h) => {
            session.resources.add(h, Role::Parent);
            verify_info(h, true)?;
            verify_acl(h, &session.resources.sid, false)?;
        }
        Err(e)
            if e.code() == ERROR_FILE_NOT_FOUND.to_hresult()
                || e.code() == ERROR_PATH_NOT_FOUND.to_hresult() => {}
        Err(e) => return Err(e.into()),
    }
    absent(&local.join("Programs").join("FutureClassroom"))?;
    marker_absent()?;
    current(session.deadline)
}

/// Each group uses the existing checked native ledger (64 resources/8 owners),
/// with at most six groups for <=72 files and <=4 path components. No unbounded
/// owner pool or additional native resource budget is introduced.
pub(super) struct Proof {
    groups: Vec<Resources>,
    deadline: Instant,
    returned: bool,
    marker_watch: Option<HANDLE>,
}
impl Proof {
    fn new(deadline: Instant) -> Self {
        Self {
            groups: Vec::new(),
            deadline,
            returned: false,
            marker_watch: None,
        }
    }
    fn owner(&mut self) -> Result<&mut Resources> {
        current(self.deadline)?;
        if self
            .groups
            .last()
            .is_none_or(|g| g.handles.len() > HANDLE_LIMIT - 4)
        {
            if self.groups.len() >= PIN_GROUPS {
                return Err(LifecycleError::Invalid);
            }
            self.groups.push(Resources::empty()?);
        }
        self.groups.last_mut().ok_or(LifecycleError::Invalid)
    }
    pub(super) fn check(&self) -> Result<()> {
        current(self.deadline)?;
        if self.returned {
            return Err(LifecycleError::Invalid);
        }
        for owner in &self.groups {
            owner.check_live()?;
        }
        if self
            .marker_watch
            .is_some_and(|event| unsafe { WaitForSingleObject(event, 0) } != WAIT_TIMEOUT)
        {
            return Err(LifecycleError::Unsettled);
        }
        Ok(())
    }
    pub(super) fn release(&mut self) -> Result<()> {
        self.check()?;
        for group in self.groups.iter_mut().rev() {
            group.release()?;
        }
        self.returned = true;
        Ok(())
    }
    fn retain(&mut self) {
        for group in &mut self.groups {
            group.retain_until_exit();
        }
        self.returned = true;
    }
}
impl Drop for Proof {
    fn drop(&mut self) {
        if !self.returned && self.release().is_err() {
            self.retain();
        }
    }
}

fn pin_directories(
    proof: &mut Proof,
    base: &Path,
    manifest: &Manifest,
    sid: &[u8],
) -> Result<BTreeSet<PathBuf>> {
    let mut dirs = BTreeSet::new();
    dirs.insert(base.parent().ok_or(LifecycleError::Invalid)?.to_owned());
    dirs.insert(base.to_owned());
    for entry in &manifest.entries {
        let path = base.join(&entry.path);
        let mut parent = path.parent();
        while let Some(dir) = parent {
            if !dir.starts_with(base) {
                break;
            }
            dirs.insert(dir.to_owned());
            parent = dir.parent();
        }
    }
    for dir in &dirs {
        valid_path(dir)?;
        let owner = proof.owner()?;
        let h = owner.open_file(
            dir,
            FILE_READ_ATTRIBUTES.0 | FILE_LIST_DIRECTORY.0 | READ_CONTROL.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            true,
            false,
        )?;
        owner.add(h, Role::Parent);
        verify_info(h, true)?;
        verify_acl(h, sid, false)?;
    }
    Ok(dirs)
}
fn exact_names(
    proof: &mut Proof,
    base: &Path,
    dirs: &BTreeSet<PathBuf>,
    manifest: &Manifest,
) -> Result<()> {
    let expected: BTreeSet<_> = manifest
        .entries
        .iter()
        .map(|e| base.join(&e.path))
        .chain(dirs.iter().filter(|d| d.starts_with(base)).cloned())
        .collect();
    for dir in dirs.iter().filter(|d| d.starts_with(base)) {
        let name = native_path(&dir.join("*"))?;
        let owner = proof.owner()?;
        let mut found = WIN32_FIND_DATAW::default();
        let search = unsafe { FindFirstFileW(&name, &mut found)? };
        owner.add(search, Role::Search);
        let mut entries = 0usize;
        loop {
            current(proof.deadline)?;
            let n = found
                .cFileName
                .iter()
                .position(|&c| c == 0)
                .ok_or(LifecycleError::Invalid)?;
            let name = OsString::from_wide(&found.cFileName[..n]);
            if name != "." && name != ".." {
                entries += 1;
                if entries > 144 || found.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                    return Err(LifecycleError::Unsettled);
                }
                let path = dir.join(name);
                if !expected.contains(&path) {
                    return Err(LifecycleError::Unsettled);
                }
            }
            match unsafe { FindNextFileW(search, &mut found) } {
                Ok(()) => {}
                Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
                Err(e) => return Err(e.into()),
            }
        }
        // The search belongs to the same fixed group that created it.
        let owner = proof
            .groups
            .iter_mut()
            .find(|g| {
                g.handles
                    .iter()
                    .any(|h| h.handle == search && h.role == Role::Search)
            })
            .ok_or(LifecycleError::Invalid)?;
        owner.close_handle(search)?;
    }
    Ok(())
}
fn files(proof: &mut Proof, base: &Path, manifest: &Manifest, sid: &[u8]) -> Result<()> {
    let dirs = pin_directories(proof, base, manifest, sid)?;
    exact_names(proof, base, &dirs, manifest)?;
    for entry in &manifest.entries {
        let deadline = proof.deadline;
        let owner = proof.owner()?;
        let h = owner.open_file(
            &base.join(&entry.path),
            GENERIC_READ.0 | READ_CONTROL.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        owner.add(h, Role::Parent);
        verify_info(h, false)?;
        let mut n = 0;
        unsafe {
            GetFileSizeEx(h, &mut n)?;
        }
        if n != i64::from(entry.bytes)
            || owner.digest(h, u64::from(manifest::MAX_PAYLOAD), deadline)? != entry.sha256
        {
            return Err(LifecycleError::Unsettled);
        }
    }
    Ok(())
}
fn markers(proof: &mut Proof, manifest: &Manifest) -> Result<()> {
    let name = HSTRING::from(MARKERS);
    let mut key = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &name,
            Some(0),
            KEY_READ | KEY_WOW64_32KEY,
            &mut key,
        )
        .ok()?;
    }
    proof.owner()?.add(HANDLE(key.0), Role::Registry);
    // Observe actual writes throughout enumeration and the final proof checks.
    // This is not a claimed registry CAS or Run-value deletion primitive.
    let event = unsafe { CreateEventW(None, true, false, None)? };
    proof.owner()?.add(event, Role::Control);
    unsafe {
        RegNotifyChangeKeyValue(
            key,
            true,
            REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
            Some(event),
            true,
        )
        .ok()?;
    }
    proof.marker_watch = Some(event);
    let mut subkeys = 0;
    let mut values = 0;
    unsafe {
        RegQueryInfoKeyW(
            key,
            None,
            None,
            None,
            Some(&mut subkeys),
            None,
            None,
            Some(&mut values),
            None,
            None,
            None,
            None,
        )
        .ok()?;
    }
    if subkeys != 0 || values as usize != manifest.entries.len() {
        return Err(LifecycleError::Unsettled);
    }
    let expected: BTreeSet<_> = manifest
        .entries
        .iter()
        .map(|e| e.component.clone())
        .collect();
    let mut seen = BTreeSet::new();
    for index in 0..=72u32 {
        let mut name = [0u16; 40];
        let mut n = name.len() as u32;
        let mut kind = 0u32;
        let mut data = [0u8; 4];
        let mut size = 4;
        let status = unsafe {
            RegEnumValueW(
                key,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut n,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            return if seen == expected {
                Ok(())
            } else {
                Err(LifecycleError::Unsettled)
            };
        }
        if status != ERROR_SUCCESS
            || n as usize >= name.len()
            || kind != REG_DWORD.0
            || size != 4
            || u32::from_le_bytes(data) != 1
        {
            return Err(LifecycleError::Unsettled);
        }
        let value = String::from_utf16(&name[..n as usize]).map_err(|_| LifecycleError::Invalid)?;
        if !expected.contains(&value) || !seen.insert(value) {
            return Err(LifecycleError::Unsettled);
        }
    }
    Err(LifecycleError::Unsettled)
}
fn component_registration(manifest: &Manifest) -> Result<()> {
    let code = HSTRING::from(format!("{{{}}}", manifest.product));
    for entry in &manifest.entries {
        let component = HSTRING::from(format!("{{{}}}", entry.component));
        let mut path = [0u16; PATH_LIMIT];
        let mut n = path.len() as u32;
        let state = unsafe {
            MsiGetComponentPathExW(
                &code,
                &component,
                None,
                Some(MSIINSTALLCONTEXT_USERUNMANAGED),
                Some(windows::core::PWSTR(path.as_mut_ptr())),
                Some(&mut n),
            )
        };
        if state != INSTALLSTATE_LOCAL || n == 0 || n as usize >= path.len() {
            return Err(LifecycleError::Unsettled);
        }
    }
    Ok(())
}
pub(super) fn staged(session: &mut Session) -> Result<bool> {
    let mut proof = Proof::new(session.deadline);
    let base = root()?;
    let observed = files(&mut proof, &base, &session.manifest, &session.resources.sid)
        .and_then(|()| markers(&mut proof, &session.manifest));
    // These pins must ACTUALLY close before End: a rollback needs DELETE access.
    // Cleanup failure is fatal, never a reason to authorize End or replacement.
    proof.release()?;
    current(session.deadline)?;
    Ok(observed.is_ok())
}
pub(super) fn reconcile(session: &mut Session, outcome: SettledOutcome) -> Result<Proof> {
    let mut proof = Proof::new(session.deadline);
    let base = root()?;
    match outcome {
        SettledOutcome::Commit => {
            if product(&session.manifest, "VersionString")?.as_deref()
                != Some(session.manifest.version.as_str())
            {
                return Err(LifecycleError::Unsettled);
            }
            component_registration(&session.manifest)?;
            files(&mut proof, &base, &session.manifest, &session.resources.sid)?;
            markers(&mut proof, &session.manifest)?;
        }
        SettledOutcome::Rollback => {
            no_product(&session.manifest)?;
            absent(&base)?;
            marker_absent()?;
        }
    }
    proof.check()?;
    Ok(proof)
}

#[cfg(test)]
mod tests {
    use super::super::super::super::broker::manifest::Entry;
    use super::*;
    use std::{fs, os::windows::io::AsRawHandle};

    struct Tree(PathBuf);
    impl Drop for Tree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("owned effects fixture cleaned");
        }
    }
    fn fixture() -> (Tree, PathBuf, Manifest, Vec<u8>) {
        use fc_ports::RandomSource;
        let mut nonce = [0; 16];
        crate::SystemRandom.fill(&mut nonce).unwrap();
        let name: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        let tree = Tree(std::env::temp_dir().join(format!(
            "FutureClassroom-Test-InstallerEffects-{}-{name}",
            std::process::id()
        )));
        fs::create_dir(&tree.0).unwrap();
        let base = tree.0.join("install");
        fs::create_dir(&base).unwrap();
        fs::create_dir(base.join("web")).unwrap();
        fs::create_dir(base.join("web/assets")).unwrap();
        let mut owner = Resources::empty().unwrap();
        let sid = owner.token_sid(unsafe { GetCurrentProcess() }).unwrap();
        let mut entries = Vec::new();
        for (i, path) in [
            "LICENSE",
            "MSI-CANDIDATE.txt",
            "PROBE-README.txt",
            "THIRD-PARTY-NOTICES.txt",
            "fc-bootstrap.exe",
            "manifest.json",
            "web/THIRD-PARTY-NOTICES.txt",
            "web/assets/main.js",
            "web/fc-assets.json",
            "web/index.html",
        ]
        .iter()
        .enumerate()
        {
            let bytes = format!("public effects fixture {i}\n").into_bytes();
            let file = base.join(path);
            fs::write(&file, &bytes).unwrap();
            let file = fs::File::open(file).unwrap();
            let sha256 = owner
                .digest(
                    HANDLE(file.as_raw_handle()),
                    1024,
                    Instant::now() + Duration::from_secs(10),
                )
                .unwrap();
            entries.push(Entry {
                path: (*path).to_owned(),
                bytes: bytes.len() as u32,
                sha256,
                component: format!("00000000-0000-0000-0000-{:012X}", i + 1),
            });
        }
        owner.release().unwrap();
        (
            tree,
            base,
            Manifest {
                product: "12345678-1234-1234-1234-123456789ABC".into(),
                version: "0.1.0".into(),
                entries,
            },
            sid,
        )
    }
    #[test]
    fn actual_closed_file_effects_pin_bytes_and_reject_extra_changed_and_linked_files() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_tree, base, manifest, sid) = fixture();
        let mut proof = Proof::new(Instant::now() + Duration::from_secs(10));
        files(&mut proof, &base, &manifest, &sid).unwrap();
        proof.check().unwrap();
        assert!(fs::OpenOptions::new()
            .write(true)
            .open(base.join("fc-bootstrap.exe"))
            .is_err());
        assert!(fs::rename(&base, base.with_file_name("unexpected-move")).is_err());
        proof.release().unwrap();
        fs::write(base.join("unlisted.txt"), b"must reject").unwrap();
        let mut proof = Proof::new(Instant::now() + Duration::from_secs(10));
        assert!(files(&mut proof, &base, &manifest, &sid).is_err());
        proof.release().unwrap();
        fs::remove_file(base.join("unlisted.txt")).unwrap();
        let mut wrong = manifest;
        wrong.entries[0].sha256[0] ^= 1;
        let mut proof = Proof::new(Instant::now() + Duration::from_secs(10));
        assert!(files(&mut proof, &base, &wrong, &sid).is_err());
        proof.release().unwrap();
        wrong.entries[0].sha256[0] ^= 1;
        let alias = base.parent().unwrap().join("outside-link.txt");
        fs::hard_link(base.join("LICENSE"), &alias).unwrap();
        let mut proof = Proof::new(Instant::now() + Duration::from_secs(10));
        assert!(files(&mut proof, &base, &wrong, &sid).is_err());
        proof.release().unwrap();
        fs::remove_file(alias).unwrap();
    }
    #[test]
    fn actual_target_fact_cannot_select_directory_or_alias() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let expected = root().unwrap().join("fc-bootstrap.exe");
        assert!(target(expected.to_str().unwrap()).is_ok());
        for path in [
            root().unwrap(),
            expected.with_file_name("other.exe"),
            expected.with_extension("exe:ads"),
            root().unwrap().join("..").join("fc-bootstrap.exe"),
        ] {
            assert!(target(path.to_str().unwrap()).is_err());
        }
    }
}
