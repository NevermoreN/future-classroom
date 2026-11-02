//! Before-Begin capture of this OS image's fixed sibling source MSI.
//! Facts only: no PublishedPredecessor, role, upgrade Plan or v2 grant is made.
//! The reader takes the SAME Resources by value, never another CLAIMS slot.
use super::*;
use crate::installer_lifecycle::{
    broker::{carrier, payload_manifest},
    guard_contract::GUARD_TEMPLATE_QUERY,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
mod carrier_image;
mod database;
#[cfg(test)]
mod native_probe;
const INNER_MSI: &str = "FutureClassroom.msi";
const MAX_SOURCE: u64 = 64 * 1024 * 1024;
const CLOSE_LIMIT: usize = 32;

struct MsiOwned {
    handle: MSIHANDLE,
    view_open: bool,
    close_attempted: bool,
    view_return: Option<u32>,
    close_return: Option<u32>,
}
#[derive(Debug)]
pub(super) struct MsiCloseReceipt {
    pub(super) handle: u32,
    pub(super) view_return: Option<u32>,
    pub(super) close_return: u32,
}
/// Synchronous observations with all owned MSI objects checked closed. The
/// original source/image/ancestor pins stay in the returned Resources owner.
#[derive(Debug)]
pub(super) struct CapturedSource {
    pub(super) abi: u16,
    pub(super) package: String,
    pub(super) family: String,
    pub(super) manifest: Manifest,
    pub(super) envelope: Vec<u8>,
    pub(super) template_bytes: usize,
    pub(super) template_sha256: [u8; 32],
    pub(super) source_object: (u32, u64),
    pub(super) source_bytes: u64,
    pub(super) source_sha256: [u8; 32],
    pub(super) image_sha256: [u8; 32],
    pub(super) close_receipts: Vec<MsiCloseReceipt>,
}
impl CapturedSource {
    pub(super) fn session_manifest(&self) -> Manifest {
        Manifest {
            product: self.manifest.product.clone(),
            version: self.manifest.version.clone(),
            entries: self
                .manifest
                .entries
                .iter()
                .map(|e| manifest::Entry {
                    path: e.path.clone(),
                    bytes: e.bytes,
                    sha256: e.sha256,
                    component: e.component.clone(),
                })
                .collect(),
        }
    }
}
pub(super) struct SourceReader {
    resources: Resources,
    deadline: Instant,
    msi: Vec<MsiOwned>,
    close_receipts: Vec<MsiCloseReceipt>,
}
impl SourceReader {
    /// No path/hash/manifest arguments: the OS image and fixed MSI leaf supply
    /// the only location. `image` is Session's original pinned current image.
    pub(super) fn capture(
        resources: Resources,
        image: HANDLE,
        deadline: Instant,
    ) -> Result<(Resources, CapturedSource)> {
        let mut reader = Box::new(Self {
            resources,
            deadline,
            msi: Vec::with_capacity(8),
            close_receipts: Vec::with_capacity(CLOSE_LIMIT),
        });
        let observed = catch_unwind(AssertUnwindSafe(|| reader.read(image)))
            .unwrap_or(Err(LifecycleError::Faulted));
        let closed = if reader.resources.failed {
            Err(LifecycleError::Faulted)
        } else {
            reader.close_all_msi()
        };
        if let Err(error) = closed {
            reader.resources.retain_until_exit();
            let _original_box = Box::into_raw(reader);
            return Err(error);
        }
        match observed {
            Ok(mut source) => {
                source.close_receipts = std::mem::take(&mut reader.close_receipts);
                // No MSI handle can be lost in Session's kernel-only Drop.
                if !reader.msi.is_empty() {
                    unreachable!();
                }
                let Self { resources, .. } = *reader;
                Ok((resources, source))
            }
            Err(error) => {
                if let Err(close_error) = reader.resources.release() {
                    reader.resources.retain_until_exit();
                    let _original_box = Box::into_raw(reader);
                    return Err(close_error);
                }
                Err(error)
            }
        }
    }
    fn time(&self) -> Result<()> {
        current(self.deadline)?;
        self.resources.check_live()
    }
    fn room(&self, incoming: usize) -> Result<()> {
        self.time()?;
        if self.resources.handles.len() + self.msi.len() + incoming > HANDLE_LIMIT {
            return Err(LifecycleError::Invalid);
        }
        Ok(())
    }
    fn register_msi(&mut self, handle: MSIHANDLE, view_open: bool) -> Result<MSIHANDLE> {
        if handle.0 == 0 {
            return Err(LifecycleError::Invalid);
        }
        self.msi.push(MsiOwned {
            handle,
            view_open,
            close_attempted: false,
            view_return: None,
            close_return: None,
        });
        Ok(handle)
    }
    fn close_msi(&mut self, handle: MSIHANDLE) -> Result<()> {
        let i = self
            .msi
            .iter()
            .position(|v| v.handle == handle)
            .ok_or(LifecycleError::Invalid)?;
        let item = &mut self.msi[i];
        if item.close_attempted || self.close_receipts.len() >= CLOSE_LIMIT {
            return Err(LifecycleError::Faulted);
        }
        if item.view_open {
            let code = unsafe { MsiViewClose(handle) };
            item.view_return = Some(code);
            if code != 0 {
                item.close_attempted = true;
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
        self.close_receipts.push(MsiCloseReceipt {
            handle: handle.0,
            view_return: item.view_return,
            close_return: code,
        });
        self.msi.remove(i);
        Ok(())
    }
    fn close_all_msi(&mut self) -> Result<()> {
        while let Some(item) = self.msi.last() {
            self.close_msi(item.handle)?;
        }
        Ok(())
    }
    fn size(&self, h: HANDLE) -> Result<u64> {
        self.time()?;
        let mut size = 0;
        unsafe {
            GetFileSizeEx(h, &mut size)?;
        }
        if size <= 0 || size as u64 > MAX_SOURCE {
            return Err(LifecycleError::Invalid);
        }
        Ok(size as u64)
    }
    fn read(&mut self, image: HANDLE) -> Result<CapturedSource> {
        self.time()?;
        let image_object = verify_info(image, false)?;
        let image_size = self.size(image)? as usize;
        let hole = carrier_image::hole(image_size, |at, b| {
            self.time()?;
            self.resources.read_at(image, at as i64, b)
        })?;
        let mut seal = [0; 32];
        self.resources.read_at(image, hole as i64, &mut seal)?;
        // The fixed carrier slot is a format value until this complete, pinned
        // original MSI has actually been hashed by the existing CNG path.
        self.room(2)?;
        let image_sha = self.resources.digest(image, MAX_SOURCE, self.deadline)?;
        let path = image_path(unsafe { GetCurrentProcess() })?
            .parent()
            .ok_or(LifecycleError::Invalid)?
            .join(INNER_MSI);
        self.room(1)?;
        let file = self.resources.open_file(
            &path,
            GENERIC_READ.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        self.resources.add(file, Role::Control);
        let object = verify_info(file, false)?;
        let size = self.size(file)?;
        self.room(2)?;
        let sha = self.resources.digest(file, MAX_SOURCE, self.deadline)?;
        if sha != seal {
            return Err(LifecycleError::Invalid);
        }
        self.room(1)?;
        let mut db = MSIHANDLE(0);
        let code = unsafe { MsiOpenDatabaseW(&native_path(&path)?, MSIDBOPEN_READONLY, &mut db) };
        let db = self.abi2_acquired(code, db)?;
        let (product, family, version) = self.abi2_properties(db)?;
        let package = self.abi2_package(db)?;
        let (view, row) = self.abi2_one(db, payload_manifest::QUERY)?;
        let n = unsafe { MsiRecordDataSize(row, 1) } as usize;
        if !(payload_manifest::HEADER_BYTES..=payload_manifest::MAX_BYTES).contains(&n) {
            return Err(LifecycleError::Invalid);
        }
        let mut envelope = vec![0; n];
        self.abi2_stream_read(row, &mut envelope)?;
        self.abi2_stream_end(row)?;
        self.abi2_end(view, row)?;
        let parsed = payload_manifest::parse(&envelope)?;
        if family != INSTALL_FAMILY
            || product != parsed.payload().product
            || version != parsed.payload().version
            || package != parsed.package()
        {
            return Err(LifecycleError::Invalid);
        }
        let (template_bytes, template_sha) = self.template(db, image, image_size, hole)?;
        self.close_msi(db)?;
        // Every MSI object has returned before rehash/CNG, Session or Begin.
        if !self.msi.is_empty() {
            return Err(LifecycleError::Unsettled);
        }
        self.room(2)?;
        if verify_info(file, false)? != object
            || self.size(file)? != size
            || self.resources.digest(file, MAX_SOURCE, self.deadline)? != sha
        {
            return Err(LifecycleError::Unsettled);
        }
        self.room(2)?;
        if verify_info(image, false)? != image_object
            || self.size(image)? as usize != image_size
            || self.resources.digest(image, MAX_SOURCE, self.deadline)? != image_sha
        {
            return Err(LifecycleError::Unsettled);
        }
        // Parsing an already captured bounded envelope again moves its complete
        // payload facts into the typed result without duplicating PE/MSI bytes.
        drop(parsed);
        let manifest = manifest::parse(&envelope[payload_manifest::HEADER_BYTES..])?;
        self.time()?;
        Ok(CapturedSource {
            abi: carrier::ABI,
            package,
            family,
            manifest,
            envelope,
            template_bytes,
            template_sha256: template_sha,
            source_object: object,
            source_bytes: size,
            source_sha256: sha,
            image_sha256: image_sha,
            close_receipts: Vec::new(),
        })
    }
    fn template(
        &mut self,
        db: MSIHANDLE,
        image: HANDLE,
        length: usize,
        hole: usize,
    ) -> Result<(usize, [u8; 32])> {
        let (view, row) = self.abi2_one(db, GUARD_TEMPLATE_QUERY)?;
        if unsafe { MsiRecordDataSize(row, 1) } as usize != length {
            return Err(LifecycleError::Invalid);
        }
        self.room(2)?;
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
        let algorithm_handle = self.resources.add(HANDLE(algorithm.0), Role::Algorithm);
        let mut hash = BCRYPT_HASH_HANDLE::default();
        unsafe {
            BCryptCreateHash(algorithm, &mut hash, None, None, 0).ok()?;
        }
        let hash_handle = self.resources.add(HANDLE(hash.0), Role::Hash);
        let mut template = [0; 16384];
        let mut actual = [0; 16384];
        let mut offset = 0;
        while offset < length {
            let n = template.len().min(length - offset);
            self.abi2_stream_read(row, &mut template[..n])?;
            self.resources
                .read_at(image, offset as i64, &mut actual[..n])?;
            for i in 0..n {
                if (hole..hole + 32).contains(&(offset + i)) {
                    if template[i] != 0 {
                        return Err(LifecycleError::Invalid);
                    }
                } else if template[i] != actual[i] {
                    return Err(LifecycleError::Invalid);
                }
            }
            unsafe {
                BCryptHashData(hash, &template[..n], 0).ok()?;
            }
            offset += n;
        }
        self.abi2_stream_end(row)?;
        let mut digest = [0; 32];
        unsafe {
            BCryptFinishHash(hash, &mut digest, 0).ok()?;
        }
        let index = self
            .resources
            .handles
            .iter()
            .position(|v| v.role == Role::Hash && v.handle == hash_handle)
            .ok_or(LifecycleError::Invalid)?;
        self.resources.close(index)?;
        let index = self
            .resources
            .handles
            .iter()
            .position(|v| v.role == Role::Algorithm && v.handle == algorithm_handle)
            .ok_or(LifecycleError::Invalid)?;
        self.resources.close(index)?;
        self.abi2_end(view, row)?;
        Ok((length, digest))
    }
}
