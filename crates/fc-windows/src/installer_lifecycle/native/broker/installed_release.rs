//! Native read-only installed-release capture, before any Begin/active install.
//! This is NOT VerifiedInstalledRelease or upgrade admission. The cached database
//! is opened readonly before any Begin. Trusted predecessor/release provenance
//! remains absent. No ordinary install/repair path calls this module.
use super::*;

mod cached_database;
mod database_rules;
mod format;

use cached_database::{CapturedCachedDatabase, CloseReceipt, MsiOwned};

const RELATED_BOUND: u32 = 16;
const CONTEXT_BOUND: u32 = 3;
const CACHE_LIMIT: u64 = 64 * 1024 * 1024; // Existing fixed-source MSI bound.

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReadError {
    Lifecycle(LifecycleError),
    NativeApi(u32),
    MalformedNativeMetadata,
    MultipleRelatedProducts,
    UnsupportedContext,
    NotInstalled,
    EnumerationBound,
    ChangedDuringRead,
}
impl From<LifecycleError> for ReadError {
    fn from(error: LifecycleError) -> Self {
        Self::Lifecycle(error)
    }
}
impl From<windows::core::Error> for ReadError {
    fn from(error: windows::core::Error) -> Self {
        Self::Lifecycle(error.into())
    }
}
type ReadResult<T> = std::result::Result<T, ReadError>;

#[derive(Debug)]
pub(super) struct CapturedInstalledRelease {
    product: String,
    version: [u32; 3],
    sid: String,
    local_package: PathBuf,
    cache_object: (u32, u64),
    cache_bytes: u64,
    cache_sha256: [u8; 32],
    database: Box<CapturedCachedDatabase>,
}
impl CapturedInstalledRelease {
    pub(super) fn cached_database(&self) -> &CapturedCachedDatabase {
        &self.database
    }
    pub(super) fn product_code(&self) -> &str {
        &self.product
    }
    pub(super) fn version(&self) -> [u32; 3] {
        self.version
    }
    pub(super) fn sid(&self) -> &str {
        &self.sid
    }
    pub(super) fn local_package(&self) -> &Path {
        &self.local_package
    }
    pub(super) fn cache_object(&self) -> (u32, u64) {
        self.cache_object
    }
    pub(super) fn cache_bytes(&self) -> u64 {
        self.cache_bytes
    }
    pub(super) fn cache_sha256(&self) -> [u8; 32] {
        self.cache_sha256
    }
}

#[derive(Debug)]
pub(super) enum InstalledReleaseObservation {
    /// Only the current native API visibility scope, never a Fresh permit or
    /// proof of absence for other deployment accounts.
    NoneInCurrentUserApiScope,
    Single(CapturedInstalledRelease),
}

#[derive(Debug, PartialEq, Eq)]
struct Registration {
    product: String,
    version: [u32; 3],
    local_package: PathBuf,
}

pub(super) struct InstalledReleaseReader {
    resources: Resources,
    deadline: Instant,
    sid: String,
    msi: Vec<MsiOwned>,
    close_receipts: Vec<CloseReceipt>,
}
impl InstalledReleaseReader {
    /// Caller supplies the original operation deadline. This function creates
    /// no gate/Pending, invokes no MSI action, and grants no maintenance right.
    /// It must not be called by a CA or while an installation is running.
    pub(super) fn capture(deadline: Instant) -> ReadResult<InstalledReleaseObservation> {
        current(deadline)?;
        if deadline.saturating_duration_since(Instant::now())
            > super::super::super::broker::BROKER_BUDGET
        {
            return Err(ReadError::Lifecycle(LifecycleError::Invalid));
        }
        let mut reader = Box::new(Self {
            resources: Resources::empty()?,
            deadline,
            sid: String::new(),
            msi: Vec::with_capacity(HANDLE_LIMIT),
            close_receipts: Vec::with_capacity(database_rules::RECEIPT_LIMIT),
        });
        // A known synchronous native/format failure is a negative observation,
        // not an abandoned producer. Close every original MSI object first,
        // then all CNG/kernel controls, on this same creating thread.
        let observed = reader.read();
        let closed = if reader.resources.failed {
            // A prior checked CNG/kernel close failed; do not continue closing
            // other original objects or release its claim after that unknown.
            Err(ReadError::Lifecycle(LifecycleError::Faulted))
        } else {
            reader
                .close_all_msi()
                .and_then(|()| reader.resources.release().map_err(ReadError::from))
        };
        if let Err(error) = closed {
            // One failed checked close is sticky. Keep this exact Box, its MSI
            // close-attempt/actual-return ledger and original8-owner claim to
            // process exit. Never retry via Drop or release the claim early.
            reader.resources.retain_until_exit();
            std::mem::forget(reader);
            return Err(error);
        }
        current(deadline)?;
        observed
    }

