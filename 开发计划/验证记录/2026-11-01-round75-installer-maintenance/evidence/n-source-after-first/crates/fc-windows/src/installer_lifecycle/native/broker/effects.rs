//! Trusted, fixed-scope reconciliation. No caller boolean or notification can
//! manufacture this proof. The scope is the approved payload, installer-owned
//! component markers, and MSI product/context/version registration. Installer
//! engine cache/log internals are not deleted or claimed absent here.
use super::*;

const MARKERS: &str = r"Software\FutureClassroom\Installer\Components";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const PIN_GROUPS: usize = 6; // + session + original Maintenance = original 8 slots

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    Fresh,
    SameManifest,
}
impl Admission {
    pub(super) fn operation(self) -> MaintenanceOperation {
        match self {
            Self::Fresh => MaintenanceOperation::Install,
            Self::SameManifest => MaintenanceOperation::SameManifest,
        }
    }
    fn preimage(self) -> ExpectedState {
        match self {
            Self::Fresh => ExpectedState::Absent,
            Self::SameManifest => ExpectedState::Present,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExpectedState {
    Present,
    Absent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Observation {
    Present,
    Absent,
    Other,
}
/// Only this module constructs decisions from native observations. Neither
/// notification packets nor public installer properties can choose this state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Decision {
    outcome: SettledOutcome,
    expected: ExpectedState,
}
impl Decision {
    fn observed(admission: Admission, observed: Observation) -> Self {
        let expected = match (admission, observed) {
            (_, Observation::Present) => Some(ExpectedState::Present),
            (Admission::SameManifest, Observation::Absent) => Some(ExpectedState::Absent),
            _ => None,
        };
        match expected {
            Some(expected) => Self {
                outcome: SettledOutcome::Commit,
                expected,
            },
            None => Self {
                outcome: SettledOutcome::Rollback,
                expected: admission.preimage(),
            },
        }
    }
    pub(super) fn outcome(self) -> SettledOutcome {
        self.outcome
    }
}

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
    if !known_product_absence(result) {
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
fn known_product_absence(result: u32) -> bool {
    // EnumProductsEx explicitly distinguishes an exhausted enumeration from a
    // specified product unknown in all requested contexts. Both are absence;
    // access/configuration/argument errors are never converted to absence.
    result == ERROR_NO_MORE_ITEMS.0 || result == ERROR_UNKNOWN_PRODUCT.0
}
fn exact_product_code(buffer: &[u16; 39], product: &str) -> bool {
    buffer[38] == 0
        && String::from_utf16(&buffer[..38])
            .is_ok_and(|s| s.eq_ignore_ascii_case(&format!("{{{product}}}")))
}
fn exact_user_sid(text: &str, sid: &[u8]) -> bool {
    // Compare the returned text with the already-copied native TOKEN_USER SID.
    // No LocalAlloc capability or unbounded SID-derived allocation is needed.
    if sid.len() < 8 || sid[0] != 1 || sid[1] > 15 || sid.len() != 8 + usize::from(sid[1]) * 4 {
        return false;
    }
    let mut fields = text.split('-');
    if fields.next() != Some("S") || fields.next() != Some("1") {
        return false;
    }
    let Some(authority) = fields.next() else {
        return false;
    };
    let authority = if let Some(hex) = authority.strip_prefix("0x") {
        if hex.is_empty() || hex.len() > 12 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return false;
        }
        u64::from_str_radix(hex, 16).ok()
    } else if !authority.is_empty() && authority.bytes().all(|b| b.is_ascii_digit()) {
        authority.parse::<u64>().ok()
    } else {
        None
    };
    let expected = sid[2..8].iter().fold(0u64, |a, &b| (a << 8) | u64::from(b));
    if authority != Some(expected) {
        return false;
    }
    for bytes in sid[8..].chunks_exact(4) {
        let Some(field) = fields.next() else {
            return false;
        };
        if field.is_empty()
            || !field.bytes().all(|b| b.is_ascii_digit())
            || field.parse::<u32>().ok() != Some(u32::from_le_bytes(bytes.try_into().unwrap()))
        {
            return false;
        }
    }
    fields.next().is_none()
}
fn same_registration(manifest: &Manifest, sid: &[u8]) -> Result<()> {
    let code = HSTRING::from(format!("{{{}}}", manifest.product));
    let mut found = [0u16; 39];
    let mut context = MSIINSTALLCONTEXT_NONE;
    let mut user = [0u16; 256];
    let mut n = user.len() as u32;
    let result = unsafe {
        MsiEnumProductsExW(
            &code,
            None,
            MSIINSTALLCONTEXT_ALL.0 as u32,
            0,
            Some(&mut found),
            Some(&mut context),
            Some(windows::core::PWSTR(user.as_mut_ptr())),
            Some(&mut n),
        )
    };
    if result != ERROR_SUCCESS.0
        || context != MSIINSTALLCONTEXT_USERUNMANAGED
        || !exact_product_code(&found, &manifest.product)
        || n == 0
        || n as usize >= user.len()
        || user[n as usize] != 0
        || !String::from_utf16(&user[..n as usize]).is_ok_and(|text| exact_user_sid(&text, sid))
    {
        return Err(LifecycleError::Unsettled);
    }
    // A second instance in managed/machine context is not a healthy per-user
    // preimage. Only the documented end of this successful enumeration is OK.
    let next = unsafe {
        MsiEnumProductsExW(
            &code,
            None,
            MSIINSTALLCONTEXT_ALL.0 as u32,
            1,
            None,
            None,
            None,
            None,
        )
    };
    if next != ERROR_NO_MORE_ITEMS.0 {
        return Err(LifecycleError::Unsettled);
    }
    let upgrade = HSTRING::from(format!("{{{INSTALL_FAMILY}}}"));
    let first = unsafe {
        MsiEnumRelatedProductsW(&upgrade, None, 0, windows::core::PWSTR(found.as_mut_ptr()))
    };
    if first != ERROR_SUCCESS.0 || !exact_product_code(&found, &manifest.product) {
        return Err(LifecycleError::Unsettled);
    }
    let next = unsafe {
        MsiEnumRelatedProductsW(&upgrade, None, 1, windows::core::PWSTR(found.as_mut_ptr()))
    };
    if next != ERROR_NO_MORE_ITEMS.0
        || product(manifest, "VersionString")?.as_deref() != Some(manifest.version.as_str())
    {
        return Err(LifecycleError::Unsettled);
    }
    component_registration(manifest)
}
fn run_absent_at(path: &str) -> Result<()> {
    let mut owner = Resources::empty()?;
    let name = HSTRING::from(path);
    let mut key = HKEY::default();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &name,
            Some(0),
            KEY_QUERY_VALUE | KEY_WOW64_32KEY,
            &mut key,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        owner.release()?;
        return Ok(());
    }
    result.ok()?;
    owner.add(HANDLE(key.0), Role::Registry);
    let mut kind = REG_VALUE_TYPE::default();
    let mut bytes = 0;
    let result = unsafe {
        RegQueryValueExW(
            key,
            windows::core::w!("FutureClassroom"),
            None,
            Some(&mut kind),
            None,
            Some(&mut bytes),
        )
    };
    owner.release()?;
    match result {
        ERROR_FILE_NOT_FOUND => Ok(()),
        ERROR_SUCCESS | ERROR_MORE_DATA => Err(LifecycleError::Unsettled),
        error => Err(windows::core::Error::from_hresult(error.to_hresult()).into()),
    }
}
fn exact_component_key_path(actual: &str, component: &str) -> bool {
    // Microsoft documents numeric HKCU root 01 for a 32-bit registry keypath,
    // and root+20 for a 64-bit registry keypath. Approved components have exact
    // Attributes=4 (registry keypath), no 64-bit bit, and their keypath is this
    // value (no trailing separator), not a registry subkey.
    actual.eq_ignore_ascii_case(&format!(r"01:\{MARKERS}\{component}"))
}
fn absent(path: &Path) -> Result<()> {
    valid_path(path)?;
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(LifecycleError::Unsettled),
    }
}
fn marker_absent() -> Result<()> {
    let mut owner = Resources::empty()?;
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
        owner.release()?;
        return Ok(());
    }
    if result == ERROR_SUCCESS {
        owner.add(HANDLE(key.0), Role::Registry);
        owner.release()?;
    }
    Err(LifecycleError::Unsettled)
}
fn fresh(session: &mut Session) -> Result<()> {
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

pub(super) fn admit(session: &mut Session) -> Result<Admission> {
    current(session.deadline)?;
    let admission = if product(&session.manifest, "VersionString")?.is_some() {
        Admission::SameManifest
    } else {
        Admission::Fresh
    };
    revalidate(session, admission)?;
    Ok(admission)
}
pub(super) fn revalidate(session: &mut Session, admission: Admission) -> Result<()> {
    current(session.deadline)?;
    run_absent_at(RUN_KEY)?;
    match admission {
        Admission::Fresh => fresh(session)?,
        Admission::SameManifest => {
            let mut proof = present(session)?;
            // These preimage pins must return before Begin ACK: the original
            // installer needs write/delete access for both reapply and remove.
            proof.release()?;
        }
    }
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

fn manifest_path(base: &Path, relative: &str) -> Result<PathBuf> {
    // The closed manifest uses '/' on the wire. Push only its validated single
    // components so the native DOS path never retains a wire separator. The
    // fixed base and the external/native path policy remain unchanged.
    if !manifest::path(relative) {
        return Err(LifecycleError::Invalid);
    }
    let mut path = base.to_owned();
    for component in relative.split('/') {
        path.push(component);
    }
    Ok(path)
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
        let path = manifest_path(base, &entry.path)?;
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
    let mut expected: BTreeSet<_> = manifest
        .entries
        .iter()
        .map(|e| manifest_path(base, &e.path))
        .collect::<Result<_>>()?;
    expected.extend(dirs.iter().filter(|d| d.starts_with(base)).cloned());
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
            &manifest_path(base, &entry.path)?,
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
        let actual =
            String::from_utf16(&path[..n as usize]).map_err(|_| LifecycleError::Invalid)?;
        if !exact_component_key_path(&actual, &entry.component) {
            return Err(LifecycleError::Unsettled);
        }
    }
    Ok(())
}
fn present(session: &Session) -> Result<Proof> {
    let mut proof = Proof::new(session.deadline);
    let base = root()?;
    same_registration(&session.manifest, &session.resources.sid)?;
    files(&mut proof, &base, &session.manifest, &session.resources.sid)?;
    markers(&mut proof, &session.manifest)?;
    proof.check()?;
    Ok(proof)
}
pub(super) fn staged(session: &mut Session, admission: Admission) -> Result<Decision> {
    current(session.deadline)?;
    run_absent_at(RUN_KEY)?;
    let mut proof = Proof::new(session.deadline);
    let base = root()?;
    let observed = files(&mut proof, &base, &session.manifest, &session.resources.sid)
        .and_then(|()| markers(&mut proof, &session.manifest));
    // These pins must ACTUALLY close before End: a rollback needs DELETE access.
    // Cleanup failure is fatal, never a reason to authorize End or replacement.
    proof.release()?;
    current(session.deadline)?;
    let observed = if observed.is_ok() {
        Observation::Present
    } else if absent(&base).and_then(|()| marker_absent()).is_ok() {
        Observation::Absent
    } else {
        Observation::Other
    };
    current(session.deadline)?;
    Ok(Decision::observed(admission, observed))
}
pub(super) fn reconcile(session: &mut Session, decision: Decision) -> Result<Proof> {
    current(session.deadline)?;
    run_absent_at(RUN_KEY)?;
    if decision.expected == ExpectedState::Present {
        return present(session);
    }
    let proof = Proof::new(session.deadline);
    let base = root()?;
    no_product(&session.manifest)?;
    absent(&base)?;
    marker_absent()?;
    proof.check()?;
    Ok(proof)
}

#[cfg(test)]
mod tests {
    use super::super::super::super::broker::manifest::Entry;
    use super::*;
    use std::{fs, os::windows::io::AsRawHandle};

    #[test]
    fn maintenance_decision_preserves_original_preimage_and_pending_tags() {
        for (operation, tag) in [
            (MaintenanceOperation::Install, 1),
            (MaintenanceOperation::Repair, 2),
            (MaintenanceOperation::Upgrade, 3),
            (MaintenanceOperation::Uninstall, 4),
            (MaintenanceOperation::SameManifest, 5),
        ] {
            assert_eq!(operation.tag(), tag);
        }
        assert_eq!(Admission::Fresh.operation(), MaintenanceOperation::Install);
        assert_eq!(
            Admission::SameManifest.operation(),
            MaintenanceOperation::SameManifest
        );
        for (admission, observation, outcome, expected) in [
            (
                Admission::Fresh,
                Observation::Present,
                SettledOutcome::Commit,
                ExpectedState::Present,
            ),
            (
                Admission::Fresh,
                Observation::Absent,
                SettledOutcome::Rollback,
                ExpectedState::Absent,
            ),
            (
                Admission::Fresh,
                Observation::Other,
                SettledOutcome::Rollback,
                ExpectedState::Absent,
            ),
            (
                Admission::SameManifest,
                Observation::Present,
                SettledOutcome::Commit,
                ExpectedState::Present,
            ),
            (
                Admission::SameManifest,
                Observation::Absent,
                SettledOutcome::Commit,
                ExpectedState::Absent,
            ),
            (
                Admission::SameManifest,
                Observation::Other,
                SettledOutcome::Rollback,
                ExpectedState::Present,
            ),
        ] {
            let decision = Decision::observed(admission, observation);
            assert_eq!(decision.outcome(), outcome);
            assert_eq!(decision.expected, expected);
        }
    }

    #[test]
    fn registered_product_and_user_sid_must_match_exact_native_identity() {
        let product = "12345678-1234-1234-1234-123456789ABC";
        let mut code = [0u16; 39];
        for (to, from) in code.iter_mut().zip(format!("{{{product}}}").encode_utf16()) {
            *to = from;
        }
        assert!(exact_product_code(&code, product));
        assert!(!exact_product_code(
            &code,
            "12345678-1234-1234-1234-123456789ABD"
        ));
        code[38] = 1;
        assert!(!exact_product_code(&code, product));
        // Independent binary SID fixture S-1-5-21-42, as TOKEN_USER copies it.
        let sid = [1, 2, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 42, 0, 0, 0];
        assert!(exact_user_sid("S-1-5-21-42", &sid));
        for other in [
            "S-1-5-21-43",
            "S-1-5-21",
            "S-1-5-21-42-0",
            "S-1-5-21-4294967296",
            "S-1-5-21-+42",
            "S-2-5-21-42",
            "S-1-6-21-42",
            "S-1-281474976710661-21-42",
        ] {
            assert!(!exact_user_sid(other, &sid), "{other}");
        }
        assert!(!exact_user_sid("S-1-5-21-42", &sid[..15]));
    }

    struct RegistryFixture {
        owner: Resources,
        path: Option<String>,
        key: HKEY,
    }
    impl RegistryFixture {
        fn new() -> Self {
            use fc_ports::RandomSource;
            let mut nonce = [0; 16];
            crate::SystemRandom.fill(&mut nonce).unwrap();
            let tail: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
            let path = format!(
                r"Software\FutureClassroom-Test-InstallerRunGuard-{}-{tail}",
                std::process::id()
            );
            let mut fixture = Self {
                owner: Resources::empty().unwrap(),
                path: None,
                key: HKEY::default(),
            };
            let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
            unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(&path),
                    None,
                    None,
                    REG_OPTION_NON_VOLATILE,
                    KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_32KEY,
                    None,
                    &mut fixture.key,
                    Some(&mut disposition),
                )
                .ok()
                .unwrap();
            }
            fixture.owner.add(HANDLE(fixture.key.0), Role::Registry);
            assert_eq!(
                disposition, REG_CREATED_NEW_KEY,
                "never use/delete a preexisting fixture key"
            );
            fixture.path = Some(path);
            fixture
        }
        fn read(&self) -> (REG_VALUE_TYPE, Vec<u8>) {
            let mut kind = REG_VALUE_TYPE::default();
            let mut bytes = [0u8; 1024];
            let mut n = bytes.len() as u32;
            unsafe {
                RegQueryValueExW(
                    self.key,
                    windows::core::w!("FutureClassroom"),
                    None,
                    Some(&mut kind),
                    Some(bytes.as_mut_ptr()),
                    Some(&mut n),
                )
                .ok()
                .unwrap();
            }
            assert!(n as usize <= bytes.len());
            (kind, bytes[..n as usize].to_vec())
        }
    }
    impl Drop for RegistryFixture {
        fn drop(&mut self) {
            self.owner
                .release()
                .expect("return owned test registry capability");
            if let Some(path) = &self.path {
                assert!(path.starts_with(r"Software\FutureClassroom-Test-InstallerRunGuard-"));
                unsafe {
                    RegDeleteKeyExW(
                        HKEY_CURRENT_USER,
                        &HSTRING::from(path),
                        KEY_WOW64_32KEY.0,
                        None,
                    )
                    .ok()
                    .expect("delete only newly created owned test registry key");
                }
            }
        }
    }

    #[test]
    fn actual_run_absence_guard_never_changes_any_present_value() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let fixture = RegistryFixture::new();
        let path = fixture.path.as_ref().unwrap().clone();
        assert!(run_absent_at(&path).is_ok());
        let plausible: Vec<u8> = format!(
            "\"{}\" --autostart\0",
            root().unwrap().join("fc-bootstrap.exe").display()
        )
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
        for (kind, bytes) in [
            (REG_SZ, plausible),
            (REG_SZ, Vec::new()),
            (REG_EXPAND_SZ, vec![b'X', 0, 0, 0]),
            (REG_DWORD, vec![1, 0, 0, 0]),
            (REG_BINARY, vec![0x7f; 256]),
            (REG_NONE, vec![0]),
        ] {
            unsafe {
                RegSetValueExW(
                    fixture.key,
                    windows::core::w!("FutureClassroom"),
                    None,
                    kind,
                    Some(&bytes),
                )
                .ok()
                .unwrap();
            }
            let before = fixture.read();
            assert_eq!(run_absent_at(&path), Err(LifecycleError::Unsettled));
            assert_eq!(
                fixture.read(),
                before,
                "guard is read-only even for plausible owned commands"
            );
            unsafe {
                RegDeleteValueW(fixture.key, windows::core::w!("FutureClassroom"))
                    .ok()
                    .unwrap();
            }
            assert!(run_absent_at(&path).is_ok());
        }
        drop(fixture);
        assert!(run_absent_at(&path).is_ok());
    }

    #[test]
    fn enum_absence_and_x86_registry_component_paths_are_exact() {
        for result in [ERROR_UNKNOWN_PRODUCT.0, ERROR_NO_MORE_ITEMS.0] {
            assert!(known_product_absence(result));
        }
        for result in [
            ERROR_SUCCESS.0,
            ERROR_ACCESS_DENIED.0,
            ERROR_INVALID_PARAMETER.0,
            ERROR_BAD_CONFIGURATION.0,
        ] {
            assert!(!known_product_absence(result));
        }
        let component = "12345678-1234-1234-1234-123456789ABC";
        assert!(exact_component_key_path(
            &format!(r"01:\{MARKERS}\{component}"),
            component
        ));
        for actual in [
            format!(r"21:\{MARKERS}\{component}"),
            format!(r"02:\{MARKERS}\{component}"),
            format!(r"01:\{MARKERS}\{component}\"),
            format!(r"01:\{MARKERS}\other"),
        ] {
            assert!(!exact_component_key_path(&actual, component));
        }
    }

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
        // Reproduce the previous internal guard conflict without relaxing that
        // guard: Windows join preserves the wire text's embedded '/'.
        let wire_path = base.join("web/assets/main.js");
        assert!(wire_path.to_str().unwrap().contains('/'));
        let invalid = windows::core::HRESULT(0x80070057u32 as i32);
        assert_eq!(valid_path(&wire_path).unwrap_err().code(), invalid);
        assert_eq!(native_path(&wire_path).unwrap_err().code(), invalid);
        for entry in &manifest.entries {
            let path = manifest_path(&base, &entry.path).unwrap();
            assert!(!path.to_str().unwrap().contains('/'));
            valid_path(&path).expect("closed manifest component path passes native guard");
            native_path(&path).expect("closed manifest component path admits extended DOS path");
        }
        for invalid in [
            "../outside",
            "web/../outside",
            "C:\\outside",
            "web\\index.html",
        ] {
            assert!(manifest_path(&base, invalid).is_err());
        }
        let mut proof = Proof::new(Instant::now() + Duration::from_secs(10));
        files(&mut proof, &base, &manifest, &sid)
            .expect("actual file proof after closed manifest path preflight");
        proof.check().unwrap();
        assert!(fs::OpenOptions::new()
            .write(true)
            .open(base.join("fc-bootstrap.exe"))
            .is_err());
        assert!(fs::rename(&base, base.with_file_name("unexpected-move")).is_err());
        proof.release().unwrap();
        // The exact preimage pins have really returned, allowing the installer
        // to rename/remove or replace files after Begin ACK.
        let moved = base.with_file_name("returned-pins-move");
        fs::rename(&base, &moved).unwrap();
        fs::rename(&moved, &base).unwrap();
        let main = base.join("fc-bootstrap.exe");
        let original = fs::read(&main).unwrap();
        fs::write(&main, &original).unwrap();
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
