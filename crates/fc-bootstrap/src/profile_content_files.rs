//! Local GlobalAdmin preparation on an existing bounded worker. Durable Begin
//! and media reservations precede any copy/decode; prepared file pins remain
//! alive through the sole-writer commit. This module never grants authority.
use fc_domain::{
    ids::MediaVersionId, layout::LayoutScope, materials::MaterialKind, profile_content::*,
};
use fc_materials::{
    office::{ContainerError, ValidationControl},
    validation::{decode_content_text, hash_file, validate_file, ValidationError},
};
use fc_ports::{ProfileContentPreparation, ProfileContentRepositoryError as Error, RandomSource};
use fc_storage::business::profile_content::{MediaReservation, MediaRetirement};
use fc_windows::materials::{
    chooser::SelectedFile,
    files::{ControlledFiles, LockedFile},
    image,
    raster::{RasterCache, RasterFrame},
};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[path = "profile_content_files/inventory.rs"]
mod inventory;
pub use inventory::{ContentFileLocation, OwnedContentEntry};

fn unavailable(_: impl std::fmt::Debug) -> Error {
    Error::Unavailable
}
fn invalid() -> Error {
    Error::Domain(ProfileContentError::InvalidMedia)
}
fn validation(error: ValidationError) -> Error {
    match error {
        ValidationError::Canceled => Error::Cancelled,
        ValidationError::TooLarge => Error::Domain(ProfileContentError::TextTooLarge),
        ValidationError::InvalidText => Error::Domain(ProfileContentError::InvalidText),
        _ => Error::Unavailable,
    }
}
fn extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Bmp => "bmp",
    }
}
fn material(format: ImageFormat) -> MaterialKind {
    match format {
        ImageFormat::Png => MaterialKind::Png,
        ImageFormat::Jpeg => MaterialKind::Jpeg,
        ImageFormat::Bmp => MaterialKind::Bmp,
    }
}

