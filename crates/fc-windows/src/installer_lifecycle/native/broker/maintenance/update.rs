use super::*;
use crate::installer_lifecycle::{
    broker::{payload_manifest, published_host::predecessor},
    update_observation::UpdateSourceFacts,
};
use fc_domain::updates::ExactPredecessor;
use sha2::{Digest, Sha256};

fn with_reader<T>(exclusive: bool, operation: impl FnOnce(&mut Reader) -> Result<T>) -> Result<T> {
    let identity = InstallIdentity::for_current_user()?;
    let resources = Resources::open(&identity, exclusive)?;
    let mut reader = Box::new(Reader {
        resources,
        deadline: Instant::now() + Duration::from_secs(30),
        msi: Vec::with_capacity(8),
        closed: Vec::with_capacity(RECEIPTS),
        closed_total: 0,
    });
    let result = catch_unwind(AssertUnwindSafe(|| operation(&mut reader)))
        .unwrap_or(Err(LifecycleError::Faulted));
    let closed = if reader.resources.failed {
        Err(LifecycleError::Faulted)
    } else {
        reader
            .close_all_msi()
            .and_then(|()| reader.resources.release())
    };
    if let Err(error) = closed {
        reader.resources.retain_until_exit();
        std::mem::forget(reader);
        return Err(error);
    }
    result
}

fn uuid(value: &str) -> Result<[u8; 16]> {
    if !manifest::uuid(value) {
        return Err(LifecycleError::Invalid);
    }
    let hex: String = value.chars().filter(|v| *v != '-').collect();
    let mut result = [0; 16];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| LifecycleError::Invalid)?;
    }
    Ok(result)
}

fn predecessor_facts(record: &predecessor::PublishedPredecessor) -> Result<ExactPredecessor> {
    let payload = record.payload()?;
    Ok(ExactPredecessor {
        product_code: uuid(&payload.payload().product)?,
        package_code: uuid(payload.package())?,
        source_commit: record.source_commit,
        source_manifest_sha256: record.source_manifest_sha,
        producer_sha256: record.producer_sha,
        // This historical field is the published original source MSI hash,
        // not an assertion that Windows Installer's cached MSI is byte-identical.
        cached_msi_sha256: record.source_msi_sha,
        payload_envelope_sha256: Sha256::digest(record.envelope()).into(),
        template_sha256: record.template_sha,
        guard_sha256: record.guard_sha,
        logical_sha256: record.logical_sha,
        template_bytes: record.template_bytes,
        installer_abi: 2,
        guard_capability: record.guard_protocol,
    })
}

impl Reader {
    fn update_blob(&mut self, db: MSIHANDLE, query: &'static str, limit: usize) -> Result<Vec<u8>> {
        let view = self.view(db, query)?;
        let row = self.fetch(view)?.ok_or(LifecycleError::Invalid)?;
        let size = unsafe { MsiRecordDataSize(row, 1) } as usize;
        if size == 0 || size > limit {
            return Err(LifecycleError::Invalid);
        }
        let mut bytes = vec![0; size];
        let mut got = size as u32;
        let rc = unsafe { MsiRecordReadStream(row, 1, Some(PSTR(bytes.as_mut_ptr())), &mut got) };
        if rc != 0 || got as usize != size || self.fetch(view)?.is_some() {
            return Err(LifecycleError::Invalid);
        }
        self.close_msi(row)?;
        self.close_msi(view)?;
        Ok(bytes)
    }
}

pub(in crate::installer_lifecycle) fn inspect(directory: &Path) -> Result<UpdateSourceFacts> {
    with_reader(false, |reader| {
        let source = directory.join("FutureClassroom.msi");
        reader.pin(&source, true)?;
        let mut db = MSIHANDLE(0);
        let rc = unsafe { MsiOpenDatabaseW(&native_path(&source)?, MSIDBOPEN_READONLY, &mut db) };
        let db = reader.acquired(rc, db)?;
        let envelope =
            reader.update_blob(db, payload_manifest::QUERY, payload_manifest::MAX_BYTES)?;
        let wire = reader.update_blob(db, predecessor::QUERY, predecessor::MAX_BYTES)?;
        let payload = payload_manifest::parse(&envelope)?;
        let predecessor = predecessor::parse(&wire)?;
        let facts = predecessor_facts(&predecessor)?;
        let template = reader.binary_hash(
            db,
            "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcGuardClientTemplate'",
            None,
        )?;
        let guard = reader.binary_hash(
            db,
            "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcControlledGuardDll'",
            None,
        )?;
        reader.close_msi(db)?;
        reader.binary_facts(&source, payload.package(), &envelope, template, guard, true)?;
        reader.database(&source, payload.package())?;
        let exe = payload
            .payload()
            .entries
            .iter()
            .find(|v| v.path == "fc-bootstrap.exe")
            .ok_or(LifecycleError::Invalid)?;
        Ok(UpdateSourceFacts {
            product_code: payload.payload().product.clone(),
            version: payload.payload().version.clone(),
            installed_exe_sha256: exe.sha256,
            predecessor_wire: wire,
            predecessor: facts,
        })
    })
}

