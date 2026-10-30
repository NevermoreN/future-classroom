//! Controlled, handle-pinned canonical voice objects. Call only on the shared
//! bounded file worker, after installation ACL read-back and instance locking.
use crate::backend::Backend;
use fc_domain::voice::*;
use fc_materials::{
    office::{ContainerError, ValidationControl},
    validate_wav_header,
    validation::hash_file,
};
use fc_ports::voice::{
    prepare_voice_upload, PreparedVoiceUpload, VoicePreparation, VoiceRepositoryError,
};
use fc_windows::{
    audio::VerifiedWave,
    materials::files::{ControlledFiles, LockedFile},
};
use std::{
    collections::BTreeSet,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid controlled voice object",
    )
}
fn canceled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "voice preparation canceled")
}
fn file_error(_: impl std::fmt::Debug) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "controlled voice file unavailable",
    )
}
fn preparation_error(error: io::Error, control: &Control<'_>) -> VoiceRepositoryError {
    if !(control.authorize)() {
        return VoiceRepositoryError::Domain(VoiceError::Permission(
            fc_domain::auth::PermissionError::StaleAuthorization,
        ));
    }
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => {
            VoiceRepositoryError::Domain(VoiceError::InvalidWav)
        }
        io::ErrorKind::Interrupted => VoiceRepositoryError::Domain(VoiceError::Expired),
        _ => VoiceRepositoryError::Unavailable,
    }
}
struct Control<'a> {
    authorize: &'a dyn Fn() -> bool,
    start: Instant,
}
impl ValidationControl for Control<'_> {
    fn canceled(&self) -> bool {
        !(self.authorize)() || self.start.elapsed() >= Duration::from_secs(30)
    }
    fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
        fc_windows::system::current_thread_cpu_time().map_err(|_| ContainerError::ClockUnavailable)
    }
}
fn inspect(file: &mut LockedFile, control: &Control<'_>) -> io::Result<VoicePcmMetadata> {
    let length = file.len().map_err(file_error)?;
    if length > MAX_WAV_BYTES {
        return Err(invalid());
    }
    file.file_mut().seek(SeekFrom::Start(0))?;
    let mut header = [0; 44];
    file.file_mut().read_exact(&mut header)?;
    let info = validate_wav_header(&header, length).map_err(|_| invalid())?;
    let (actual, sha256) = hash_file(file.file_mut(), control).map_err(file_error)?;
    if actual != length {
        return Err(invalid());
    }
    let pcm = VoicePcmMetadata {
        byte_length: actual,
        frame_count: info.frames,
        sha256,
    };
    pcm.validate().map_err(file_error)?;
    Ok(pcm)
}
pub struct ControlledVoiceFiles {
    files: ControlledFiles,
    staging: PathBuf,
    active: PathBuf,
}
pub struct PendingVoiceUpload {
    proof: PreparedVoiceUpload,
    pin: LockedFile,
}
impl PendingVoiceUpload {
    /// Hold this exact validated inode across the publish DB commit. If the DB
    /// result is unknown, retain its bytes for startup reconciliation.
    pub fn into_parts(self) -> (PreparedVoiceUpload, LockedFile) {
        (self.proof, self.pin)
    }
    pub fn commit(
        self,
        backend: &mut Backend,
        lease: fc_application::identity::SessionLease,
    ) -> crate::backend::Result<VoiceAsset> {
        let (proof, _pin) = self.into_parts();
        backend.voice_publish(lease, proof)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceCleanup {
    pub removed_files: u32,
    pub finalized_assets: u32,
    pub busy_assets: u32,
}
impl ControlledVoiceFiles {
    pub fn new(root: &Path) -> io::Result<Self> {
        let files = ControlledFiles::new(root).map_err(file_error)?;
        let staging = files
            .directory(&["voice".into(), "staging".into()])
            .map_err(file_error)?;
        let active = files
            .directory(&["voice".into(), "active".into()])
            .map_err(file_error)?;
        Ok(Self {
            files,
            staging,
            active,
        })
    }
    fn path(&self, active: bool, id: VoiceAssetId) -> PathBuf {
        (if active { &self.active } else { &self.staging }).join(format!("{}.wav", id.get()))
    }
    /// Existing local immutable input adapter, used by the explicit synthetic
    /// probe. HTTP bodies must use prepare_body to avoid a second uncharged spool.
    pub fn prepare(
        &self,
        input: LockedFile,
        reservation: VoiceAsset,
        authorize: &dyn Fn() -> bool,
    ) -> io::Result<PendingVoiceUpload> {
        let mut preparation = Preparation {
            owner: self,
            control: Control {
                authorize,
                start: Instant::now(),
            },
            pin: None,
        };
        let proof =
            prepare_voice_upload(&mut preparation, input, reservation).map_err(file_error)?;
        let pin = preparation.pin.take().ok_or_else(invalid)?;
        Ok(PendingVoiceUpload { proof, pin })
    }
    /// Production upload body adapter: stream directly into the already charged
    /// reservation's sole staging object. HTTP must supply a timeout-bounded
    /// reader, run this on a reserved physical worker, and never spool a second
    /// uncharged body. No client filename or path participates in publication.
    pub fn prepare_body<R: Read>(
        &self,
        input: R,
        reservation: VoiceAsset,
        authorize: &dyn Fn() -> bool,
    ) -> io::Result<PendingVoiceUpload> {
        let mut preparation = BodyPreparation {
            owner: self,
            control: Control {
                authorize,
                start: Instant::now(),
            },
            input: Some(input),
            pin: None,
        };
        let proof = prepare_voice_upload(&mut preparation, (), reservation).map_err(file_error)?;
        Ok(PendingVoiceUpload {
            proof,
            pin: preparation.pin.take().ok_or_else(invalid)?,
        })
    }
    /// Hash and native stream open overlap immutable READ-only handles, so there
    /// is no verified-path/reopen replacement window. Both deny write/delete.
    pub fn source(&self, asset: &VoiceAsset) -> io::Result<VerifiedWave> {
        if asset.state != VoiceAssetState::InUse {
            return Err(invalid());
        }
        let expected = asset.pcm.as_ref().ok_or_else(invalid)?;
        let path = self.path(true, asset.id);
        let mut pin = LockedFile::open(&path, MAX_WAV_BYTES).map_err(file_error)?;
        let control = Control {
            authorize: &|| true,
            start: Instant::now(),
        };
        if &inspect(&mut pin, &control)? != expected {
            return Err(invalid());
        }
        let source = VerifiedWave::open(&path).map_err(file_error)?;
        drop(pin);
        Ok(source)
    }
    /// Startup recovery must run first. Known live objects are never collected;
    /// exclusive actual deletion precedes quota finalization. Native-held files
    /// retain their charged bytes and are retried after the driver returns them.
    pub fn reconcile(&self, backend: &mut Backend) -> io::Result<VoiceCleanup> {
        let inventory = backend.voice_maintain().map_err(file_error)?;
        let mut report = VoiceCleanup::default();
        let ids: BTreeSet<_> = inventory.assets.iter().map(|a| a.id).collect();
        for asset in &inventory.assets {
            if asset.state != VoiceAssetState::DeletePending {
                continue;
            }
            let mut complete = true;
            for active in [false, true] {
                match self.files.remove(&self.path(active, asset.id)) {
                    Ok(removed) => report.removed_files += u32::from(removed),
                    Err(_) => complete = false,
                }
            }
            if complete {
                backend
                    .voice_finalize_delete(asset.id, asset.revision)
                    .map_err(file_error)?;
                report.finalized_assets += 1;
            } else {
                report.busy_assets += 1;
            }
        }
        for dir in [&self.staging, &self.active] {
            for path in self.files.entries(dir, 256).map_err(file_error)? {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(invalid)?;
                let raw = name.strip_suffix(".wav").ok_or_else(invalid)?;
                let id = raw
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| VoiceAssetId::new(n).ok())
                    .ok_or_else(invalid)?;
                if raw != id.get().to_string() {
                    return Err(invalid());
                }
                if !ids.contains(&id) {
                    let mut unavailable = false;
                    let removed = self
                        .files
                        .remove_when(&path, |_| {
                            // Pin the exact orphan before consulting current DB state.
                            // A newly reserved upload may have appeared after the
                            // first inventory was captured; it must never be removed.
                            match backend.voice_inventory() {
                                Ok(fresh) => !fresh.assets.iter().any(|a| a.id == id),
                                Err(_) => {
                                    unavailable = true;
                                    false
                                }
                            }
                        })
                        .map_err(file_error)?;
                    if unavailable {
                        return Err(file_error("voice inventory unavailable"));
                    }
                    report.removed_files += u32::from(removed);
                }
            }
        }
        Ok(report)
    }
}
struct Preparation<'a> {
    owner: &'a ControlledVoiceFiles,
    control: Control<'a>,
    pin: Option<LockedFile>,
}
impl VoicePreparation for Preparation<'_> {
    type Input = LockedFile;
    fn prepare(
        &mut self,
        mut input: LockedFile,
        reservation: &VoiceAsset,
    ) -> Result<VoicePcmMetadata, VoiceRepositoryError> {
        let mut work = || -> io::Result<(VoicePcmMetadata, LockedFile)> {
            if self.control.canceled() {
                return Err(canceled());
            }
            let original = inspect(&mut input, &self.control)?;
            input.file_mut().seek(SeekFrom::Start(0))?;
            let staging = self.owner.path(false, reservation.id);
            let mut target = self.owner.files.create_new(&staging).map_err(file_error)?;
            let mut block = [0; 32 * 1024];
            let mut bytes = 0u64;
            loop {
                if self.control.canceled() {
                    return Err(canceled());
                }
                let n = input.file_mut().read(&mut block)?;
                if n == 0 {
                    break;
                }
                bytes = bytes.checked_add(n as u64).ok_or_else(invalid)?;
                if bytes > MAX_WAV_BYTES {
                    return Err(invalid());
                }
                target.file_mut().write_all(&block[..n])?;
            }
            if bytes != original.byte_length {
                return Err(invalid());
            }
            if self.control.canceled() {
                return Err(canceled());
            }
            let mut pin = target.seal().map_err(file_error)?;
            if inspect(&mut pin, &self.control)? != original {
                return Err(invalid());
            }
            if self.control.canceled() {
                return Err(canceled());
            }
            pin = pin
                .publish(&self.owner.files, &self.owner.path(true, reservation.id))
                .map_err(file_error)?;
            Ok((original, pin))
        };
        let (pcm, pin) = work().map_err(|e| preparation_error(e, &self.control))?;
        self.pin = Some(pin);
        Ok(pcm)
    }
}

