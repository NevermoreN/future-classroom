//! Before-Begin readonly database facts. No SQL/path/identity is caller supplied.
//! Every owned MSI object is closed on its creating thread, in its own namespace.
use super::*;
use database_rules::{
    CloseState, CustomActionRow, DirectoryRow, ExecuteSequenceRow, BINARY_LIMIT,
    CUSTOM_ACTION_LIMIT, DIRECTORY_LIMIT, EXECUTE_SEQUENCE_LIMIT, RECEIPT_LIMIT, ROW_LIMIT,
    TEXT_LIMIT,
};
use windows::core::{PCWSTR, PSTR, PWSTR};

const PROPERTIES: &str = "SELECT `Property`, `Value` FROM `Property` WHERE `Property` = 'ProductCode' OR `Property` = 'UpgradeCode' OR `Property` = 'ProductVersion'";
const COMPONENTS: &str = "SELECT `Component`, `ComponentId`, `Directory_`, `Attributes`, `Condition`, `KeyPath` FROM `Component`";
const FILES: &str = "SELECT `File`, `Component_`, `FileName`, `FileSize`, `Version`, `Language`, `Attributes`, `Sequence` FROM `File`";
const DIRECTORIES: &str = "SELECT `Directory`, `Directory_Parent`, `DefaultDir` FROM `Directory`";
const CUSTOM_ACTIONS: &str = "SELECT `Action`, `Type`, `Source`, `Target` FROM `CustomAction`";
const EXECUTE_SEQUENCE: &str =
    "SELECT `Action`, `Condition`, `Sequence` FROM `InstallExecuteSequence`";
const BINARIES: &str = "SELECT `Name` FROM `Binary`";
const TEMPLATE: &str = "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcGuardClientTemplate'";
const PAYLOAD: &str = "SELECT `Data` FROM `Binary` WHERE `Name` = 'FcPayloadManifest'";

