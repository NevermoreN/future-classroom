//! Native readonly logical signatures; all SQL and schemas come from fixed code.
use super::*;
struct Hash {
    algorithm: BCRYPT_ALG_HANDLE,
    hash: BCRYPT_HASH_HANDLE,
}
impl Reader {
    fn hash_new(&mut self) -> Result<Hash> {
        self.room(2)?;
        let mut a = BCRYPT_ALG_HANDLE::default();
        let rc = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut a,
                BCRYPT_SHA256_ALGORITHM,
                None,
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
        };
        if !a.0.is_null() {
            self.resources.add(HANDLE(a.0), Role::Algorithm);
        }
        rc.ok()?;
        let mut h = BCRYPT_HASH_HANDLE::default();
        let rc = unsafe { BCryptCreateHash(a, &mut h, None, None, 0) };
        if !h.0.is_null() {
            self.resources.add(HANDLE(h.0), Role::Hash);
        }
        rc.ok()?;
        Ok(Hash {
            algorithm: a,
            hash: h,
        })
    }
    fn hash_add(&self, h: &Hash, b: &[u8]) -> Result<()> {
        self.time()?;
        unsafe {
            BCryptHashData(h.hash, b, 0).ok()?;
        }
        Ok(())
    }
    fn hash_end(&mut self, h: Hash) -> Result<[u8; 32]> {
        let mut b = [0; 32];
        unsafe {
            BCryptFinishHash(h.hash, &mut b, 0).ok()?;
        }
        let i = self
            .resources
            .handles
            .iter()
            .position(|v| v.role == Role::Hash && v.handle == HANDLE(h.hash.0))
            .ok_or(LifecycleError::Invalid)?;
        self.resources.close(i)?;
        let i = self
            .resources
            .handles
            .iter()
            .position(|v| v.role == Role::Algorithm && v.handle == HANDLE(h.algorithm.0))
            .ok_or(LifecycleError::Invalid)?;
        self.resources.close(i)?;
        self.time()?;
        Ok(b)
    }
    fn view(&mut self, db: MSIHANDLE, query: &'static str) -> Result<MSIHANDLE> {
        self.room(1)?;
        let mut v = MSIHANDLE(0);
        let rc = unsafe { MsiDatabaseOpenViewW(db, &HSTRING::from(query), &mut v) };
        let v = self.acquired(rc, v)?;
        let rc = unsafe { MsiViewExecute(v, MSIHANDLE(0)) };
        if rc == 0 {
            self.msi
                .iter_mut()
                .find(|i| i.handle == v)
                .ok_or(LifecycleError::Invalid)?
                .view_open = true;
        }
        self.time()?;
        if rc != 0 {
            return Err(LifecycleError::Invalid);
        }
        Ok(v)
    }
    fn fetch(&mut self, v: MSIHANDLE) -> Result<Option<MSIHANDLE>> {
        self.room(1)?;
        let mut row = MSIHANDLE(0);
        let rc = unsafe { MsiViewFetch(v, &mut row) };
        if rc == ERROR_NO_MORE_ITEMS.0 && row.0 == 0 {
            self.time()?;
            return Ok(None);
        }
        self.acquired(rc, row).map(Some)
    }
    fn text(&self, row: MSIHANDLE, field: u32) -> Result<String> {
        self.time()?;
        let mut b = [0u16; 4097];
        let mut n = b.len() as u32;
        let rc =
            unsafe { MsiRecordGetStringW(row, field, Some(PWSTR(b.as_mut_ptr())), Some(&mut n)) };
        self.time()?;
        if rc != 0 || n as usize >= b.len() || b[n as usize] != 0 {
            return Err(LifecycleError::Invalid);
        }
        String::from_utf16(&b[..n as usize]).map_err(|_| LifecycleError::Invalid)
    }
    fn columns(&mut self, v: MSIHANDLE, kind: MSICOLINFO) -> Result<Vec<String>> {
        self.room(1)?;
        let mut row = MSIHANDLE(0);
        let rc = unsafe { MsiViewGetColumnInfo(v, kind, &mut row) };
        let row = self.acquired(rc, row)?;
        let n = unsafe { MsiRecordGetFieldCount(row) };
        if n == 0 || n > 32 {
            return Err(LifecycleError::Invalid);
        }
        let mut result = Vec::with_capacity(n as usize);
        for i in 1..=n {
            result.push(self.text(row, i)?);
        }
        self.close_msi(row)?;
        Ok(result)
    }
    fn tables(&mut self, db: MSIHANDLE) -> Result<Vec<&'static schema::Table>> {
        let v = self.view(db, "SELECT `Name` FROM `_Tables`")?;
        let mut names = BTreeSet::new();
        while let Some(row) = self.fetch(v)? {
            let name = self.text(row, 1)?;
            if schema::table(&name).is_none()
                || !names.insert(name)
                || names.len() > schema::TABLES.len()
            {
                return Err(LifecycleError::Invalid);
            }
            self.close_msi(row)?;
        }
        self.close_msi(v)?;
        if names.len() != schema::TABLES.len() {
            return Err(LifecycleError::Invalid);
        }
        names
            .into_iter()
            .map(|n| schema::table(&n).ok_or(LifecycleError::Invalid))
            .collect()
    }
    pub(super) fn database(&mut self, path: &Path, expected_package: &str) -> Result<[u8; 32]> {
        self.room(1)?;
        let mut db = MSIHANDLE(0);
        let rc = unsafe { MsiOpenDatabaseW(&native_path(path)?, MSIDBOPEN_READONLY, &mut db) };
        let db = self.acquired(rc, db)?;
        self.package(db, expected_package)?;
        let tables = self.tables(db)?;
        let all = self.hash_new()?;
        self.hash_add(&all, b"FC-SAME-SOURCE-DB-v1\0")?;
        for table in tables {
            // Reserve the WHOLE next table's bounded receipts before opening a
            // view. Checkpoint only between fully returned record/view sets.
            if self.closed.len() + self.msi.len() + MAX_ROWS + 8 > RECEIPTS {
                self.checkpoint(db)?;
            }
            let view = self.view(db, table.query)?;
            let names = self.columns(view, MSICOLINFO_NAMES)?;
            let types = self.columns(view, MSICOLINFO_TYPES)?;
            if names.len() != table.columns.len()
                || types.len() != table.columns.len()
                || names.iter().zip(table.columns).any(|(a, (b, _))| a != b)
                || types.iter().zip(table.columns).any(|(a, (_, b))| a != b)
            {
                return Err(LifecycleError::Invalid);
            }
            let mut rows = Vec::with_capacity(MAX_ROWS);
            while let Some(row) = self.fetch(view)? {
                if rows.len() >= MAX_ROWS {
                    return Err(LifecycleError::Invalid);
                }
                let hash = self.hash_new()?;
                for (i, (_, kind)) in table.columns.iter().enumerate() {
                    let field = i as u32 + 1;
                    let null = unsafe { MsiRecordIsNull(row, field) }.as_bool();
                    self.hash_add(&hash, &[u8::from(null)])?;
                    if null {
                        continue;
                    }
                    match kind.as_bytes()[0].to_ascii_lowercase() {
                        b'i' => {
                            let value = unsafe { MsiRecordGetInteger(row, field) };
                            if value == i32::MIN {
                                return Err(LifecycleError::Invalid);
                            }
                            self.hash_add(&hash, &value.to_le_bytes())?;
                        }
                        b's' | b'l' => {
                            let value = self.text(row, field)?;
                            self.hash_add(&hash, &(value.len() as u32).to_le_bytes())?;
                            self.hash_add(&hash, value.as_bytes())?;
                        }
                        b'v' => {
                            let size = unsafe { MsiRecordDataSize(row, field) };
                            if u64::from(size) > LIMIT {
                                return Err(LifecycleError::Invalid);
                            }
                            self.hash_add(&hash, &size.to_le_bytes())?;
                            let mut left = size as usize;
                            let mut buffer = [0; 16384];
                            while left != 0 {
                                let n = left.min(buffer.len());
                                let mut got = n as u32;
                                let rc = unsafe {
                                    MsiRecordReadStream(
                                        row,
                                        field,
                                        Some(PSTR(buffer.as_mut_ptr())),
                                        &mut got,
                                    )
                                };
                                if rc != 0 || got as usize != n {
                                    return Err(LifecycleError::Invalid);
                                }
                                self.hash_add(&hash, &buffer[..n])?;
                                left -= n;
                            }
                            let mut end = 1;
                            let rc = unsafe {
                                MsiRecordReadStream(
                                    row,
                                    field,
                                    Some(PSTR(buffer.as_mut_ptr())),
                                    &mut end,
                                )
                            };
                            if rc != 0 || end != 0 {
                                return Err(LifecycleError::Invalid);
                            }
                        }
                        _ => return Err(LifecycleError::Invalid),
                    }
                }
                rows.push(self.hash_end(hash)?);
                self.close_msi(row)?;
            }
            self.close_msi(view)?;
            rows.sort_unstable();
            self.hash_add(&all, &(table.name.len() as u32).to_le_bytes())?;
            self.hash_add(&all, table.name.as_bytes())?;
            for (name, kind) in table.columns {
                self.hash_add(&all, &(name.len() as u32).to_le_bytes())?;
                self.hash_add(&all, name.as_bytes())?;
                self.hash_add(&all, &(kind.len() as u32).to_le_bytes())?;
                self.hash_add(&all, kind.as_bytes())?;
            }
            self.hash_add(&all, &(rows.len() as u32).to_le_bytes())?;
            for row in rows {
                self.hash_add(&all, &row)?;
            }
        }
        let signature = self.hash_end(all)?;
        self.close_msi(db)?;
        Ok(signature)
    }
    fn package(&mut self, db: MSIHANDLE, expected: &str) -> Result<()> {
        self.room(1)?;
        let mut summary = MSIHANDLE(0);
        let rc = unsafe { MsiGetSummaryInformationW(db, PCWSTR::null(), 0, &mut summary) };
        let summary = self.acquired(rc, summary)?;
        let (mut kind, mut value) = (0, 0);
        // Even a string Summary property requires non-null output storage.
        let mut file_time = windows::Win32::Foundation::FILETIME::default();
        let mut b = [0u16; 39];
        let mut n = b.len() as u32;
        let rc = unsafe {
            MsiSummaryInfoGetPropertyW(
                summary,
                9,
                &mut kind,
                &mut value,
                Some(&mut file_time),
                Some(PWSTR(b.as_mut_ptr())),
                Some(&mut n),
            )
        };
        self.time()?;
        if rc != 0
            || kind != 30
            || n != 38
            || b[38] != 0
            || format::product_code(
                &String::from_utf16(&b[..38]).map_err(|_| LifecycleError::Invalid)?,
            )
            .as_deref()
                != Some(expected)
        {
            return Err(LifecycleError::Invalid);
        }
        self.close_msi(summary)
    }
}
