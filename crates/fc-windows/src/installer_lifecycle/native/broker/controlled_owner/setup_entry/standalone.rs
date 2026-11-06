//! One distribution: normal helper + unchanged signed official archive.
//! The sealed native source retains all MSI admission and owner checks.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Cursor, Read, Seek, SeekFrom, Write},
    os::windows::fs::OpenOptionsExt,
};
const MAGIC: &[u8; 16] = b"FC-SFX-PACK\0v4\0\0";
pub(super) const LEGACY_MAGIC: &[u8; 16] = b"FC-SFX-PACK\0v1\0\0";
const FOOTER: u64 = 64;
const NAMES: [&str; 6] = [
    "Setup.exe",
    "FutureClassroom.msi",
    "LICENSE",
    "THIRD-PARTY-NOTICES.txt",
    "installer-variants.bin",
    "manifest.json",
];
const LIMITS: [u64; 6] = [32 << 20, 128 << 20, 2 << 20, 4 << 20, 32 << 20, 32768];
fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(|e| {
        LifecycleError::Native(HRESULT::from_win32(e.raw_os_error().unwrap_or(1) as u32))
    })
}
fn invalid<T>(_: T) -> LifecycleError {
    LifecycleError::Invalid
}

pub(super) struct Package {
    image: File,
    offset: u64,
    archive: Vec<u8>,
    legacy: Option<Vec<u64>>,
}
impl Package {
    pub(super) fn unified(&self) -> bool {
        self.legacy.is_none()
    }
    fn discover() -> Result<Option<Self>> {
        Self::open(&io(std::env::current_exe())?)
    }
    pub(super) fn open(path: &std::path::Path) -> Result<Option<Self>> {
        let mut image = io(OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path))?;
        let size = io(image.metadata())?.len();
        if size < FOOTER + 512 {
            return Ok(None);
        }
        io(image.seek(SeekFrom::End(-(FOOTER as i64))))?;
        let mut footer = [0; 64];
        io(image.read_exact(&mut footer))?;
        if &footer[..16] != MAGIC {
            // Only the local, same-version maintenance fallback created after
            // an old coordinator's settled native upgrade may use this format.
            if size < 600 || !super::maintenance::local_legacy_cache(path)? {
                return Ok(None);
            }
            io(image.seek(SeekFrom::End(-88)))?;
            let mut old = [0; 88];
            io(image.read_exact(&mut old))?;
            if &old[..16] != LEGACY_MAGIC {
                return Ok(None);
            }
            let offset = u64::from_le_bytes(old[16..24].try_into().unwrap());
            let lengths: Vec<_> = (0..4)
                .map(|i| u64::from_le_bytes(old[24 + i * 8..32 + i * 8].try_into().unwrap()))
                .collect();
            if !(512..=32 << 20).contains(&offset)
                || lengths
                    .iter()
                    .zip(LIMITS)
                    .any(|(&n, max)| n == 0 || n > max)
                || offset + lengths.iter().sum::<u64>() + 88 != size
            {
                return Err(LifecycleError::Invalid);
            }
            io(image.seek(SeekFrom::Start(offset)))?;
            let mut digest = Sha256::new();
            digest.update(&old[..56]);
            let mut buffer = [0; 65536];
            let mut remaining = size - offset - 88;
            while remaining > 0 {
                let count = remaining.min(buffer.len() as u64) as usize;
                io(image.read_exact(&mut buffer[..count]))?;
                digest.update(&buffer[..count]);
                remaining -= count as u64;
            }
            if digest.finalize().as_slice() != &old[56..] {
                return Err(LifecycleError::Invalid);
            }
            return Ok(Some(Self {
                image,
                offset,
                archive: Vec::new(),
                legacy: Some(lengths),
            }));
        }
        let offset = u64::from_le_bytes(footer[16..24].try_into().unwrap());
        let length = u64::from_le_bytes(footer[24..32].try_into().unwrap());
        if !(512..=32 << 20).contains(&offset)
            || !(1..=128 << 20).contains(&length)
            || offset
                .checked_add(length)
                .and_then(|v| v.checked_add(FOOTER))
                != Some(size)
        {
            return Err(LifecycleError::Invalid);
        }
        io(image.seek(SeekFrom::Start(offset)))?;
        let mut archive = vec![0; length as usize];
        io(image.read_exact(&mut archive))?;
        if Sha256::digest(&archive).as_slice() != &footer[32..] {
            return Err(LifecycleError::Invalid);
        }
        Ok(Some(Self {
            image,
            offset,
            archive,
            legacy: None,
        }))
    }
    pub(super) fn original(&mut self) -> Result<Vec<u8>> {
        io(self.image.seek(SeekFrom::Start(0)))?;
        let mut bytes = Vec::with_capacity(io(self.image.metadata())?.len() as usize);
        io(self.image.read_to_end(&mut bytes))?;
        Ok(bytes)
    }
    pub(super) fn members(&mut self) -> Result<Vec<(&'static str, Vec<u8>)>> {
        if let Some(lengths) = &self.legacy {
            io(self.image.seek(SeekFrom::Start(self.offset)))?;
            let mut members = Vec::new();
            for (name, length) in NAMES.iter().zip(lengths) {
                let mut bytes = vec![0; *length as usize];
                io(self.image.read_exact(&mut bytes))?;
                members.push((*name, bytes));
            }
            self.validate_helper(&members)?;
            return Ok(members);
        }
        let mut outer = zip::ZipArchive::new(Cursor::new(&self.archive)).map_err(invalid)?;
        if !(4..=36).contains(&outer.len()) {
            return Err(LifecycleError::Invalid);
        }
        let mut payload = outer.by_name("payload.zip").map_err(invalid)?;
        if payload.size() > 128 << 20 || payload.compression() != zip::CompressionMethod::Stored {
            return Err(LifecycleError::Invalid);
        }
        let mut bytes = Vec::with_capacity(payload.size() as usize);
        let declared = payload.size();
        io((&mut payload).take(declared + 1).read_to_end(&mut bytes))?;
        if bytes.len() as u64 != declared {
            return Err(LifecycleError::Invalid);
        }
        drop(payload);
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
        if !(5..=6).contains(&zip.len()) {
            return Err(LifecycleError::Invalid);
        }
        let mut result = Vec::new();
        let mut total = 0u64;
        for index in 0..zip.len() {
            let mut file = zip.by_index(index).map_err(invalid)?;
            let i = NAMES
                .iter()
                .position(|name| *name == file.name())
                .ok_or(LifecycleError::Invalid)?;
            if result.iter().any(|(name, _)| *name == NAMES[i])
                || file.size() == 0
                || file.size() > LIMITS[i]
                || file.is_dir()
            {
                return Err(LifecycleError::Invalid);
            }
            total += file.size();
            if total > 128 << 20 {
                return Err(LifecycleError::Invalid);
            }
            let mut data = Vec::with_capacity(file.size() as usize);
            let declared = file.size();
            io((&mut file).take(declared + 1).read_to_end(&mut data))?;
            if data.len() as u64 != declared {
                return Err(LifecycleError::Invalid);
            }
            result.push((NAMES[i], data));
        }
        if NAMES[..5]
            .iter()
            .any(|name| !result.iter().any(|(n, _)| n == name))
        {
            return Err(LifecycleError::Invalid);
        }
        self.validate_helper(&result)?;
        Ok(result)
    }
    fn validate_helper(&mut self, result: &[(&str, Vec<u8>)]) -> Result<()> {
        let get = |name| {
            result
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, b)| b.as_slice())
                .ok_or(LifecycleError::Invalid)
        };
        let carrier = crate::installer_lifecycle::broker::carrier::parse(get("Setup.exe")?)?;
        if Sha256::digest(get("FutureClassroom.msi")?).as_slice() != carrier.digest_format_value() {
            return Err(LifecycleError::Invalid);
        }
        io(self.image.seek(SeekFrom::Start(0)))?;
        let mut helper = vec![0; self.offset as usize];
        io(self.image.read_exact(&mut helper))?;
        if helper != carrier.normal_helper() {
            return Err(LifecycleError::Invalid);
        }
        Ok(())
    }
    pub(super) fn extract(&mut self) -> Result<Extracted> {
        use fc_ports::RandomSource;
        let members = self.members()?;
        let identity = identity()?;
        let mut nonce = [0; 16];
        crate::SystemRandom.fill(&mut nonce)?;
        let suffix: String = nonce.iter().map(|v| format!("{v:02x}")).collect();
        let root = identity
            .local
            .join(format!("FutureClassroom-Setup-{suffix}"));
        with_security(&identity.sid, true, |sa| unsafe {
            CreateDirectoryW(&native_path(&root)?, Some(sa))
        })?;
        let mut extracted = Extracted {
            root,
            pins: Vec::new(),
            directory: None,
        };
        extracted.directory = Some(io(OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(&extracted.root))?);
        let mut members = members;
        if self.legacy.is_none() {
            members.push(("OriginalInstaller.exe", self.original()?));
        }
        for (name, bytes) in members {
            let mut file = io(OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(0)
                .open(extracted.root.join(name)))?;
            io(file.write_all(&bytes))?;
            io(file.sync_all())?;
        }
        if self.legacy.is_none() {
            super::maintenance::materialize_source(&extracted.root)?;
        }
        for name in NAMES
            .iter()
            .chain(std::iter::once(&"OriginalInstaller.exe"))
        {
            let path = extracted.root.join(name);
            if path.exists() {
                extracted.pins.push(io(OpenOptions::new()
                    .read(true)
                    .share_mode(FILE_SHARE_READ.0)
                    .open(path))?);
            }
        }
        Ok(extracted)
    }
}
pub(crate) fn archive() -> Result<Option<Vec<u8>>> {
    Ok(Package::discover()?
        .filter(|p| p.legacy.is_none())
        .map(|p| p.archive))
}
pub(super) struct Extracted {
    pub(super) root: PathBuf,
    pins: Vec<File>,
    directory: Option<File>,
}
impl Extracted {
    pub(super) fn cleanup(&mut self) -> Result<()> {
        self.pins.clear();
        self.directory.take();
        if !self.root.exists() {
            return Ok(());
        }
        for name in NAMES
            .iter()
            .chain(std::iter::once(&"OriginalInstaller.exe"))
        {
            match std::fs::remove_file(self.root.join(name)) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return io(Err(e)),
            }
        }
        io(std::fs::remove_dir(&self.root))
    }
}
impl Drop for Extracted {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

pub(crate) fn dispatch(args: &[String]) -> Option<Result<u32>> {
    let package = match Package::discover() {
        Ok(None) => return None,
        Ok(Some(v)) => v,
        Err(e) => return Some(Err(e)),
    };
    if args.is_empty() || matches!(args, [one] if one == "uninstall" || one == "maintenance") {
        return Some(super::setup_ui::show(
            package,
            matches!(args, [one] if one == "uninstall"),
        ));
    }
    if let [mode, force, data] = args {
        if matches!(
            mode.as_str(),
            "maintain-install" | "maintain-repair" | "maintain-uninstall"
        ) && matches!(force.as_str(), "normal" | "force")
            && matches!(data.as_str(), "keep" | "delete")
        {
            return Some(super::maintenance::run(package, mode, data == "delete"));
        }
    }
    match args {
        [one] if one == "install" || one == "repair" => {
            Some(super::maintenance::run(package, "maintain-repair", false))
        }
        _ => Some(Err(LifecycleError::Invalid)),
    }
}