pub(in crate::installer_lifecycle) fn qualify(
    wire: &[u8],
    expected: &ExactPredecessor,
) -> Result<()> {
    let record = predecessor::parse(wire)?;
    if &predecessor_facts(&record)? != expected {
        return Err(LifecycleError::Invalid);
    }
    with_reader(false, |reader| verify_predecessor(reader, &record))
}

fn verify_predecessor(
    reader: &mut Reader,
    record: &predecessor::PublishedPredecessor,
) -> Result<()> {
    let payload = record.payload()?;
    let old = payload.payload();
    let registration = registration::capture(&reader.resources, old, reader.deadline)?
        .ok_or(LifecycleError::Invalid)?;
    let cache = reader.pin(&registration.cache, true)?;
    let cache_sha = reader.resources.digest(cache, LIMIT, reader.deadline)?;
    reader.binary_facts(
        &registration.cache,
        payload.package(),
        record.envelope(),
        record.template_sha,
        record.guard_sha,
        false,
    )?;
    if reader.database(&registration.cache, payload.package())? != record.logical_sha {
        return Err(LifecycleError::Invalid);
    }
    let install = crate::system::DataPaths::installed_bootstrap_for_current_user()?
        .parent()
        .ok_or(LifecycleError::Invalid)?
        .to_owned();
    for entry in &old.entries {
        let baseline = reader.resources.handles.len();
        let path = effects::manifest_path(&install, &entry.path)?;
        let file = reader.pin(&path, true)?;
        if reader.size(file)? != u64::from(entry.bytes)
            || reader.resources.digest(file, LIMIT, reader.deadline)? != entry.sha256
        {
            return Err(LifecycleError::Invalid);
        }
        reader.close_temporary(baseline)?;
    }
    if reader.resources.digest(cache, LIMIT, reader.deadline)? != cache_sha
        || registration::capture(&reader.resources, old, reader.deadline)?.as_ref()
            != Some(&registration)
    {
        return Err(LifecycleError::Unsettled);
    }
    Ok(())
}

// The intent remains exclusively locked by the same-user caller throughout.
// This scope additionally holds the real exclusive installer/startup lease.
pub(in crate::installer_lifecycle) fn reconcile_failed(
    wire: &[u8],
    expected: &ExactPredecessor,
    target: &str,
    setup: &Path,
    coordinator: &Path,
    commit_failed: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let record = predecessor::parse(wire)?;
    if &predecessor_facts(&record)? != expected
        || !manifest::uuid(target)
        || record.payload()?.payload().product == target
    {
        return Err(LifecycleError::Invalid);
    }
    with_reader(true, |reader| {
        reader.no_bound_processes(setup, coordinator)?;
        verify_predecessor(reader, &record)?;
        let mut value = [0u16; 16];
        let mut length = value.len() as u32;
        let rc = unsafe {
            MsiGetProductInfoExW(
                &HSTRING::from(format!("{{{target}}}")),
                None,
                MSIINSTALLCONTEXT_USERUNMANAGED,
                &HSTRING::from("State"),
                Some(PWSTR(value.as_mut_ptr())),
                Some(&mut length),
            )
        };
        if rc != ERROR_UNKNOWN_PRODUCT.0 {
            return Err(LifecycleError::Unsettled);
        }
        reader.no_bound_processes(setup, coordinator)?;
        reader.resources.absent_pending()?;
        reader.time()?;
        commit_failed()
    })
}

