//! Private same-source maintenance qualification before Begin. This does not
//! publish an installed predecessor and cannot authorize cross-version Upgrade.
use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
use windows::core::{PCWSTR, PSTR, PWSTR};
mod database;
mod format;
mod registration;
mod schema;
const LIMIT: u64 = 64 * 1024 * 1024;
const RECEIPTS: usize = 256;
const MAX_ROWS: usize = 72;
const MAX_CLOSES: u32 = 8192;
struct MsiOwned {
    handle: MSIHANDLE,
    view_open: bool,
    attempted: bool,
    view_return: Option<u32>,
    close_return: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Registration {
    product: String,
    version: String,
    cache: PathBuf,
}
/// No public constructor, runtime permission field, or predecessor claim.
#[derive(Debug)]
pub(super) struct VerifiedSameSourceMaintenance {
    registration: Registration,
    cache_object: (u32, u64),
    cache_bytes: u64,
    cache_sha: [u8; 32],
    source_sha: [u8; 32],
    logical_sha: [u8; 32],
    package: String,
    checked_msi_closes: u32,
}
impl VerifiedSameSourceMaintenance {
    pub(super) fn matches(&self, s: &Session) -> bool {
        s.captured_source.as_ref().is_some_and(|v| {
            self.source_sha == v.source_sha256
                && self.package == v.package
                && self.registration.product == v.manifest.product
                && self.registration.version == v.manifest.version
                && self.logical_sha != [0; 32]
                && self.checked_msi_closes != 0
        })
    }
}
pub(super) struct Reader {
    resources: Resources,
    deadline: Instant,
    msi: Vec<MsiOwned>,
    closed: Vec<(u32, u32)>,
    closed_total: u32,
}
impl Reader {
    pub(super) fn capture(
        resources: Resources,
        deadline: Instant,
        source: &source_package::CapturedSource,
    ) -> Result<(Resources, Option<VerifiedSameSourceMaintenance>)> {
        let mut reader = Box::new(Self {
            resources,
            deadline,
            msi: Vec::with_capacity(8),
            closed: Vec::with_capacity(RECEIPTS),
            closed_total: 0,
        });
        let baseline = reader.resources.handles.len();
        let result = catch_unwind(AssertUnwindSafe(|| reader.read(source)))
            .unwrap_or(Err(LifecycleError::Faulted));
        let closed = if reader.resources.failed {
            Err(LifecycleError::Faulted)
        } else {
            reader
                .close_all_msi()
                .and_then(|()| reader.close_temporary(baseline))
        };
        if let Err(error) = closed {
            reader.resources.retain_until_exit();
            let _original = Box::into_raw(reader);
            return Err(error);
        }
        match result {
            Ok(result) => {
                let Self { resources, .. } = *reader;
                Ok((resources, result))
            }
            Err(error) => {
                if let Err(close) = reader.resources.release() {
                    reader.resources.retain_until_exit();
                    let _original = Box::into_raw(reader);
                    return Err(close);
                }
                Err(error)
            }
        }
    }
    fn time(&self) -> Result<()> {
        current(self.deadline)?;
        self.resources.check_live()
    }
    fn room(&self, n: usize) -> Result<()> {
        self.time()?;
        if self.resources.handles.len() + self.msi.len() + n > HANDLE_LIMIT
            || self.closed.len() + self.msi.len() + n > RECEIPTS
            || self.closed_total >= MAX_CLOSES
        {
            return Err(LifecycleError::Invalid);
        }
        Ok(())
    }
    fn acquired(&mut self, code: u32, handle: MSIHANDLE) -> Result<MSIHANDLE> {
        if handle.0 != 0 {
            self.msi.push(MsiOwned {
                handle,
                view_open: false,
                attempted: false,
                view_return: None,
                close_return: None,
            });
        }
        self.time()?;
        if code != 0 || handle.0 == 0 {
            return Err(LifecycleError::Invalid);
        }
        Ok(handle)
    }
    fn close_msi(&mut self, handle: MSIHANDLE) -> Result<()> {
        let index = self
            .msi
            .iter()
            .position(|v| v.handle == handle)
            .ok_or(LifecycleError::Invalid)?;
        if self.closed.len() + self.msi.len() > RECEIPTS {
            return Err(LifecycleError::Invalid);
        }
        let item = &mut self.msi[index];
        if item.attempted {
            return Err(LifecycleError::Faulted);
        }
        if item.view_open {
            let rc = unsafe { MsiViewClose(handle) };
            item.view_return = Some(rc);
            if rc != 0 {
                item.attempted = true;
                return Err(LifecycleError::Unsettled);
            }
            item.view_open = false;
        }
        item.attempted = true;
        let rc = unsafe { MsiCloseHandle(handle) };
        item.close_return = Some(rc);
        if rc != 0 {
            return Err(LifecycleError::Unsettled);
        }
        self.closed.push((handle.0, rc));
        self.closed_total += 1;
        self.msi.remove(index);
        Ok(())
    }
    fn close_all_msi(&mut self) -> Result<()> {
        while let Some(v) = self.msi.last() {
            self.close_msi(v.handle)?;
        }
        Ok(())
    }
    fn close_temporary(&mut self, baseline: usize) -> Result<()> {
        while self.resources.handles.len() > baseline {
            self.resources.close(self.resources.handles.len() - 1)?;
        }
        Ok(())
    }
    fn checkpoint(&mut self, db: MSIHANDLE) -> Result<()> {
        // Consume successful metadata only between complete tables. The one
        // original database stays live in this SAME ledger/claim. No failed or
        // attempted-close object is removed, reset, reconstructed or retried.
        if self.msi.len() != 1
            || self.msi[0].handle != db
            || self.msi[0].attempted
            || self.closed.iter().any(|&(_, rc)| rc != 0)
        {
            return Err(LifecycleError::Unsettled);
        }
        self.closed.clear();
        self.time()
    }
    fn read(
        &mut self,
        source: &source_package::CapturedSource,
    ) -> Result<Option<VerifiedSameSourceMaintenance>> {
        let Some(registration) = registration::capture(&self.resources, source, self.deadline)?
        else {
            return Ok(None);
        };
        let source_path = image_path(unsafe { GetCurrentProcess() })?
            .parent()
            .ok_or(LifecycleError::Invalid)?
            .join("FutureClassroom.msi");
        let source_file = self.pin(&source_path, false)?;
        let source_object = verify_info(source_file, false)?;
        let source_bytes = self.size(source_file)?;
        self.room(2)?;
        if self.resources.digest(source_file, LIMIT, self.deadline)? != source.source_sha256 {
            return Err(LifecycleError::Invalid);
        }
        let source_logical = self.database(&source_path, &source.package)?;
        if !self.msi.is_empty() {
            return Err(LifecycleError::Unsettled);
        }
        self.closed.clear();
        let cache = self.pin(&registration.cache, true)?;
        let object = verify_info(cache, false)?;
        let size = self.size(cache)?;
        self.room(2)?;
        let sha = self.resources.digest(cache, LIMIT, self.deadline)?;
        let cache_logical = self.database(&registration.cache, &source.package)?;
        if cache_logical != source_logical {
            return Err(LifecycleError::Invalid);
        }
        self.room(2)?;
        if verify_info(cache, false)? != object
            || self.size(cache)? != size
            || self.resources.digest(cache, LIMIT, self.deadline)? != sha
        {
            return Err(LifecycleError::Unsettled);
        }
        self.room(2)?;
        if verify_info(source_file, false)? != source_object
            || self.size(source_file)? != source_bytes
            || self.resources.digest(source_file, LIMIT, self.deadline)? != source.source_sha256
        {
            return Err(LifecycleError::Unsettled);
        }
        if registration::capture(&self.resources, source, self.deadline)?.as_ref()
            != Some(&registration)
        {
            return Err(LifecycleError::Unsettled);
        }
        self.time()?;
        Ok(Some(VerifiedSameSourceMaintenance {
            registration,
            cache_object: object,
            cache_bytes: size,
            cache_sha: sha,
            source_sha: source.source_sha256,
            logical_sha: cache_logical,
            package: source.package.clone(),
            checked_msi_closes: self.closed_total,
        }))
    }
    fn pin(&mut self, path: &Path, cache: bool) -> Result<HANDLE> {
        valid_path(path)?;
        if cache {
            let mut parents: Vec<_> = path.ancestors().skip(1).collect();
            parents.reverse();
            if parents.len() > 48 {
                return Err(LifecycleError::Invalid);
            }
            for p in parents {
                self.room(1)?;
                let h = self.resources.open_file(
                    p,
                    FILE_READ_ATTRIBUTES.0 | READ_CONTROL.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    OPEN_EXISTING,
                    true,
                    false,
                )?;
                self.resources.add(h, Role::Parent);
                verify_info(h, true)?;
            }
        }
        self.room(1)?;
        let h = self.resources.open_file(
            path,
            GENERIC_READ.0 | READ_CONTROL.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        self.resources.add(h, Role::Control);
        verify_info(h, false)?;
        if cache {
            verify_acl(h, &self.resources.sid, false)?;
        }
        Ok(h)
    }
    fn size(&self, h: HANDLE) -> Result<u64> {
        self.time()?;
        let mut n = 0;
        unsafe {
            GetFileSizeEx(h, &mut n)?;
        };
        if n <= 0 || n as u64 > LIMIT {
            return Err(LifecycleError::Invalid);
        }
        Ok(n as u64)
    }
}
/// After Begin this uses only original native registration and pinned file
/// facts, never OpenDatabase. Temporary cache pins return BEFORE Run/retirement.
pub(super) fn revalidate(s: &mut Session) -> Result<()> {
    let Some(proof) = &s.verified_maintenance else {
        return Ok(());
    };
    if !proof.matches(s) {
        return Err(LifecycleError::Invalid);
    }
    let source = s.captured_source.as_ref().ok_or(LifecycleError::Invalid)?;
    if registration::capture(&s.resources, source, s.deadline)?.as_ref()
        != Some(&proof.registration)
    {
        return Err(LifecycleError::Unsettled);
    }
    let baseline = s.resources.handles.len();
    current(s.deadline)?;
    s.resources.check_live()?;
    // The temporary leaf and the digest's original CNG pair share the64 cap.
    if baseline + 3 > HANDLE_LIMIT {
        return Err(LifecycleError::Invalid);
    }
    let h = s.resources.open_file(
        &proof.registration.cache,
        GENERIC_READ.0 | READ_CONTROL.0,
        FILE_SHARE_READ,
        OPEN_EXISTING,
        false,
        false,
    )?;
    s.resources.add(h, Role::Control);
    let mut size = 0;
    unsafe {
        GetFileSizeEx(h, &mut size)?;
    }
    let result = if verify_info(h, false)? == proof.cache_object
        && size as u64 == proof.cache_bytes
        && s.resources.digest(h, LIMIT, s.deadline)? == proof.cache_sha
    {
        Ok(())
    } else {
        Err(LifecycleError::Unsettled)
    };
    while s.resources.handles.len() > baseline {
        s.resources.close(s.resources.handles.len() - 1)?;
    }
    result
}

// An allocation-only released sentinel while exclusive Session access moves
// its original owner into the stable reader Box. It acquires NO claim/handle.
pub(super) fn released_placeholder() -> Resources {
    Resources {
        handles: Vec::new(),
        slot: None,
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
        _thread: std::marker::PhantomData,
    }
}
