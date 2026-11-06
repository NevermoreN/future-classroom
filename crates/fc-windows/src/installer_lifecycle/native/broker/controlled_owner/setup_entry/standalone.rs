//! Presentation-only SFX envelope. The extracted sealed Setup retains every
//! original source, ownership, MSI and upgrade admission check.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::fs::OpenOptionsExt,
    process::{Child, Command},
};

pub(super) const MAGIC: &[u8; 16] = b"FC-SFX-PACK\0v1\0\0";
const FOOTER: u64 = 88;
const NAMES: [&str; 4] = [
    "Setup.exe",
    "FutureClassroom.msi",
    "LICENSE",
    "THIRD-PARTY-NOTICES.txt",
];
// Fixed members total at most 166 MiB; the executable prefix is capped at
// 32 MiB separately. Notices include the complete bundled dependency licenses.
const LIMITS: [u64; 4] = [32 << 20, 128 << 20, 2 << 20, 4 << 20];
fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(|e| {
        LifecycleError::Native(HRESULT::from_win32(e.raw_os_error().unwrap_or(1) as u32))
    })
}

pub(super) struct Package {
    image: File,
    offset: u64,
    lengths: [u64; 4],
    digest: [u8; 32],
    header: [u8; 56],
}
impl Package {
    fn discover() -> Result<Option<Self>> {
        let path = io(std::env::current_exe())?;
        let mut image = io(OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(path))?;
        let size = io(image.metadata())?.len();
        if size < FOOTER {
            return Ok(None);
        }
        io(image.seek(SeekFrom::End(-(FOOTER as i64))))?;
        let mut footer = [0; FOOTER as usize];
        io(image.read_exact(&mut footer))?;
        if &footer[..16] != MAGIC {
            return Ok(None);
        }
        let number = |offset| u64::from_le_bytes(footer[offset..offset + 8].try_into().unwrap());
        let offset = number(16);
        let lengths = [number(24), number(32), number(40), number(48)];
        let mut end = offset;
        if !(512..=32 << 20).contains(&offset) {
            return Err(LifecycleError::Invalid);
        }
        for (length, limit) in lengths.into_iter().zip(LIMITS) {
            if length == 0 || length > limit {
                return Err(LifecycleError::Invalid);
            }
            end = end.checked_add(length).ok_or(LifecycleError::Invalid)?;
        }
        if end.checked_add(FOOTER) != Some(size) {
            return Err(LifecycleError::Invalid);
        }
        Ok(Some(Self {
            image,
            offset,
            lengths,
            digest: footer[56..].try_into().unwrap(),
            header: footer[..56].try_into().unwrap(),
        }))
    }
    pub(super) fn extract(&mut self) -> Result<Extracted> {
        use fc_ports::RandomSource;
        let identity = identity()?;
        let mut nonce = [0u8; 16];
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
        io(self.image.seek(SeekFrom::Start(self.offset)))?;
        let mut digest = Sha256::new();
        digest.update(self.header);
        let mut buffer = [0u8; 65536];
        for (name, length) in NAMES.into_iter().zip(self.lengths) {
            let path = extracted.root.join(name);
            let mut output = io(OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(0)
                .open(&path))?;
            let mut remaining = length;
            while remaining > 0 {
                let count = remaining.min(buffer.len() as u64) as usize;
                io(self.image.read_exact(&mut buffer[..count]))?;
                digest.update(&buffer[..count]);
                io(output.write_all(&buffer[..count]))?;
                remaining -= count as u64;
            }
            io(output.sync_all())?;
            drop(output);
            extracted.pins.push(io(OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ.0)
                .open(path))?);
        }
        if digest.finalize().as_slice() != self.digest {
            return Err(LifecycleError::Invalid);
        }
        Ok(extracted)
    }
}
pub(super) struct Extracted {
    root: PathBuf,
    pins: Vec<File>,
    directory: Option<File>,
}
impl Extracted {
    pub(super) fn start(&self, operation: &str) -> Result<Child> {
        io(Command::new(self.root.join("Setup.exe"))
            .arg(operation)
            .current_dir(&self.root)
            .spawn())
    }
    pub(super) fn cleanup(&mut self) -> Result<()> {
        self.pins.clear();
        self.directory.take();
        if !self.root.exists() {
            return Ok(());
        }
        // Delete only the four fixed files we created, never recurse through an
        // unexpected object that an unrelated process may have placed here.
        for name in NAMES {
            match std::fs::remove_file(self.root.join(name)) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => {
                    io::<()>(Err(e))?;
                }
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
    if args.is_empty() {
        return Some(super::setup_ui::show(package));
    }
    let operation = match args {
        [s] if matches!(s.as_str(), "install" | "repair" | "uninstall") => s.as_str(),
        _ => return Some(Err(LifecycleError::Invalid)),
    };
    Some((|| {
        let mut package = package;
        let mut extracted = package.extract()?;
        let mut child = extracted.start(operation)?;
        let status = io(child.wait())?;
        extracted.cleanup()?;
        Ok(status.code().ok_or(LifecycleError::Unsettled)? as u32)
    })())
}