/// Selections originate in the native local picker. Image order is explicit and
/// retained; paths, handles and pixels are never stored in domain documents.
pub enum LocalContentInput {
    Text(String),
    Txt(SelectedFile),
    Images(Vec<SelectedFile>),
    ImagePages(Vec<ImagePageInput>),
    Empty,
}
pub enum ImagePageInput {
    Existing(MediaReference),
    Selected(SelectedFile),
}
impl LocalContentInput {
    pub fn allocation_bytes(&self) -> Option<usize> {
        let payload = match self {
            Self::Text(text) => text.capacity(),
            Self::Txt(selected) => selected.allocation_bytes(),
            Self::Images(selected) => {
                if selected.is_empty() || selected.len() > MAX_CONTENT_PAGES {
                    return None;
                }
                selected.iter().try_fold(
                    selected
                        .capacity()
                        .checked_mul(std::mem::size_of::<SelectedFile>())?,
                    |bytes, item| bytes.checked_add(item.allocation_bytes()),
                )?
            }
            Self::ImagePages(pages) => {
                if pages.is_empty() || pages.len() > MAX_CONTENT_PAGES {
                    return None;
                }
                pages.iter().try_fold(
                    pages
                        .capacity()
                        .checked_mul(std::mem::size_of::<ImagePageInput>())?,
                    |bytes, page| {
                        bytes.checked_add(match page {
                            ImagePageInput::Existing(_) => 0,
                            ImagePageInput::Selected(file) => file.allocation_bytes(),
                        })
                    },
                )?
            }
            Self::Empty => 0,
        };
        payload.checked_add(std::mem::size_of::<Self>())
    }
}
enum SelectedImage {
    New {
        source: LockedFile,
        bytes: u64,
        format: ImageFormat,
    },
    Existing(MediaReference),
}
fn open_image(file: SelectedFile) -> Result<SelectedImage, Error> {
    let format = match MaterialKind::from_name(&file.display_name().map_err(unavailable)?) {
        Some(MaterialKind::Png) => ImageFormat::Png,
        Some(MaterialKind::Jpeg) => ImageFormat::Jpeg,
        Some(MaterialKind::Bmp) => ImageFormat::Bmp,
        _ => return Err(invalid()),
    };
    let source = file.open(MAX_CONTENT_IMAGE_BYTES).map_err(unavailable)?;
    let bytes = source.len().map_err(unavailable)?;
    Ok(SelectedImage::New {
        source,
        bytes,
        format,
    })
}
enum Sources {
    Text(String),
    Txt(LockedFile),
    Images(Vec<SelectedImage>),
    Empty,
}
/// An uncloneable worker-owned immutable selection. The random token binds its
/// admission metadata; actual bytes receive independent SHA-256 verification in
/// preparation and the sole writer recomputes the canonical payload digest.
pub struct ContentSources {
    token: [u8; 32],
    sources: Sources,
}
impl ContentSources {
    /// Cheap source admission only: pin local immutable handles and inspect sizes.
    /// No payload copying, hashing, text decoding or WIC runs before durable Begin.
    pub fn open<R: RandomSource>(input: LocalContentInput, random: &mut R) -> Result<Self, Error> {
        let sources = match input {
            LocalContentInput::Text(text) => {
                if text.len() > MAX_CONTENT_TEXT_BYTES {
                    return Err(Error::Domain(ProfileContentError::TextTooLarge));
                }
                Sources::Text(text)
            }
            LocalContentInput::Txt(file) => {
                if !file
                    .display_name()
                    .map_err(unavailable)?
                    .to_ascii_lowercase()
                    .ends_with(".txt")
                {
                    return Err(Error::Domain(ProfileContentError::InvalidText));
                }
                Sources::Txt(file.open(64 * 1024).map_err(unavailable)?)
            }
            LocalContentInput::Images(files) => {
                if files.is_empty() || files.len() > MAX_CONTENT_PAGES {
                    return Err(Error::Domain(ProfileContentError::Capacity));
                }
                let mut images = Vec::with_capacity(files.len());
                for file in files {
                    images.push(open_image(file)?);
                }
                Sources::Images(images)
            }
            LocalContentInput::ImagePages(pages) => {
                if pages.is_empty() || pages.len() > MAX_CONTENT_PAGES {
                    return Err(Error::Domain(ProfileContentError::Capacity));
                }
                let mut images = Vec::with_capacity(pages.len());
                for page in pages {
                    images.push(match page {
                        ImagePageInput::Selected(file) => open_image(file)?,
                        ImagePageInput::Existing(reference) => {
                            reference.validate().map_err(Error::Domain)?;
                            SelectedImage::Existing(reference)
                        }
                    });
                }
                Sources::Images(images)
            }
            LocalContentInput::Empty => Sources::Empty,
        };
        let mut token = [0; 32];
        random.fill(&mut token).map_err(|_| Error::Unavailable)?;
        Ok(Self { token, sources })
    }
    pub fn image_sizes(&self) -> Option<Vec<u64>> {
        match &self.sources {
            Sources::Images(images) => Some(
                images
                    .iter()
                    .filter_map(|image| match image {
                        SelectedImage::New { bytes, .. } => Some(*bytes),
                        SelectedImage::Existing(_) => None,
                    })
                    .collect(),
            ),
            _ => None,
        }
    }
    pub fn intent_fingerprint(
        &self,
        slot: ContentSlotKey,
        operation: ContentOperation,
    ) -> Result<[u8; 32], Error> {
        let valid = matches!(
            (&self.sources, operation),
            (
                Sources::Text(_) | Sources::Txt(_),
                ContentOperation::Publish(ContentPayloadKind::Text)
            ) | (
                Sources::Images(_),
                ContentOperation::Publish(ContentPayloadKind::Images)
            ) | (
                Sources::Empty,
                ContentOperation::Clear | ContentOperation::Undo { .. }
            )
        );
        if !valid {
            return Err(Error::Domain(ProfileContentError::KindMismatch));
        }
        let mut bytes = b"fc-content-intent-v1\0".to_vec();
        bytes.extend_from_slice(&self.token);
        match slot.scope {
            LayoutScope::Public => bytes.push(0),
            LayoutScope::Subject(key) => {
                bytes.push(1);
                for id in [
                    key.class_id.get(),
                    key.subject_id.get(),
                    key.teacher_id.get(),
                ] {
                    bytes.extend_from_slice(&id.to_le_bytes());
                }
            }
        }
        bytes.extend_from_slice(&slot.node_id.get().to_le_bytes());
        match operation {
            ContentOperation::Publish(ContentPayloadKind::Text) => bytes.push(0),
            ContentOperation::Publish(ContentPayloadKind::Images) => bytes.push(1),
            ContentOperation::Clear => bytes.push(2),
            ContentOperation::Undo { target_revision } => {
                bytes.push(3);
                bytes.extend_from_slice(&target_revision.get().to_le_bytes());
            }
        }
        Ok(fc_security::sha256(&bytes))
    }
}

