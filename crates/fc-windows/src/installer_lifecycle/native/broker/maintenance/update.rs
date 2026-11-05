use super::*;
use crate::installer_lifecycle::{
    broker::{payload_manifest, published_host::predecessor},
    update_observation::UpdateSourceFacts,
};
use fc_domain::updates::ExactPredecessor;
use sha2::{Digest, Sha256};

fn with_reader<T>(operation: impl FnOnce(&mut Reader) -> Result<T>) -> Result<T> {
    let identity = InstallIdentity::for_current_user()?;
    let resources = Resources::open(&identity, false)?;
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
    with_reader(|reader| {
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
    with_reader(|reader| {
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
    })
}
