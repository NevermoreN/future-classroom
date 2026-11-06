//! Presentation-only SFX envelope. The extracted sealed Setup retains every
//! original source, ownership, MSI and upgrade admission check.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::fs::OpenOptionsExt,
};

pub(super) const MAGIC: &[u8; 16] = b"FC-SFX-PACK\0v1\0\0";
const FOOTER: u64 = 88;
const MAGIC_V2: &[u8; 16] = b"FC-SFX-PACK\0v2\0\0";
const MAGIC_V3: &[u8; 16] = b"FC-SFX-PACK\0v3\0\0";
const NAMES: [&str; 8] = [
    "Setup.exe",
    "FutureClassroom.msi",
    "LICENSE",
    "THIRD-PARTY-NOTICES.txt",
    "previous/Setup.exe",
    "previous/FutureClassroom.msi",
    "legacy/Setup.exe",
    "legacy/FutureClassroom.msi",
];
// Each original maintenance pair has its own fixed bounds; the executable
// prefix is capped at 32 MiB separately. Notices include the complete bundled dependency licenses.
const LIMITS: [u64; 8] = [
    32 << 20,
    128 << 20,
    2 << 20,
    4 << 20,
    32 << 20,
    128 << 20,
    32 << 20,
    128 << 20,
];
fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(|e| {
        LifecycleError::Native(HRESULT::from_win32(e.raw_os_error().unwrap_or(1) as u32))
    })
}

pub(super) struct Package {
    image: File,
    offset: u64,
    lengths: Vec<u64>,
    digest: [u8; 32],
    header: Vec<u8>,
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
        let mut tail = [0; 16];
        if size >= 104 {
            io(image.seek(SeekFrom::End(-104)))?;
            io(image.read_exact(&mut tail))?;
        }
        let mut third = [0; 16];
        if size >= 120 {
            io(image.seek(SeekFrom::End(-120)))?;
            io(image.read_exact(&mut third))?;
        }
        let footer_size = if &third == MAGIC_V3 {
            120
        } else if &tail == MAGIC_V2 {
            104
        } else {
            FOOTER
        };
        io(image.seek(SeekFrom::End(-(footer_size as i64))))?;
        let mut footer = vec![0; footer_size as usize];
        io(image.read_exact(&mut footer))?;
        if &footer[..16]
            != if footer_size == 120 {
                MAGIC_V3
            } else if footer_size == 104 {
                MAGIC_V2
            } else {
                MAGIC
            }
        {
            return Ok(None);
        }
        let number = |offset| u64::from_le_bytes(footer[offset..offset + 8].try_into().unwrap());
        let offset = number(16);
        let count = if footer_size == 120 {
            8
        } else if footer_size == 104 {
            6
        } else {
            4
        };
        let lengths: Vec<_> = (0..count).map(|i| number(24 + 8 * i)).collect();
        let mut end = offset;
        if !(512..=32 << 20).contains(&offset) {
            return Err(LifecycleError::Invalid);
        }
        for (&length, limit) in lengths.iter().zip(LIMITS) {
            if length == 0 || length > limit {
                return Err(LifecycleError::Invalid);
            }
            end = end.checked_add(length).ok_or(LifecycleError::Invalid)?;
        }
        if end.checked_add(footer_size) != Some(size) {
            return Err(LifecycleError::Invalid);
        }
        Ok(Some(Self {
            image,
            offset,
            lengths,
            digest: footer[footer.len() - 32..].try_into().unwrap(),
            header: footer[..footer.len() - 32].to_vec(),
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
            previous: self.lengths.len() >= 6,
            legacy: self.lengths.len() == 8,
        };
        if extracted.previous {
            io(std::fs::create_dir(extracted.root.join("previous")))?;
        }
        if extracted.legacy {
            io(std::fs::create_dir(extracted.root.join("legacy")))?;
        }
        extracted.directory = Some(io(OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(&extracted.root))?);
        io(self.image.seek(SeekFrom::Start(self.offset)))?;
        let mut digest = Sha256::new();
        digest.update(&self.header);
        let mut buffer = [0u8; 65536];
        for (index, (name, &length)) in NAMES.into_iter().zip(&self.lengths).enumerate() {
            let path = extracted.root.join(name);
            let mut output = io(OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(0)
                .open(&path))?;
            let mut remaining = length;
            let mut member_digest = Sha256::new();
            while remaining > 0 {
                let count = remaining.min(buffer.len() as u64) as usize;
                io(self.image.read_exact(&mut buffer[..count]))?;
                digest.update(&buffer[..count]);
                member_digest.update(&buffer[..count]);
                io(output.write_all(&buffer[..count]))?;
                remaining -= count as u64;
            }
            io(output.sync_all())?;
            drop(output);
            if index >= 4 {
                let expected = match index {
                    4 => super::previous_source::SETUP_SHA,
                    5 => super::previous_source::MSI_SHA,
                    6 => super::previous_source::LEGACY_SETUP_SHA,
                    _ => super::previous_source::LEGACY_MSI_SHA,
                };
                if <[u8; 32]>::from(member_digest.finalize()) != expected {
                    return Err(LifecycleError::Invalid);
                }
            }
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
    pub(super) root: PathBuf,
    pins: Vec<File>,
    directory: Option<File>,
    pub(super) previous: bool,
    pub(super) legacy: bool,
}
impl Extracted {
    pub(super) fn cleanup(&mut self) -> Result<()> {
        self.pins.clear();
        self.directory.take();
        if !self.root.exists() {
            return Ok(());
        }
        // Delete only the fixed files we created, never recurse through an
        // unexpected object that an unrelated process may have placed here.
        for name in NAMES.into_iter().take(if self.legacy {
            8
        } else if self.previous {
            6
        } else {
            4
        }) {
            match std::fs::remove_file(self.root.join(name)) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => {
                    io::<()>(Err(e))?;
                }
            }
        }
        if self.legacy {
            io(std::fs::remove_dir(self.root.join("legacy")))?;
        }
        if self.previous {
            io(std::fs::remove_dir(self.root.join("previous")))?;
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