impl Reader {
    fn no_bound_processes(&mut self, setup: &Path, coordinator: &Path) -> Result<()> {
        let names = [setup, coordinator].map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .ok_or(LifecycleError::Invalid)
        });
        let names = [names[0]?, names[1]?];
        self.room(1)?;
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? };
        self.resources.add(snapshot, Role::Control);
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut next = unsafe { Process32FirstW(snapshot, &mut entry) };
        for _ in 0..8192 {
            self.time()?;
            if let Err(error) = next {
                if error.code() != ERROR_NO_MORE_FILES.to_hresult() {
                    return Err(error.into());
                }
                return self.resources.close_handle(snapshot);
            }
            let end = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .filter(|length| *length != 0)
                .ok_or(LifecycleError::Unsettled)?;
            let name = String::from_utf16(&entry.szExeFile[..end])
                .map_err(|_| LifecycleError::Unsettled)?;
            if names.iter().any(|bound| name.eq_ignore_ascii_case(bound)) {
                self.room(1)?;
                let process = self.resources.process(entry.th32ProcessID, Role::Control)?;
                let image = image_path(process)?;
                if [setup, coordinator].iter().any(|bound| {
                    image
                        .as_os_str()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&bound.as_os_str().to_string_lossy())
                }) {
                    match unsafe { WaitForSingleObject(process, 0) } {
                        WAIT_OBJECT_0 => (),
                        WAIT_TIMEOUT => return Err(LifecycleError::GuardianRunning),
                        _ => return Err(LifecycleError::Unsettled),
                    }
                }
                self.resources.close_handle(process)?;
            }
            next = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        Err(LifecycleError::Invalid)
    }
}

/// Explicit installer repair of an unregistered orphan. Only paths declared
/// by the verified current/previous source are removed; teaching data is outside
/// this fixed installation directory. Pending/live owners still reject entry.
pub(in crate::installer_lifecycle) fn remove_orphaned(directory: &Path) -> Result<()> {
    with_reader(true, |reader| {
        let mut related = [0u16; 39];
        let family = HSTRING::from(format!("{{{INSTALL_FAMILY}}}"));
        if unsafe { MsiEnumRelatedProductsW(&family, None, 0, PWSTR(related.as_mut_ptr())) }
            != ERROR_NO_MORE_ITEMS.0
        {
            return Err(LifecycleError::Invalid);
        }
        let root = crate::system::DataPaths::installed_bootstrap_for_current_user()?
            .parent()
            .ok_or(LifecycleError::Invalid)?
            .to_owned();
        if !root.exists() {
            return Ok(());
        }
        let mut allowed = BTreeSet::new();
        for source in [
            directory.join("FutureClassroom.msi"),
            directory.join("previous/FutureClassroom.msi"),
            directory.join("legacy/FutureClassroom.msi"),
        ] {
            if !source.exists() {
                continue;
            }
            reader.pin(&source, true)?;
            let mut db = MSIHANDLE(0);
            let rc =
                unsafe { MsiOpenDatabaseW(&native_path(&source)?, MSIDBOPEN_READONLY, &mut db) };
            let db = reader.acquired(rc, db)?;
            let wire =
                reader.update_blob(db, payload_manifest::QUERY, payload_manifest::MAX_BYTES)?;
            let payload = payload_manifest::parse(&wire)?;
            for entry in &payload.payload().entries {
                allowed.insert(entry.path.to_ascii_lowercase());
            }
            reader.close_msi(db)?;
        }
        let h = reader.resources.open_file(
            &root,
            READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            true,
            false,
        )?;
        verify_info(h, true)?;
        verify_acl(h, &reader.resources.sid, true)?;
        reader.resources.add(h, Role::Parent);
        let files = crate::materials::files::ControlledFiles::new(&root)?;
        let mut pending = vec![root.clone()];
        let mut directories = Vec::new();
        let mut leaves = Vec::new();
        while let Some(path) = pending.pop() {
            for child in files.entries(&path, 32768)? {
                let relative = child
                    .strip_prefix(&root)
                    .map_err(|_| LifecycleError::Invalid)?
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_ascii_lowercase();
                if child.is_dir() {
                    if !allowed
                        .iter()
                        .any(|n| n.starts_with(&(relative.clone() + "/")))
                    {
                        return Err(LifecycleError::Invalid);
                    }
                    directories.push(child.clone());
                    pending.push(child);
                } else {
                    if !allowed.contains(&relative) {
                        return Err(LifecycleError::Invalid);
                    }
                    leaves.push(child);
                }
            }
        }
        for path in leaves {
            files.remove(&path)?;
        }
        directories.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for path in directories {
            files.remove_empty_directory(&path)?;
        }
        reader.resources.close_handle(h)?;
        crate::materials::files::ControlledFiles::new(
            root.parent().ok_or(LifecycleError::Invalid)?,
        )?
        .remove_empty_directory(&root)?;
        Ok(())
    })
}