    fn check(&self) -> ReadResult<()> {
        current(self.deadline)?;
        self.resources.check_live()?;
        Ok(())
    }

    fn read(&mut self) -> ReadResult<InstalledReleaseObservation> {
        self.check()?;
        self.resources.sid = self.resources.token_sid(unsafe { GetCurrentProcess() })?;
        require_user(&self.resources.sid)?;
        self.sid =
            format::sid_text(&self.resources.sid).ok_or(ReadError::MalformedNativeMetadata)?;
        let Some(product) = self.related()? else {
            return Ok(InstalledReleaseObservation::NoneInCurrentUserApiScope);
        };
        let before = self.registration(&product)?;
        let (file, object) = self.pin_cache(&before.local_package)?;
        let cache_bytes = self.size(file)?;
        let cache_sha256 = self.resources.digest(file, CACHE_LIMIT, self.deadline)?;
        let database = self.cached_database(&before)?;
        self.check()?;
        // Same thread and same API scope throughout. A related-product or
        // registration change during the read is not an upgrade candidate.
        if self.related()?.as_deref() != Some(product.as_str())
            || self.registration(&product)? != before
            || verify_info(file, false)? != object
            || self.size(file)? != cache_bytes
            || self.resources.digest(file, CACHE_LIMIT, self.deadline)? != cache_sha256
        {
            return Err(ReadError::ChangedDuringRead);
        }
        Ok(InstalledReleaseObservation::Single(
            CapturedInstalledRelease {
                product: before.product,
                version: before.version,
                sid: self.sid.clone(),
                local_package: before.local_package,
                cache_object: object,
                cache_bytes,
                cache_sha256,
                database: Box::new(database),
            },
        ))
    }

    fn related(&self) -> ReadResult<Option<String>> {
        let family = HSTRING::from(format!("{{{INSTALL_FAMILY}}}"));
        let mut found = BTreeSet::new();
        for index in 0..=RELATED_BOUND {
            self.check()?;
            let mut output = [0u16; 39];
            let code = unsafe {
                MsiEnumRelatedProductsW(
                    &family,
                    None,
                    index,
                    windows::core::PWSTR(output.as_mut_ptr()),
                )
            };
            self.check()?;
            if code == ERROR_NO_MORE_ITEMS.0 {
                return match found.len() {
                    0 => Ok(None),
                    1 => Ok(found.into_iter().next()),
                    _ => Err(ReadError::MultipleRelatedProducts),
                };
            }
            if code != 0 {
                return Err(ReadError::NativeApi(code));
            }
            if index == RELATED_BOUND {
                return Err(ReadError::EnumerationBound);
            }
            let text = exact_guid_buffer(&output)?;
            if !found.insert(text) {
                return Err(ReadError::ChangedDuringRead);
            }
        }
        Err(ReadError::EnumerationBound)
    }

    fn registration(&self, product: &str) -> ReadResult<Registration> {
        let braced = HSTRING::from(format!("{{{product}}}"));
        self.context(&braced, product)?;
        if self.property(&braced, "State")? != "5" {
            return Err(ReadError::NotInstalled);
        }
        let version = format::version(&self.property(&braced, "VersionString")?)
            .ok_or(ReadError::MalformedNativeMetadata)?;
        let local_package = self.property(&braced, "LocalPackage")?;
        let mut windows = [0u16; PATH_LIMIT];
        self.check()?;
        let count = unsafe { GetWindowsDirectoryW(Some(&mut windows)) } as usize;
        self.check()?;
        if count == 0 || count >= windows.len() {
            return Err(ReadError::MalformedNativeMetadata);
        }
        let directory = String::from_utf16(&windows[..count])
            .map_err(|_| ReadError::MalformedNativeMetadata)?;
        if !format::cache_path(&directory, &local_package) {
            return Err(ReadError::MalformedNativeMetadata);
        }
        Ok(Registration {
            product: product.to_owned(),
            version,
            local_package: PathBuf::from(local_package),
        })
    }

