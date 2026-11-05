//! Additional capture in the SAME Reader/Resources/claim; never a second owner.
use super::*;
use crate::installer_lifecycle::broker::published_host::predecessor;
const TEMPLATE: &str = "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcGuardClientTemplate'";
const GUARD: &str = "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcControlledGuardDll'";
const PAYLOAD: &str = "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcPayloadManifest'";
impl Reader {
    pub(super) fn capture_upgrade(
        &mut self,
        source: &source_package::CapturedSource,
    ) -> Result<Option<upgrade::VerifiedUpgradePlan>> {
        let mut found = [0u16; 39];
        self.time()?;
        let rc = unsafe {
            MsiEnumRelatedProductsW(
                &HSTRING::from(format!("{{{INSTALL_FAMILY}}}")),
                None,
                0,
                PWSTR(found.as_mut_ptr()),
            )
        };
        self.time()?;
        if rc == ERROR_NO_MORE_ITEMS.0 {
            return Ok(None);
        }
        if rc != 0 {
            return Err(LifecycleError::Invalid);
        }
        let related = format::product_code(
            &String::from_utf16(&found[..38]).map_err(|_| LifecycleError::Invalid)?,
        )
        .ok_or(LifecycleError::Invalid)?;
        if found[38] != 0 {
            return Err(LifecycleError::Invalid);
        }
        if related == source.manifest.product {
            return Ok(None);
        }
        // A source without a qualified capability3 predecessor may install
        // afresh or maintain itself, but cannot enter an older endpoint. This
        // is observed before Begin and every business write.
        let record = source
            .predecessor
            .as_ref()
            .ok_or(LifecycleError::UpgradeProtocolUnsupported)?;
        let published = record.payload()?;
        let old = published.payload();
        let new_version =
            format::version(&source.manifest.version).ok_or(LifecycleError::Invalid)?;
        let old_version = format::version(&old.version).ok_or(LifecycleError::Invalid)?;
        if old.product != related
            || old.product == source.manifest.product
            || old_version >= new_version
            || published.package() == source.package
        {
            return Err(LifecycleError::Invalid);
        }
        let union = upgrade::union(old, &source.manifest)?;
        let registration = registration::capture(&self.resources, old, self.deadline)?
            .ok_or(LifecycleError::Invalid)?;
        let source_path = image_path(unsafe { GetCurrentProcess() })?
            .parent()
            .ok_or(LifecycleError::Invalid)?
            .join("FutureClassroom.msi");
        let file = self.pin(&source_path, false)?;
        let object = verify_info(file, false)?;
        let size = self.size(file)?;
        self.room(2)?;
        if self.resources.digest(file, LIMIT, self.deadline)? != source.source_sha256 {
            return Err(LifecycleError::Invalid);
        }
        self.binary_facts(
            &source_path,
            &source.package,
            &source.envelope,
            source.template_sha256,
            source.guard_sha256,
            true,
        )?;
        // This also rejects every unknown/missing table/schema in the new MSI.
        self.database(&source_path, &source.package)?;
        if !self.msi.is_empty() {
            return Err(LifecycleError::Unsettled);
        }
        self.closed.clear();
        let cache = self.pin(&registration.cache, true)?;
        let cache_object = verify_info(cache, false)?;
        let cache_bytes = self.size(cache)?;
        self.room(2)?;
        let cache_sha = self.resources.digest(cache, LIMIT, self.deadline)?;
        self.binary_facts(
            &registration.cache,
            published.package(),
            record.envelope(),
            record.template_sha,
            record.guard_sha,
            false,
        )?;
        let logical = self.database(&registration.cache, published.package())?;
        if logical != record.logical_sha {
            return Err(LifecycleError::Invalid);
        }
        self.room(2)?;
        if verify_info(cache, false)? != cache_object
            || self.size(cache)? != cache_bytes
            || self.resources.digest(cache, LIMIT, self.deadline)? != cache_sha
        {
            return Err(LifecycleError::Unsettled);
        }
        self.room(2)?;
        if verify_info(file, false)? != object
            || self.size(file)? != size
            || self.resources.digest(file, LIMIT, self.deadline)? != source.source_sha256
        {
            return Err(LifecycleError::Unsettled);
        }
        if registration::capture(&self.resources, old, self.deadline)?.as_ref()
            != Some(&registration)
        {
            return Err(LifecycleError::Unsettled);
        }
        Ok(Some(upgrade::VerifiedUpgradePlan {
            old: upgrade::clone_manifest(old),
            current_auxiliary: source.auxiliary()?,
            predecessor_auxiliary: record.auxiliary()?,
            union,
            registration,
            cache_object,
            cache_bytes,
            cache_sha,
            source_sha: source.source_sha256,
            old_package: published.package().into(),
            checked_msi_closes: self.closed_total,
        }))
    }
    pub(super) fn binary_hash(
        &mut self,
        db: MSIHANDLE,
        query: &'static str,
        exact: Option<&[u8]>,
    ) -> Result<[u8; 32]> {
        let view = self.view(db, query)?;
        let row = self.fetch(view)?.ok_or(LifecycleError::Invalid)?;
        let size = unsafe { MsiRecordDataSize(row, 1) } as usize;
        if size == 0 || size as u64 > LIMIT || exact.is_some_and(|v| v.len() != size) {
            return Err(LifecycleError::Invalid);
        }
        if query == TEMPLATE && size < crate::installer_lifecycle::broker::carrier::MIN_IMAGE_BYTES
        {
            return Err(LifecycleError::Invalid);
        }
        let mut tail = [0u8; 132];
        let hash = self.hash_new()?;
        let mut offset = 0;
        let mut bytes = [0; 16384];
        while offset < size {
            let n = (size - offset).min(bytes.len());
            let mut got = n as u32;
            let rc =
                unsafe { MsiRecordReadStream(row, 1, Some(PSTR(bytes.as_mut_ptr())), &mut got) };
            if rc != 0
                || got as usize != n
                || exact.is_some_and(|v| v[offset..offset + n] != bytes[..n])
            {
                return Err(LifecycleError::Invalid);
            }
            if query == TEMPLATE {
                let start = size.saturating_sub(tail.len()).max(offset);
                if start < offset + n {
                    let destination = start - size.saturating_sub(tail.len());
                    tail[destination..destination + offset + n - start]
                        .copy_from_slice(&bytes[start - offset..n]);
                }
            }
            self.hash_add(&hash, &bytes[..n])?;
            offset += n;
        }
        let mut end = 1;
        let rc = unsafe { MsiRecordReadStream(row, 1, Some(PSTR(bytes.as_mut_ptr())), &mut end) };
        if rc != 0 || end != 0 || self.fetch(view)?.is_some() {
            return Err(LifecycleError::Invalid);
        }
        if query == TEMPLATE {
            use crate::installer_lifecycle::broker::carrier;
            if &tail[..16] != carrier::SEAL_MAGIC
                || tail[16..48] != [0; 32]
                || tail[112..116] != 64u32.to_le_bytes()
                || &tail[116..] != carrier::FOOTER
                || tail[48..108] != carrier::descriptor()[..60]
            {
                return Err(LifecycleError::Invalid);
            }
            if tail[108..112] != u32::from(carrier::GUARD_PROTOCOL).to_le_bytes() {
                return Err(LifecycleError::UpgradeProtocolUnsupported);
            }
        }
        self.close_msi(row)?;
        self.close_msi(view)?;
        self.hash_end(hash)
    }
    pub(super) fn binary_facts(
        &mut self,
        path: &Path,
        package: &str,
        envelope: &[u8],
        template: [u8; 32],
        guard: [u8; 32],
        new: bool,
    ) -> Result<()> {
        self.room(1)?;
        let mut db = MSIHANDLE(0);
        let rc = unsafe { MsiOpenDatabaseW(&native_path(path)?, MSIDBOPEN_READONLY, &mut db) };
        let db = self.acquired(rc, db)?;
        self.package(db, package)?;
        // Closed old3/old4 -> new4 grammar. Every present fourth stream is
        // bounded wire data, and remains in database()'s whole fixed31 digest.
        // Parsing does not authorize or recursively inspect that older release.
        let binary_view = self.view(db, "SELECT `Name` FROM `Binary`")?;
        let mut binary_names = BTreeSet::new();
        while let Some(row) = self.fetch(binary_view)? {
            let name = self.text(row, 1)?;
            if !binary_names.insert(name) || binary_names.len() > 4 {
                return Err(LifecycleError::Invalid);
            }
            self.close_msi(row)?;
        }
        self.close_msi(binary_view)?;
        if !upgrade::binary_names(new, &binary_names) {
            return Err(LifecycleError::Invalid);
        }
        if binary_names.contains("FcPublishedPredecessor") {
            self.predecessor_wire(db)?;
        }
        let expected = crate::installer_lifecycle::broker::payload_manifest::parse(envelope)?;
        let view=self.view(db,"SELECT `Property`, `Value` FROM `Property` WHERE `Property` = 'ProductCode' OR `Property` = 'UpgradeCode' OR `Property` = 'ProductVersion'")?;
        let mut seen = BTreeSet::new();
        for _ in 0..3 {
            let row = self.fetch(view)?.ok_or(LifecycleError::Invalid)?;
            let name = self.text(row, 1)?;
            let value = self.text(row, 2)?;
            let right = match name.as_str() {
                "ProductCode" => value == format!("{{{}}}", expected.payload().product),
                "UpgradeCode" => value == format!("{{{INSTALL_FAMILY}}}"),
                "ProductVersion" => value == expected.payload().version,
                _ => false,
            };
            if !right || !seen.insert(name) {
                return Err(LifecycleError::Invalid);
            }
            self.close_msi(row)?;
        }
        if self.fetch(view)?.is_some() {
            return Err(LifecycleError::Invalid);
        }
        self.close_msi(view)?;
        self.binary_hash(db, PAYLOAD, Some(envelope))?;
        if self.binary_hash(db, TEMPLATE, None)? != template
            || self.binary_hash(db, GUARD, None)? != guard
        {
            return Err(LifecycleError::Invalid);
        }
        let view=self.view(db,"SELECT `Action`, `Type`, `Source`, `Target` FROM `CustomAction` WHERE `Action` = 'FcControlledInnerGuard'")?;
        let row = self.fetch(view)?.ok_or(LifecycleError::Invalid)?;
        if self.text(row, 1)? != "FcControlledInnerGuard"
            || unsafe { MsiRecordGetInteger(row, 2) } != 1
            || self.text(row, 3)? != "FcControlledGuardDll"
            || self.text(row, 4)? != "FcControlledInnerGuard"
            || self.fetch(view)?.is_some()
        {
            return Err(LifecycleError::Invalid);
        }
        self.close_msi(row)?;
        self.close_msi(view)?;
        let view=self.view(db,"SELECT `Condition`, `Sequence` FROM `InstallExecuteSequence` WHERE `Action` = 'FcControlledInnerGuard'")?;
        let row = self.fetch(view)?.ok_or(LifecycleError::Invalid)?;
        if self.text(row, 1)? != "1"
            || unsafe { MsiRecordGetInteger(row, 2) } != 1499
            || self.fetch(view)?.is_some()
        {
            return Err(LifecycleError::Invalid);
        }
        self.close_msi(row)?;
        self.close_msi(view)?;
        if new {
            let view=self.view(db,"SELECT `Sequence` FROM `InstallExecuteSequence` WHERE `Action` = 'RemoveExistingProducts'")?;
            let row = self.fetch(view)?.ok_or(LifecycleError::Invalid)?;
            if unsafe { MsiRecordGetInteger(row, 1) } != 1501 || self.fetch(view)?.is_some() {
                return Err(LifecycleError::Invalid);
            }
            self.close_msi(row)?;
            self.close_msi(view)?;
        }
        self.close_msi(db)
    }
    fn predecessor_wire(&mut self, db: MSIHANDLE) -> Result<()> {
        let view = self.view(db, predecessor::QUERY)?;
        let row = self.fetch(view)?.ok_or(LifecycleError::Invalid)?;
        self.time()?;
        let size = unsafe { MsiRecordDataSize(row, 1) } as usize;
        if !(predecessor::HEADER_BYTES + 1..=predecessor::MAX_BYTES).contains(&size) {
            return Err(LifecycleError::Invalid);
        }
        let mut bytes = vec![0; size];
        for chunk in bytes.chunks_mut(16384) {
            self.time()?;
            let mut got = chunk.len() as u32;
            let rc =
                unsafe { MsiRecordReadStream(row, 1, Some(PSTR(chunk.as_mut_ptr())), &mut got) };
            self.time()?;
            if rc != 0 || got as usize != chunk.len() {
                return Err(LifecycleError::Invalid);
            }
        }
        let mut tail = [0u8; 1];
        let mut got = 1;
        self.time()?;
        let rc = unsafe { MsiRecordReadStream(row, 1, Some(PSTR(tail.as_mut_ptr())), &mut got) };
        self.time()?;
        if rc != 0 || got != 0 || self.fetch(view)?.is_some() {
            return Err(LifecycleError::Invalid);
        }
        self.close_msi(row)?;
        self.close_msi(view)?;
        self.time()?;
        predecessor::parse(&bytes)?;
        self.time()
    }
}