struct BodyPreparation<'a, R: Read> {
    owner: &'a ControlledVoiceFiles,
    control: Control<'a>,
    input: Option<R>,
    pin: Option<LockedFile>,
}
impl<R: Read> VoicePreparation for BodyPreparation<'_, R> {
    type Input = ();
    fn prepare(
        &mut self,
        _: (),
        reservation: &VoiceAsset,
    ) -> Result<VoicePcmMetadata, VoiceRepositoryError> {
        let mut work = || -> io::Result<(VoicePcmMetadata, LockedFile)> {
            if self.control.canceled() {
                return Err(canceled());
            }
            let input = self.input.as_mut().ok_or_else(invalid)?;
            let mut target = self
                .owner
                .files
                .create_new(&self.owner.path(false, reservation.id))
                .map_err(file_error)?;
            let mut block = [0; 32 * 1024];
            let mut bytes = 0u64;
            loop {
                if self.control.canceled() {
                    return Err(canceled());
                }
                let n = input.read(&mut block)?;
                if n == 0 {
                    break;
                }
                bytes = bytes.checked_add(n as u64).ok_or_else(invalid)?;
                if bytes > MAX_WAV_BYTES {
                    return Err(invalid());
                }
                target.file_mut().write_all(&block[..n])?;
            }
            if self.control.canceled() {
                return Err(canceled());
            }
            let mut pin = target.seal().map_err(file_error)?;
            let pcm = inspect(&mut pin, &self.control)?;
            if pcm.byte_length != bytes || self.control.canceled() {
                return Err(invalid());
            }
            pin = pin
                .publish(&self.owner.files, &self.owner.path(true, reservation.id))
                .map_err(file_error)?;
            Ok((pcm, pin))
        };
        let (pcm, pin) = work().map_err(|e| preparation_error(e, &self.control))?;
        self.pin = Some(pin);
        Ok(pcm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::OpenOptions,
        os::windows::io::AsRawHandle,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let r = std::fs::remove_dir_all(&self.0);
            if !std::thread::panicking() {
                r.unwrap();
            }
        }
    }
    fn silence() -> Vec<u8> {
        let bytes = 16_000u32;
        let mut h = vec![0; 44 + bytes as usize];
        h[..4].copy_from_slice(b"RIFF");
        h[4..8].copy_from_slice(&(bytes + 36).to_le_bytes());
        h[8..16].copy_from_slice(b"WAVEfmt ");
        h[16..20].copy_from_slice(&16u32.to_le_bytes());
        h[20..22].copy_from_slice(&1u16.to_le_bytes());
        h[22..24].copy_from_slice(&1u16.to_le_bytes());
        h[24..28].copy_from_slice(&16_000u32.to_le_bytes());
        h[28..32].copy_from_slice(&32_000u32.to_le_bytes());
        h[32..34].copy_from_slice(&2u16.to_le_bytes());
        h[34..36].copy_from_slice(&16u16.to_le_bytes());
        h[36..40].copy_from_slice(b"data");
        h[40..44].copy_from_slice(&bytes.to_le_bytes());
        h
    }
    #[test]
    fn sealed_voice_inode_rejects_writes_replacement_and_hash_tampering() {
        let directory = Directory(std::env::temp_dir().join(format!(
                "FutureClassroom-Test-Voice-Files-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&directory.0).unwrap();
        let files = ControlledVoiceFiles::new(&directory.0).unwrap();
        let id = VoiceAssetId::new(123).unwrap();
        let staging = files.path(false, id);
        let active = files.path(true, id);
        let body = silence();
        let mut new = files.files.create_new(&staging).unwrap();
        new.file_mut().write_all(&body).unwrap();
        let mut pin = new.seal().unwrap();
        let handle = pin.file().as_raw_handle();
        assert!(
            pin.file_mut().write_all(&[1]).is_err(),
            "sealed handle must lose WRITE rights"
        );
        assert!(OpenOptions::new().write(true).open(&staging).is_err());
        assert!(std::fs::rename(&staging, staging.with_extension("replacement")).is_err());
        let pcm = inspect(
            &mut pin,
            &Control {
                authorize: &|| true,
                start: Instant::now(),
            },
        )
        .unwrap();
        pin = pin.publish(&files.files, &active).unwrap();
        assert_eq!(
            pin.file().as_raw_handle(),
            handle,
            "publication keeps exact sealed handle"
        );
        assert!(pin.file_mut().write_all(&[1]).is_err());
        assert_eq!(
            inspect(
                &mut pin,
                &Control {
                    authorize: &|| true,
                    start: Instant::now()
                }
            )
            .unwrap(),
            pcm
        );
        drop(pin);
        let asset = VoiceAsset {
            id,
            owner: fc_domain::ids::PrincipalId::new(1).unwrap(),
            class_id: fc_domain::ids::ClassId::new(1).unwrap(),
            state: VoiceAssetState::InUse,
            revision: fc_domain::ids::Revision::new(1),
            charged_bytes: pcm.byte_length,
            pcm: Some(pcm),
            created_at: 0,
            expires_at: 600,
        };
        let source = files.source(&asset).unwrap();
        assert!(OpenOptions::new().write(true).open(&active).is_err());
        assert!(std::fs::rename(&active, active.with_extension("replacement")).is_err());
        drop(source);
        let mut changed = OpenOptions::new().write(true).open(&active).unwrap();
        changed.seek(SeekFrom::Start(44)).unwrap();
        changed.write_all(&[1]).unwrap();
        changed.sync_all().unwrap();
        drop(changed);
        assert!(
            files.source(&asset).is_err(),
            "valid canonical header cannot hide changed body hash"
        );
    }
    #[test]
    fn streamed_body_has_one_charged_object_and_sealed_proof() {
        let directory = Directory(std::env::temp_dir().join(format!(
                "FutureClassroom-Test-Voice-Body-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&directory.0).unwrap();
        let files = ControlledVoiceFiles::new(&directory.0).unwrap();
        let id = VoiceAssetId::new(124).unwrap();
        let asset = VoiceAsset {
            id,
            owner: fc_domain::ids::PrincipalId::new(1).unwrap(),
            class_id: fc_domain::ids::ClassId::new(1).unwrap(),
            state: VoiceAssetState::Reserved,
            revision: fc_domain::ids::Revision::new(1),
            charged_bytes: MAX_WAV_BYTES,
            pcm: None,
            created_at: 0,
            expires_at: 600,
        };
        let body = silence();
        let pending = files.prepare_body(&body[..], asset, &|| true).unwrap();
        let (proof, mut pin) = pending.into_parts();
        assert_eq!(proof.pcm().byte_length, body.len() as u64);
        assert!(files.files.entries(&files.staging, 2).unwrap().is_empty());
        assert_eq!(files.files.entries(&files.active, 2).unwrap().len(), 1);
        assert!(pin.file_mut().write_all(&[1]).is_err());
        assert_eq!(pin.path(), files.path(true, id));
        drop(pin);
    }
}