    fn context(&self, product: &HSTRING, expected: &str) -> ReadResult<()> {
        let mut supported = true;
        for index in 0..=CONTEXT_BOUND {
            self.check()?;
            let mut output = [0u16; 39];
            let mut context = MSIINSTALLCONTEXT_NONE;
            let mut sid = [0u16; 184];
            let mut length = (sid.len() - 1) as u32;
            let code = unsafe {
                MsiEnumProductsExW(
                    product,
                    None,
                    MSIINSTALLCONTEXT_ALL.0 as u32,
                    index,
                    Some(&mut output),
                    Some(&mut context),
                    Some(windows::core::PWSTR(sid.as_mut_ptr())),
                    Some(&mut length),
                )
            };
            self.check()?;
            if code == ERROR_NO_MORE_ITEMS.0 {
                return if index == 1 && supported {
                    Ok(())
                } else {
                    Err(ReadError::UnsupportedContext)
                };
            }
            if code != 0 {
                return Err(ReadError::NativeApi(code));
            }
            if index == CONTEXT_BOUND {
                return Err(ReadError::EnumerationBound);
            }
            if exact_guid_buffer(&output)? != expected || length as usize >= sid.len() {
                return Err(ReadError::MalformedNativeMetadata);
            }
            let actual_sid = String::from_utf16(&sid[..length as usize])
                .map_err(|_| ReadError::MalformedNativeMetadata)?;
            supported &= context == MSIINSTALLCONTEXT_USERUNMANAGED && actual_sid == self.sid;
        }
        Err(ReadError::EnumerationBound)
    }

    fn property(&self, product: &HSTRING, name: &str) -> ReadResult<String> {
        let mut output = [0u16; PATH_LIMIT];
        let mut length = (output.len() - 1) as u32;
        self.check()?;
        let code = unsafe {
            MsiGetProductInfoExW(
                product,
                None,
                MSIINSTALLCONTEXT_USERUNMANAGED,
                &HSTRING::from(name),
                Some(windows::core::PWSTR(output.as_mut_ptr())),
                Some(&mut length),
            )
        };
        self.check()?;
        if code != 0 {
            return Err(ReadError::NativeApi(code));
        }
        if length == 0 || length as usize >= output.len() {
            return Err(ReadError::MalformedNativeMetadata);
        }
        let value = String::from_utf16(&output[..length as usize])
            .map_err(|_| ReadError::MalformedNativeMetadata)?;
        if value.chars().any(|c| c.is_control()) {
            return Err(ReadError::MalformedNativeMetadata);
        }
        Ok(value)
    }

    fn pin_cache(&mut self, path: &Path) -> ReadResult<(HANDLE, (u32, u64))> {
        valid_path(path)?;
        let mut parents: Vec<_> = path.ancestors().skip(1).collect();
        parents.reverse();
        if parents.len() > 48 {
            return Err(ReadError::MalformedNativeMetadata);
        }
        for ancestor in parents {
            self.check()?;
            let handle = self.resources.open_file(
                ancestor,
                FILE_READ_ATTRIBUTES.0 | READ_CONTROL.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                OPEN_EXISTING,
                true,
                false,
            )?;
            self.resources.add(handle, Role::Parent);
            verify_info(handle, true)?;
        }
        let file = self.resources.open_file(
            path,
            GENERIC_READ.0 | READ_CONTROL.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        self.resources.add(file, Role::Control);
        let object = verify_info(file, false)?;
        verify_acl(file, &self.resources.sid, false)?;
        Ok((file, object))
    }

    fn size(&self, file: HANDLE) -> ReadResult<u64> {
        self.check()?;
        let mut size = 0;
        unsafe { GetFileSizeEx(file, &mut size)? };
        self.check()?;
        if size <= 0 || size as u64 > CACHE_LIMIT {
            return Err(ReadError::MalformedNativeMetadata);
        }
        Ok(size as u64)
    }
}

fn exact_guid_buffer(output: &[u16; 39]) -> ReadResult<String> {
    if output[38] != 0 {
        return Err(ReadError::MalformedNativeMetadata);
    }
    let text = String::from_utf16(&output[..38]).map_err(|_| ReadError::MalformedNativeMetadata)?;
    format::product_code(&text).ok_or(ReadError::MalformedNativeMetadata)
}

// Unwinding must never let Resources Drop release a claim while raw MSI objects
// remain. Ordinary returned errors take the explicit checked-cleanup path above.
// This fallback retains the original live MSI ledger allocation without calling
// any MSI close from Drop; Resources carries !Send/!Sync and same-thread use.
impl Drop for InstalledReleaseReader {
    fn drop(&mut self) {
        if !self.msi.is_empty() {
            self.resources.retain_until_exit();
            std::mem::forget(std::mem::take(&mut self.msi));
            std::mem::forget(std::mem::take(&mut self.close_receipts));
        }
    }
}