/// Separate managed namespace: material GC never scans or removes content media.
/// Construct after protected deployment read-back while holding its instance lock.
pub struct ControlledContentFiles {
    files: ControlledFiles,
    originals: PathBuf,
    quarantine: PathBuf,
}
impl ControlledContentFiles {
    pub fn new(root: &Path) -> Result<Self, Error> {
        let files = ControlledFiles::new(root).map_err(unavailable)?;
        let namespace = files
            .directory(&["profile-content".into()])
            .map_err(unavailable)?;
        let marker = namespace.join(".future-classroom-owner");
        const OWNER: &[u8] = b"FUTURE_CLASSROOM_PROFILE_CONTENT_V1\n";
        let entries = files.entries(&namespace, 3).map_err(unavailable)?;
        if !entries.contains(&marker) {
            if !entries.is_empty() {
                return Err(invalid());
            }
            let mut staging = files.create_new(&marker).map_err(unavailable)?;
            staging.file_mut().write_all(OWNER).map_err(unavailable)?;
            staging.finish().map_err(unavailable)?;
        } else if entries.iter().any(|path| {
            *path != marker
                && path
                    .file_name()
                    .is_none_or(|name| name != "originals" && name != "quarantine")
        }) {
            return Err(invalid());
        }
        let mut owner = LockedFile::open(&marker, OWNER.len() as u64).map_err(unavailable)?;
        let mut contents = [0; OWNER.len()];
        if owner.len().map_err(unavailable)? != OWNER.len() as u64 {
            return Err(invalid());
        }
        owner
            .file_mut()
            .read_exact(&mut contents)
            .map_err(unavailable)?;
        if contents != OWNER {
            return Err(invalid());
        }
        let originals = files
            .directory(&["profile-content".into(), "originals".into()])
            .map_err(unavailable)?;
        let quarantine = files
            .directory(&["profile-content".into(), "quarantine".into()])
            .map_err(unavailable)?;
        Ok(Self {
            files,
            originals,
            quarantine,
        })
    }
    fn original(&self, id: MediaVersionId, format: ImageFormat) -> PathBuf {
        self.originals
            .join(format!("{:020}.{}", id.get(), extension(format)))
    }
    fn staging(&self, id: MediaVersionId) -> PathBuf {
        self.quarantine.join(format!("{:020}.part", id.get()))
    }
    /// Trusted GC calls this only after retire succeeded and all actual file
    /// preparations/pins exited. Any busy/failed deletion keeps the quota charged;
    /// only subsequent sole-writer finalize may return the bytes.
    pub fn remove_retired(&self, ticket: &MediaRetirement) -> Result<(), Error> {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Bmp] {
            self.files
                .remove(&self.original(ticket.id, format))
                .map_err(unavailable)?;
        }
        self.files
            .remove(&self.staging(ticket.id))
            .map_err(unavailable)?;
        Ok(())
    }
    /// Caller first captures an exact current scene descriptor and rechecks it
    /// after this worker returns. Hash and WIC read the same immutable handle.
    pub fn decode_current(
        &self,
        cache: &RasterCache,
        reference: &MediaReference,
        max_width: u32,
        max_height: u32,
        can_continue: &dyn Fn() -> bool,
    ) -> Result<Arc<RasterFrame>, Error> {
        reference.validate().map_err(Error::Domain)?;
        let mut source = LockedFile::open(
            &self.original(reference.version_id, reference.format),
            MAX_CONTENT_IMAGE_BYTES,
        )
        .map_err(unavailable)?;
        let control = Control::new(can_continue)?;
        let (bytes, sha256) = hash_file(source.file_mut(), &control).map_err(validation)?;
        if bytes != reference.bytes || sha256 != reference.sha256 {
            return Err(invalid());
        }
        control.check()?;
        cache
            .decode_verified(&source, reference, sha256, max_width, max_height, || {
                !control.canceled()
            })
            .map_err(unavailable)
    }
    pub fn preparation<'a>(
        &'a self,
        reservation: Option<&'a MediaReservation>,
        can_continue: &'a dyn Fn() -> bool,
    ) -> ContentPreparation<'a> {
        ContentPreparation {
            files: self,
            reservation,
            gate: PreparationGate::Simple(can_continue),
            pins: Vec::new(),
        }
    }
    /// AuthorityUpdating keeps the admitted physical owner alive while pausing
    /// new read/effect steps. The per-file control below bounds every wait by
    /// the same cancellation, CPU and wall deadlines as decoding itself.
    pub fn preparation_while<'a>(
        &'a self,
        reservation: Option<&'a MediaReservation>,
        state: &'a dyn Fn() -> PreparationState,
    ) -> ContentPreparation<'a> {
        ContentPreparation {
            files: self,
            reservation,
            gate: PreparationGate::Stateful(state),
            pins: Vec::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreparationState {
    Ready,
    AuthorityUpdating,
    Revoked,
}
#[derive(Clone, Copy)]
enum PreparationGate<'a> {
    Simple(&'a dyn Fn() -> bool),
    Stateful(&'a dyn Fn() -> PreparationState),
}
impl PreparationGate<'_> {
    fn state(self) -> PreparationState {
        match self {
            Self::Simple(check) => {
                if check() {
                    PreparationState::Ready
                } else {
                    PreparationState::Revoked
                }
            }
            Self::Stateful(check) => check(),
        }
    }
}
struct Control<'a> {
    gate: PreparationGate<'a>,
    started: Instant,
    cpu: Duration,
}
impl<'a> Control<'a> {
    fn new(can_continue: &'a dyn Fn() -> bool) -> Result<Self, Error> {
        Self::with_gate(PreparationGate::Simple(can_continue))
    }
    fn with_gate(gate: PreparationGate<'a>) -> Result<Self, Error> {
        let control = Self {
            gate,
            started: Instant::now(),
            cpu: fc_windows::system::current_thread_cpu_time().map_err(unavailable)?,
        };
        control.check()?;
        Ok(control)
    }
    fn check(&self) -> Result<(), Error> {
        if self.canceled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl ValidationControl for Control<'_> {
    fn canceled(&self) -> bool {
        loop {
            if self.started.elapsed() >= Duration::from_secs(30)
                || fc_windows::system::current_thread_cpu_time()
                    .ok()
                    .and_then(|now| now.checked_sub(self.cpu))
                    .is_none_or(|elapsed| elapsed >= Duration::from_secs(10))
            {
                return true;
            }
            match self.gate.state() {
                PreparationState::Ready => return false,
                PreparationState::Revoked => return true,
                PreparationState::AuthorityUpdating => {
                    std::thread::park_timeout(Duration::from_millis(2))
                }
            }
        }
    }
    fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
        fc_windows::system::current_thread_cpu_time().map_err(|_| ContainerError::ClockUnavailable)
    }
}
/// Keep this object alive until commit really returns. Dropping it releases all
/// published source pins; only THEN may worker-exit settle and GC be requested.
/// Failed partial bundles remain registered/charged for trusted retirement.
pub struct ContentPreparation<'a> {
    files: &'a ControlledContentFiles,
    reservation: Option<&'a MediaReservation>,
    gate: PreparationGate<'a>,
    pins: Vec<LockedFile>,
}
impl ProfileContentPreparation for ContentPreparation<'_> {
    type Input = ContentSources;
    fn sha256(&self, bytes: &[u8]) -> Result<[u8; 32], Error> {
        Ok(fc_security::sha256(bytes))
    }
    fn prepare(
        &mut self,
        admission: &ContentAdmission,
        input: ContentSources,
    ) -> Result<ContentPayload, Error> {
        Control::with_gate(self.gate)?.check()?;
        if input.intent_fingerprint(admission.request.slot, admission.request.operation)?
            != admission.request.payload_fingerprint
        {
            return Err(Error::IdempotencyConflict);
        }
        match input.sources {
            Sources::Text(text) => {
                let payload = ContentPayload::Text(text);
                payload
                    .validate(admission.request.slot)
                    .map_err(Error::Domain)?;
                Ok(payload)
            }
            Sources::Txt(mut source) => {
                let control = Control::with_gate(self.gate)?;
                let text = decode_content_text(source.file_mut(), &control).map_err(validation)?;
                Ok(ContentPayload::Text(text))
            }
            Sources::Empty => Ok(ContentPayload::Empty),
            Sources::Images(images) => {
                let sizes: Vec<_> = images
                    .iter()
                    .filter_map(|image| match image {
                        SelectedImage::New { bytes, .. } => Some(*bytes),
                        SelectedImage::Existing(_) => None,
                    })
                    .collect();
                match self.reservation {
                    Some(reservation)
                        if reservation.admission == *admission
                            && reservation.ids.len() == sizes.len()
                            && reservation.bytes == sizes => {}
                    None if sizes.is_empty() => (),
                    _ => return Err(invalid()),
                }
                let mut pages = Vec::with_capacity(images.len());
                let mut new_index = 0;
                for selected in images {
                    let control = Control::with_gate(self.gate)?;
                    let SelectedImage::New {
                        mut source,
                        bytes,
                        format,
                    } = selected
                    else {
                        let SelectedImage::Existing(reference) = selected else {
                            return Err(invalid());
                        };
                        if reference.owner != admission.request.slot {
                            return Err(invalid());
                        }
                        let mut original = LockedFile::open(
                            &self.files.original(reference.version_id, reference.format),
                            MAX_CONTENT_IMAGE_BYTES,
                        )
                        .map_err(unavailable)?;
                        let (bytes, sha256) =
                            hash_file(original.file_mut(), &control).map_err(validation)?;
                        if bytes != reference.bytes || sha256 != reference.sha256 {
                            return Err(invalid());
                        }
                        original
                            .file_mut()
                            .seek(SeekFrom::Start(0))
                            .map_err(unavailable)?;
                        let decoded =
                            image::validate_image(&original, material(reference.format), || {
                                control.canceled()
                            })
                            .map_err(unavailable)?;
                        control.check()?;
                        if (decoded.width, decoded.height) != (reference.width, reference.height) {
                            return Err(invalid());
                        }
                        self.pins.push(original);
                        pages.push(reference);
                        continue;
                    };
                    let id = self
                        .reservation
                        .and_then(|reservation| reservation.ids.get(new_index))
                        .ok_or_else(invalid)?;
                    new_index += 1;
                    let mut staging = self
                        .files
                        .files
                        .create_new(&self.files.staging(*id))
                        .map_err(unavailable)?;
                    source
                        .file_mut()
                        .seek(SeekFrom::Start(0))
                        .map_err(unavailable)?;
                    let mut buffer = [0; 32 * 1024];
                    let mut copied = 0u64;
                    loop {
                        control.check()?;
                        let n = source.file_mut().read(&mut buffer).map_err(unavailable)?;
                        control.check()?;
                        if n == 0 {
                            break;
                        }
                        copied = copied.checked_add(n as u64).ok_or_else(invalid)?;
                        if copied > bytes {
                            return Err(invalid());
                        }
                        staging
                            .file_mut()
                            .write_all(&buffer[..n])
                            .map_err(unavailable)?;
                    }
                    if copied != bytes {
                        return Err(invalid());
                    }
                    let mut pinned = staging.seal().map_err(unavailable)?;
                    let report = validate_file(
                        pinned.file_mut(),
                        &format!("image.{}", extension(format)),
                        &control,
                    )
                    .map_err(validation)?;
                    let header = report.image.ok_or_else(invalid)?;
                    pinned
                        .file_mut()
                        .seek(SeekFrom::Start(0))
                        .map_err(unavailable)?;
                    let decoded =
                        image::validate_image(&pinned, material(format), || control.canceled())
                            .map_err(unavailable)?;
                    control.check()?;
                    if report.bytes != bytes
                        || (header.width, header.height) != (decoded.width, decoded.height)
                    {
                        return Err(invalid());
                    }
                    let resource = MediaReference {
                        version_id: *id,
                        owner: admission.request.slot,
                        sha256: report.sha256,
                        bytes: report.bytes,
                        width: decoded.width,
                        height: decoded.height,
                        format,
                    };
                    resource.validate().map_err(Error::Domain)?;
                    let original = pinned
                        .publish(&self.files.files, &self.files.original(*id, format))
                        .map_err(unavailable)?;
                    self.pins.push(original);
                    pages.push(resource);
                }
                Control::with_gate(self.gate)?.check()?;
                Ok(ContentPayload::Images(pages))
            }
        }
    }
}

#[cfg(test)]
#[path = "profile_content_files_tests.rs"]
mod tests;
