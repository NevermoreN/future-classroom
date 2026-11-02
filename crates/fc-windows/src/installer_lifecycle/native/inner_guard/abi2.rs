//! Readonly facts from this CA's active database. No package-opening API and no
//! caller property, path, hash or role participates in this observation.
use super::*;
use crate::installer_lifecycle::{
    broker::{carrier, payload_manifest},
    guard_contract::v2::ObservedPackage,
    INSTALL_FAMILY,
};
const PROPERTIES: &str = "SELECT `Property`, `Value` FROM `Property` WHERE `Property` = 'ProductCode' OR `Property` = 'UpgradeCode' OR `Property` = 'ProductVersion'";
const TRAILER: usize = 132;
pub(super) enum PackageFormat {
    Legacy,
    Abi2(ObservedPackage),
}
impl GuardState {
    pub(super) fn abi2_image_hole(&self, image: HANDLE, length: usize) -> Result<usize> {
        super::carrier_image::hole(length, |at, bytes| self.read_at(image, at, bytes))
    }

    fn abi2_acquired(&mut self, code: u32, handle: MSIHANDLE) -> Result<MSIHANDLE> {
        // Preserve even an unexpected nonzero output on API failure.
        if handle.0 != 0 {
            self.register_msi(handle, false)?;
        }
        self.time()?;
        if code != 0 || handle.0 == 0 {
            return Err(LifecycleError::Invalid);
        }
        Ok(handle)
    }
    fn abi2_view(&mut self, db: MSIHANDLE, query: &'static str) -> Result<MSIHANDLE> {
        self.room(1)?;
        let mut view = MSIHANDLE(0);
        let code = unsafe { MsiDatabaseOpenViewW(db, &HSTRING::from(query), &mut view) };
        let view = self.abi2_acquired(code, view)?;
        let code = unsafe { MsiViewExecute(view, MSIHANDLE(0)) };
        if code == 0 {
            self.msi
                .iter_mut()
                .find(|v| v.handle == view)
                .ok_or(LifecycleError::Invalid)?
                .view_open = true;
        }
        self.time()?;
        if code != 0 {
            return Err(LifecycleError::Invalid);
        }
        Ok(view)
    }
    fn abi2_fetch(&mut self, view: MSIHANDLE) -> Result<Option<MSIHANDLE>> {
        self.room(1)?;
        let mut row = MSIHANDLE(0);
        let code = unsafe { MsiViewFetch(view, &mut row) };
        if code == ERROR_NO_MORE_ITEMS.0 && row.0 == 0 {
            self.time()?;
            return Ok(None);
        }
        self.abi2_acquired(code, row).map(Some)
    }
    fn abi2_one(&mut self, db: MSIHANDLE, query: &'static str) -> Result<(MSIHANDLE, MSIHANDLE)> {
        let view = self.abi2_view(db, query)?;
        let row = self.abi2_fetch(view)?.ok_or(LifecycleError::Invalid)?;
        Ok((view, row))
    }
    fn abi2_end(&mut self, view: MSIHANDLE, row: MSIHANDLE) -> Result<()> {
        if self.abi2_fetch(view)?.is_some() {
            return Err(LifecycleError::Invalid);
        }
        self.close_msi(row)?;
        self.close_msi(view)
    }
    fn abi2_stream_read(&self, row: MSIHANDLE, bytes: &mut [u8]) -> Result<()> {
        self.time()?;
        let mut count = bytes.len() as u32;
        let code =
            unsafe { MsiRecordReadStream(row, 1, Some(PSTR(bytes.as_mut_ptr())), &mut count) };
        if code != 0 || count as usize != bytes.len() {
            return Err(LifecycleError::Invalid);
        }
        self.time()
    }
    fn abi2_stream_end(&self, row: MSIHANDLE) -> Result<()> {
        let mut byte = [0];
        let mut count = 1;
        let code =
            unsafe { MsiRecordReadStream(row, 1, Some(PSTR(byte.as_mut_ptr())), &mut count) };
        if code != 0 || count != 0 {
            return Err(LifecycleError::Invalid);
        }
        self.time()
    }
    pub(super) fn package_format(&mut self, install: MSIHANDLE) -> Result<PackageFormat> {
        self.room(1)?;
        let db = self.register_msi(unsafe { MsiGetActiveDatabase(install) }, false)?;
        let (view, row) = self.abi2_one(db, GUARD_TEMPLATE_QUERY)?;
        let length = unsafe { MsiRecordDataSize(row, 1) } as usize;
        if !(69..=MAX_IMAGE).contains(&length) {
            return Err(LifecycleError::Invalid);
        }
        // Only a fixed-size tail and chunk are retained, never a whole PE copy.
        let mut tail = [0u8; TRAILER];
        let mut chunk = [0u8; 16384];
        let mut offset = 0;
        while offset < length {
            let count = chunk.len().min(length - offset);
            self.abi2_stream_read(row, &mut chunk[..count])?;
            let start = (length.saturating_sub(TRAILER)).max(offset);
            if start < offset + count {
                let destination = start - length.saturating_sub(TRAILER);
                tail[destination..destination + offset + count - start]
                    .copy_from_slice(&chunk[start - offset..count]);
            }
            offset += count;
        }
        self.abi2_stream_end(row)?;
        self.abi2_end(view, row)?;
        let format = if tail.ends_with(carrier::FOOTER) {
            if length < carrier::MIN_IMAGE_BYTES
                || tail[..16] != *SEAL_MAGIC
                || tail[16..48] != [0; 32]
                || tail[48..112] != carrier::descriptor()
                || tail[112..116] != 64u32.to_le_bytes()
            {
                return Err(LifecycleError::Invalid);
            }
            let (view, row) = self.abi2_one(db, payload_manifest::QUERY)?;
            let size = unsafe { MsiRecordDataSize(row, 1) } as usize;
            if !(payload_manifest::HEADER_BYTES..=payload_manifest::MAX_BYTES).contains(&size) {
                return Err(LifecycleError::Invalid);
            }
            let mut bytes = vec![0; size];
            self.abi2_stream_read(row, &mut bytes)?;
            self.abi2_stream_end(row)?;
            self.abi2_end(view, row)?;
            let envelope = payload_manifest::parse(&bytes)?;
            let (product, family, version) = self.abi2_properties(db)?;
            let package = self.abi2_package(db)?;
            if family != INSTALL_FAMILY
                || product != envelope.payload().product
                || version != envelope.payload().version
                || package != envelope.package()
            {
                return Err(LifecycleError::Invalid);
            }
            PackageFormat::Abi2(ObservedPackage::new(&product, &package, &version)?)
        } else if tail.ends_with(manifest::FOOTER) {
            PackageFormat::Legacy
        } else {
            return Err(LifecycleError::Invalid);
        };
        self.close_msi(db)?;
        self.time()?;
        Ok(format)
    }
    fn abi2_text(&self, row: MSIHANDLE, field: u32) -> Result<String> {
        self.time()?;
        let mut buffer = [0u16; 64];
        let mut count = buffer.len() as u32;
        let code = unsafe {
            MsiRecordGetStringW(
                row,
                field,
                Some(PWSTR(buffer.as_mut_ptr())),
                Some(&mut count),
            )
        };
        if code != 0 || count == 0 || count as usize >= buffer.len() || buffer[count as usize] != 0
        {
            return Err(LifecycleError::Invalid);
        }
        String::from_utf16(&buffer[..count as usize]).map_err(|_| LifecycleError::Invalid)
    }
    fn abi2_properties(&mut self, db: MSIHANDLE) -> Result<(String, String, String)> {
        let view = self.abi2_view(db, PROPERTIES)?;
        let mut values: [Option<String>; 3] = [None, None, None];
        for _ in 0..3 {
            let row = self.abi2_fetch(view)?.ok_or(LifecycleError::Invalid)?;
            let key = self.abi2_text(row, 1)?;
            let value = self.abi2_text(row, 2)?;
            let (index, value) = match key.as_str() {
                "ProductCode" => (0, unbrace(&value)?),
                "UpgradeCode" => (1, unbrace(&value)?),
                "ProductVersion" => (2, value),
                _ => return Err(LifecycleError::Invalid),
            };
            if values[index].replace(value).is_some() {
                return Err(LifecycleError::Invalid);
            }
            self.close_msi(row)?;
        }
        if self.abi2_fetch(view)?.is_some() {
            return Err(LifecycleError::Invalid);
        }
        self.close_msi(view)?;
        let [a, b, c] = values;
        Ok((
            a.ok_or(LifecycleError::Invalid)?,
            b.ok_or(LifecycleError::Invalid)?,
            c.ok_or(LifecycleError::Invalid)?,
        ))
    }
    fn abi2_package(&mut self, db: MSIHANDLE) -> Result<String> {
        self.room(1)?;
        let mut summary = MSIHANDLE(0);
        let code = unsafe { MsiGetSummaryInformationW(db, PCWSTR::null(), 0, &mut summary) };
        let summary = self.abi2_acquired(code, summary)?;
        let (mut kind, mut integer) = (0, 0);
        // Even a string Summary property requires non-null output storage.
        let mut file_time = windows::Win32::Foundation::FILETIME::default();
        let mut buffer = [0u16; 39];
        let mut count = buffer.len() as u32; // capacity INCLUDES the terminating NUL.
        let code = unsafe {
            MsiSummaryInfoGetPropertyW(
                summary,
                9,
                &mut kind,
                &mut integer,
                Some(&mut file_time),
                Some(PWSTR(buffer.as_mut_ptr())),
                Some(&mut count),
            )
        };
        self.time()?;
        if code != 0 || kind != 30 || count != 38 || buffer[38] != 0 {
            return Err(LifecycleError::Invalid);
        }
        let package =
            unbrace(&String::from_utf16(&buffer[..38]).map_err(|_| LifecycleError::Invalid)?)?;
        self.close_msi(summary)?;
        Ok(package)
    }
}
fn unbrace(value: &str) -> Result<String> {
    let value = value
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .filter(|s| manifest::uuid(s))
        .ok_or(LifecycleError::Invalid)?;
    Ok(value.into())
}