#[derive(Clone, Copy, Debug)]
pub(super) enum MsiKind {
    Database,
    View,
    Record,
    Summary,
}
#[derive(Debug)]
pub(super) struct CloseReceipt {
    pub(super) kind: MsiKind,
    pub(super) handle: u32,
    pub(super) view_close_return: Option<u32>,
    pub(super) handle_close_return: u32,
}
pub(super) struct MsiOwned {
    handle: MSIHANDLE,
    kind: MsiKind,
    close: CloseState,
}
#[derive(Debug)]
pub(super) struct StreamIdentity {
    pub(super) bytes: u32,
    pub(super) sha256: [u8; 32],
}
#[derive(Debug)]
pub(super) enum CapturedManifestFormat {
    /// Existing33/c508 packages have no new manifest Binary. Never synthesize it.
    LegacyWithoutPayloadManifest,
    /// Presence and complete stream identity only, not a recognized/published ABI.
    PayloadManifestPresentUnparsed(StreamIdentity),
}
#[derive(Debug)]
pub(super) struct ComponentRow {
    pub(super) component: String,
    pub(super) component_id: Option<String>,
    pub(super) directory: String,
    pub(super) attributes: i32,
    pub(super) condition: Option<String>,
    pub(super) key_path: Option<String>,
}
#[derive(Debug)]
pub(super) struct FileRow {
    pub(super) file: String,
    pub(super) component: String,
    pub(super) filename: String,
    pub(super) size: i32,
    pub(super) version: Option<String>,
    pub(super) language: Option<String>,
    pub(super) attributes: i32,
    pub(super) sequence: i32,
}
#[derive(Debug)]
pub(in super::super) struct CapturedCachedDatabase {
    pub(super) product: String,
    pub(super) upgrade: String,
    pub(super) version: [u32; 3],
    pub(super) package: String,
    pub(super) template: StreamIdentity,
    pub(super) manifest_format: CapturedManifestFormat,
    pub(super) binary_names: Vec<String>,
    pub(super) components: Vec<ComponentRow>,
    pub(super) files: Vec<FileRow>,
    pub(super) directories: Vec<DirectoryRow>,
    pub(super) custom_actions: Vec<CustomActionRow>,
    pub(super) execute_sequence: Vec<ExecuteSequenceRow>,
    pub(super) close_receipts: Vec<CloseReceipt>,
}
impl InstalledReleaseReader {
    fn room(&self, incoming: usize) -> ReadResult<()> {
        self.check()?;
        if !database_rules::room(self.resources.handles.len(), self.msi.len(), incoming) {
            return Err(ReadError::Lifecycle(LifecycleError::Invalid));
        }
        Ok(())
    }
    fn msi_room(&self, incoming: usize) -> ReadResult<()> {
        self.room(incoming)?;
        if !database_rules::receipt_room(self.close_receipts.len(), self.msi.len(), incoming) {
            return Err(ReadError::EnumerationBound);
        }
        Ok(())
    }
    fn acquired(&mut self, code: u32, handle: MSIHANDLE, kind: MsiKind) -> ReadResult<MSIHANDLE> {
        // Register any nonzero output BEFORE checking native return/deadline, so
        // even an error with an unexpected output cannot drop a capability.
        if handle.0 != 0 {
            self.msi.push(MsiOwned {
                handle,
                kind,
                close: CloseState::default(),
            });
        }
        self.native_code(code)?;
        if handle.0 == 0 {
            return Err(ReadError::MalformedNativeMetadata);
        }
        Ok(handle)
    }
    fn native_code(&self, code: u32) -> ReadResult<()> {
        self.check()?;
        if code != 0 {
            return Err(ReadError::NativeApi(code));
        }
        Ok(())
    }
    fn open_view(&mut self, db: MSIHANDLE, query: &'static str) -> ReadResult<MSIHANDLE> {
        self.msi_room(1)?;
        let mut view = MSIHANDLE(0);
        let code = unsafe { MsiDatabaseOpenViewW(db, &HSTRING::from(query), &mut view) };
        let view = self.acquired(code, view, MsiKind::View)?;
        let code = unsafe { MsiViewExecute(view, MSIHANDLE(0)) };
        // Only a successful Execute owns a result set requiring ViewClose.
        if code == 0 {
            self.msi
                .last_mut()
                .ok_or(ReadError::MalformedNativeMetadata)?
                .close
                .view_open = true;
        }
        self.native_code(code)?;
        Ok(view)
    }
    fn fetch(&mut self, view: MSIHANDLE) -> ReadResult<Option<MSIHANDLE>> {
        self.msi_room(1)?;
        let mut record = MSIHANDLE(0);
        let code = unsafe { MsiViewFetch(view, &mut record) };
        if code == ERROR_NO_MORE_ITEMS.0 && record.0 == 0 {
            self.check()?;
            return Ok(None);
        }
        self.acquired(code, record, MsiKind::Record).map(Some)
    }
    pub(super) fn close_msi(&mut self, handle: MSIHANDLE) -> ReadResult<()> {
        let index = self
            .msi
            .iter()
            .position(|item| item.handle == handle)
            .ok_or(ReadError::MalformedNativeMetadata)?;
        if self.close_receipts.len() >= RECEIPT_LIMIT {
            return Err(ReadError::EnumerationBound);
        }
        let item = &mut self.msi[index];
        if item.close.view_open {
            if !item.close.view_attempt() {
                return Err(ReadError::Lifecycle(LifecycleError::Faulted));
            }
            let code = unsafe { MsiViewClose(handle) };
            item.close.view_returned(code);
            if code != 0 {
                return Err(ReadError::NativeApi(code));
            }
        }
        if !item.close.handle_attempt() {
            return Err(ReadError::Lifecycle(LifecycleError::Faulted));
        }
        let code = unsafe { MsiCloseHandle(handle) };
        item.close.handle_returned(code);
        if code != 0 {
            return Err(ReadError::NativeApi(code));
        }
        let item = self.msi.remove(index);
        self.close_receipts.push(CloseReceipt {
            kind: item.kind,
            handle: handle.0,
            view_close_return: item.close.view_close_return,
            handle_close_return: code,
        });
        Ok(())
    }
    pub(super) fn close_all_msi(&mut self) -> ReadResult<()> {
        // Cleanup also runs after a known synchronous API/validation error or
        // deadline. Do not time-gate checked cleanup. First close failure stops
        // permanently: capture retains this same Box/claim, no Drop close retry.
        while let Some(item) = self.msi.last() {
            if let Some(code) = item
                .close
                .view_close_return
                .filter(|c| *c != 0)
                .or(item.close.handle_close_return.filter(|c| *c != 0))
            {
                return Err(ReadError::NativeApi(code));
            }
            self.close_msi(item.handle)?;
        }
        Ok(())
    }
    fn string(&self, record: MSIHANDLE, field: u32) -> ReadResult<Option<String>> {
        self.check()?;
        if unsafe { MsiRecordIsNull(record, field) }.as_bool() {
            return Ok(None);
        }
        let mut buf = [0u16; TEXT_LIMIT + 1];
        let mut len = TEXT_LIMIT as u32;
        let code = unsafe {
            MsiRecordGetStringW(record, field, Some(PWSTR(buf.as_mut_ptr())), Some(&mut len))
        };
        self.native_code(code)?;
        if len as usize > TEXT_LIMIT {
            return Err(ReadError::MalformedNativeMetadata);
        }
        let value = String::from_utf16(&buf[..len as usize])
            .map_err(|_| ReadError::MalformedNativeMetadata)?;
        if !database_rules::text(&value) {
            return Err(ReadError::MalformedNativeMetadata);
        }
        Ok(Some(value))
    }
    fn required(&self, record: MSIHANDLE, field: u32) -> ReadResult<String> {
        self.string(record, field)?
            .filter(|s| !s.is_empty())
            .ok_or(ReadError::MalformedNativeMetadata)
    }
    fn integer(&self, record: MSIHANDLE, field: u32) -> ReadResult<i32> {
        self.check()?;
        let n = unsafe { MsiRecordGetInteger(record, field) };
        self.check()?;
        if n == MSI_NULL_INTEGER as i32 {
            return Err(ReadError::MalformedNativeMetadata);
        }
        Ok(n)
    }
    fn optional_integer(&self, record: MSIHANDLE, field: u32) -> ReadResult<Option<i32>> {
        self.check()?;
        if unsafe { MsiRecordIsNull(record, field) }.as_bool() {
            return Ok(None);
        }
        self.integer(record, field).map(Some)
    }
    fn properties(&mut self, db: MSIHANDLE) -> ReadResult<(String, String, [u32; 3])> {
        let view = self.open_view(db, PROPERTIES)?;
        let mut values = std::collections::BTreeMap::new();
        while let Some(row) = self.fetch(view)? {
            if values.len() >= 3 {
                return Err(ReadError::EnumerationBound);
            }
            let name = self.required(row, 1)?;
            let value = self.required(row, 2)?;
            if values.insert(name, value).is_some() {
                return Err(ReadError::MalformedNativeMetadata);
            }
            self.close_msi(row)?;
        }
        self.close_msi(view)?;
        let product = format::product_code(
            values
                .get("ProductCode")
                .ok_or(ReadError::MalformedNativeMetadata)?,
        )
        .ok_or(ReadError::MalformedNativeMetadata)?;
        let upgrade = format::product_code(
            values
                .get("UpgradeCode")
                .ok_or(ReadError::MalformedNativeMetadata)?,
        )
        .ok_or(ReadError::MalformedNativeMetadata)?;
        let version = format::version(
            values
                .get("ProductVersion")
                .ok_or(ReadError::MalformedNativeMetadata)?,
        )
        .ok_or(ReadError::MalformedNativeMetadata)?;
        Ok((product, upgrade, version))
    }
    fn package_code(&mut self, db: MSIHANDLE) -> ReadResult<String> {
        self.msi_room(1)?;
        let mut summary = MSIHANDLE(0);
        let code = unsafe { MsiGetSummaryInformationW(db, PCWSTR::null(), 0, &mut summary) };
        let summary = self.acquired(code, summary, MsiKind::Summary)?;
        let mut kind = 0;
        let mut integer = 0;
        let mut buf = [0u16; 39];
        let mut len = buf.len() as u32;
        let code = unsafe {
            MsiSummaryInfoGetPropertyW(
                summary,
                9,
                &mut kind,
                &mut integer,
                None,
                Some(PWSTR(buf.as_mut_ptr())),
                Some(&mut len),
            )
        };
        self.native_code(code)?;
        // PID_REVNUMBER is VT_LPSTR=30. A transform revision list or non-GUID
        // summary is not a package identity, even if its first GUID matches.
        if kind != 30 || len != 38 || buf[38] != 0 {
            return Err(ReadError::MalformedNativeMetadata);
        }
        let package = exact_guid_buffer(&buf)?;
        self.close_msi(summary)?;
        Ok(package)
    }
    fn binary_names(&mut self, db: MSIHANDLE) -> ReadResult<Vec<String>> {
        let view = self.open_view(db, BINARIES)?;
        let mut names = BTreeSet::new();
        while let Some(row) = self.fetch(view)? {
            if names.len() >= BINARY_LIMIT || !names.insert(self.required(row, 1)?) {
                return Err(ReadError::EnumerationBound);
            }
            self.close_msi(row)?;
        }
        self.close_msi(view)?;
        Ok(names.into_iter().collect())
    }
    fn stream(&mut self, db: MSIHANDLE, query: &'static str) -> ReadResult<StreamIdentity> {
        let view = self.open_view(db, query)?;
        let row = self
            .fetch(view)?
            .ok_or(ReadError::MalformedNativeMetadata)?;
        self.check()?;
        let length = unsafe { MsiRecordDataSize(row, 1) };
        if length == 0 || u64::from(length) > CACHE_LIMIT {
            return Err(ReadError::MalformedNativeMetadata);
        }
        self.room(2)?;
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        let code = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut algorithm,
                BCRYPT_SHA256_ALGORITHM,
                None,
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
        };
        if !algorithm.0.is_null() {
            self.resources.add(HANDLE(algorithm.0), Role::Algorithm);
        }
        code.ok()?;
        let mut hash = BCRYPT_HASH_HANDLE::default();
        let code = unsafe { BCryptCreateHash(algorithm, &mut hash, None, None, 0) };
        if !hash.0.is_null() {
            self.resources.add(HANDLE(hash.0), Role::Hash);
        }
        code.ok()?;
        let mut buffer = [0u8; 16384];
        let mut remaining = length as usize;
        while remaining != 0 {
            self.check()?;
            let mut count = buffer.len().min(remaining) as u32;
            let requested = count;
            let code =
                unsafe { MsiRecordReadStream(row, 1, Some(PSTR(buffer.as_mut_ptr())), &mut count) };
            self.native_code(code)?;
            if count == 0 || count > requested {
                return Err(ReadError::MalformedNativeMetadata);
            }
            unsafe {
                BCryptHashData(hash, &buffer[..count as usize], 0).ok()?;
            }
            remaining -= count as usize;
        }
        let mut end = 1;
        self.native_code(unsafe {
            MsiRecordReadStream(row, 1, Some(PSTR(buffer.as_mut_ptr())), &mut end)
        })?;
        if end != 0 {
            return Err(ReadError::ChangedDuringRead);
        }
        let mut digest = [0u8; 32];
        unsafe {
            BCryptFinishHash(hash, &mut digest, 0).ok()?;
        }
        for (role, handle) in [
            (Role::Hash, HANDLE(hash.0)),
            (Role::Algorithm, HANDLE(algorithm.0)),
        ] {
            let i = self
                .resources
                .handles
                .iter()
                .position(|h| h.role == role && h.handle == handle)
                .ok_or(ReadError::MalformedNativeMetadata)?;
            self.resources.close(i)?;
        }
        self.close_msi(row)?;
        if self.fetch(view)?.is_some() {
            return Err(ReadError::MalformedNativeMetadata);
        }
        self.close_msi(view)?;
        Ok(StreamIdentity {
            bytes: length,
            sha256: digest,
        })
    }
    fn components(&mut self, db: MSIHANDLE) -> ReadResult<Vec<ComponentRow>> {
        let view = self.open_view(db, COMPONENTS)?;
        let mut result = Vec::new();
        let mut ids = BTreeSet::new();
        while let Some(row) = self.fetch(view)? {
            if result.len() >= ROW_LIMIT {
                return Err(ReadError::EnumerationBound);
            }
            let value = ComponentRow {
                component: self.required(row, 1)?,
                component_id: self.string(row, 2)?,
                directory: self.required(row, 3)?,
                attributes: self.integer(row, 4)?,
                condition: self.string(row, 5)?,
                key_path: self.string(row, 6)?,
            };
            if !ids.insert(value.component.clone()) {
                return Err(ReadError::MalformedNativeMetadata);
            }
            result.push(value);
            self.close_msi(row)?;
        }
        self.close_msi(view)?;
        Ok(result)
    }
    fn files(&mut self, db: MSIHANDLE) -> ReadResult<Vec<FileRow>> {
        let view = self.open_view(db, FILES)?;
        let mut result = Vec::new();
        let mut ids = BTreeSet::new();
        while let Some(row) = self.fetch(view)? {
            if result.len() >= ROW_LIMIT {
                return Err(ReadError::EnumerationBound);
            }
            let value = FileRow {
                file: self.required(row, 1)?,
                component: self.required(row, 2)?,
                filename: self.required(row, 3)?,
                size: self.integer(row, 4)?,
                version: self.string(row, 5)?,
                language: self.string(row, 6)?,
                attributes: self.integer(row, 7)?,
                sequence: self.integer(row, 8)?,
            };
            if !ids.insert(value.file.clone()) {
                return Err(ReadError::MalformedNativeMetadata);
            }
            result.push(value);
            self.close_msi(row)?;
        }
        self.close_msi(view)?;
        Ok(result)
    }
    fn directories(&mut self, db: MSIHANDLE) -> ReadResult<Vec<DirectoryRow>> {
        let view = self.open_view(db, DIRECTORIES)?;
        let mut result = Vec::new();
        let mut ids = BTreeSet::new();
        while let Some(row) = self.fetch(view)? {
            if result.len() >= DIRECTORY_LIMIT {
                return Err(ReadError::EnumerationBound);
            }
            let value = DirectoryRow {
                directory: self.required(row, 1)?,
                parent: self.string(row, 2)?,
                default_dir: self.required(row, 3)?,
            };
            if !ids.insert(value.directory.clone()) {
                return Err(ReadError::MalformedNativeMetadata);
            }
            result.push(value);
            self.close_msi(row)?;
        }
        self.close_msi(view)?;
        Ok(result)
    }
    fn custom_actions(&mut self, db: MSIHANDLE) -> ReadResult<Vec<CustomActionRow>> {
        let view = self.open_view(db, CUSTOM_ACTIONS)?;
        let mut result = Vec::new();
        let mut ids = BTreeSet::new();
        while let Some(row) = self.fetch(view)? {
            if result.len() >= CUSTOM_ACTION_LIMIT {
                return Err(ReadError::EnumerationBound);
            }
            let value = CustomActionRow {
                action: self.required(row, 1)?,
                kind: self.integer(row, 2)?,
                source: self.string(row, 3)?,
                target: self.string(row, 4)?,
            };
            if !ids.insert(value.action.clone()) {
                return Err(ReadError::MalformedNativeMetadata);
            }
            result.push(value);
            self.close_msi(row)?;
        }
        self.close_msi(view)?;
        Ok(result)
    }
    fn execute_sequence(&mut self, db: MSIHANDLE) -> ReadResult<Vec<ExecuteSequenceRow>> {
        let view = self.open_view(db, EXECUTE_SEQUENCE)?;
        let mut result = Vec::new();
        let mut ids = BTreeSet::new();
        while let Some(row) = self.fetch(view)? {
            if result.len() >= EXECUTE_SEQUENCE_LIMIT {
                return Err(ReadError::EnumerationBound);
            }
            let value = ExecuteSequenceRow {
                action: self.required(row, 1)?,
                condition: self.string(row, 2)?,
                sequence: self.optional_integer(row, 3)?,
            };
            if !ids.insert(value.action.clone()) {
                return Err(ReadError::MalformedNativeMetadata);
            }
            result.push(value);
            self.close_msi(row)?;
        }
        self.close_msi(view)?;
        Ok(result)
    }
    pub(super) fn cached_database(
        &mut self,
        before: &Registration,
    ) -> ReadResult<CapturedCachedDatabase> {
        self.msi_room(1)?;
        let path = native_path(&before.local_package)?;
        let mut db = MSIHANDLE(0);
        let code = unsafe { MsiOpenDatabaseW(&path, MSIDBOPEN_READONLY, &mut db) };
        let db = self.acquired(code, db, MsiKind::Database)?;
        let (product, upgrade, version) = self.properties(db)?;
        if product != before.product || upgrade != INSTALL_FAMILY || version != before.version {
            return Err(ReadError::ChangedDuringRead);
        }
        let package = self.package_code(db)?;
        let binary_names = self.binary_names(db)?;
        if !binary_names.iter().any(|n| n == "FcGuardClientTemplate") {
            return Err(ReadError::MalformedNativeMetadata);
        }
        let template = self.stream(db, TEMPLATE)?;
        let manifest_format = if binary_names.iter().any(|n| n == "FcPayloadManifest") {
            CapturedManifestFormat::PayloadManifestPresentUnparsed(self.stream(db, PAYLOAD)?)
        } else {
            CapturedManifestFormat::LegacyWithoutPayloadManifest
        };
        let components = self.components(db)?;
        let files = self.files(db)?;
        let directories = self.directories(db)?;
        let custom_actions = self.custom_actions(db)?;
        let execute_sequence = self.execute_sequence(db)?;
        let paths = database_rules::declared_directory_paths(&directories)
            .ok_or(ReadError::MalformedNativeMetadata)?;
        if components
            .iter()
            .any(|row| !paths.contains_key(&row.directory))
            || !database_rules::fixed_guard_shape(&custom_actions, &execute_sequence, &binary_names)
        {
            return Err(ReadError::MalformedNativeMetadata);
        }
        self.close_msi(db)?;
        self.check()?;
        Ok(CapturedCachedDatabase {
            product,
            upgrade,
            version,
            package,
            template,
            manifest_format,
            binary_names,
            components,
            files,
            directories,
            custom_actions,
            execute_sequence,
            close_receipts: std::mem::take(&mut self.close_receipts),
        })
    }
}

#[cfg(test)]
mod native_probe;
