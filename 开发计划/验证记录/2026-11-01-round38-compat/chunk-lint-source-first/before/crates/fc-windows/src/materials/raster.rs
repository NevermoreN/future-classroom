//! Immutable render pixels, separate from the compositor's 16 MiB DIBs.
//! Reservations include frames retained by an old renderer after cache eviction;
//! neither a timeout nor removal from the map can return those bytes early.
use super::{
    files::LockedFile,
    image::{ComScope, DecoderBudget},
};
use fc_domain::ids::MediaVersionId;
use fc_domain::profile_content::{ContentSlotKey, ImageFormat, MediaReference};
use std::{
    collections::HashMap,
    io::{Seek, SeekFrom},
    os::windows::io::AsRawHandle,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use windows::{
    core::{Error, Result},
    Win32::{Foundation::*, Graphics::Imaging::*, System::Com::*},
};

pub const IMAGE_CACHE_BYTES: usize = 32 * 1024 * 1024;
const IMAGE_CACHE_ENTRIES: usize = 256;

fn invalid() -> Error {
    Error::from_hresult(E_INVALIDARG)
}
fn busy() -> Error {
    Error::from_hresult(ERROR_BUSY.to_hresult())
}

struct Budget {
    used: AtomicUsize,
    limit: usize,
}
struct Reservation {
    budget: Arc<Budget>,
    bytes: usize,
}
impl Budget {
    fn reserve(self: &Arc<Self>, bytes: usize) -> Result<Reservation> {
        let mut current = self.used.load(Ordering::Acquire);
        loop {
            let next = current
                .checked_add(bytes)
                .filter(|n| *n <= self.limit)
                .ok_or_else(|| Error::from_hresult(ERROR_NOT_ENOUGH_MEMORY.to_hresult()))?;
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(Reservation {
                        budget: self.clone(),
                        bytes,
                    })
                }
                Err(actual) => current = actual,
            }
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RasterKey {
    pub owner: ContentSlotKey,
    pub version: MediaVersionId,
    pub sha256: [u8; 32],
    pub width: u32,
    pub height: u32,
}

/// The pixel Vec is private and cannot be cloned out of its reservation. Keep
/// this Arc alive while borrowing pixels; no COM interface or source handle is
/// retained in a frame, and no pixel payload belongs in an ordinary mailbox.
pub struct RasterFrame {
    key: RasterKey,
    source: MediaReference,
    pixels: Vec<u8>,
    _reservation: Reservation,
}
impl RasterFrame {
    pub fn key(&self) -> RasterKey {
        self.key
    }
    pub fn source(&self) -> &MediaReference {
        &self.source
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn stride(&self) -> usize {
        self.key.width as usize * 4
    }
}

/// Integer containment never crops, stretches or enlarges the source image.
pub fn contained_pixels(
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
) -> Result<(u32, u32)> {
    if width == 0
        || height == 0
        || max_width == 0
        || max_height == 0
        || u64::from(width) * u64::from(height) > 8_000_000
    {
        return Err(invalid());
    }
    let max_width = max_width.min(width);
    let max_height = max_height.min(height);
    if u64::from(max_width) * u64::from(height) <= u64::from(max_height) * u64::from(width) {
        Ok((
            max_width,
            (u64::from(height) * u64::from(max_width) / u64::from(width)).max(1) as u32,
        ))
    } else {
        Ok((
            (u64::from(width) * u64::from(max_height) / u64::from(height)).max(1) as u32,
            max_height,
        ))
    }
}

pub struct RasterCache {
    budget: Arc<Budget>,
    entries: Mutex<HashMap<RasterKey, Arc<RasterFrame>>>,
}
impl Default for RasterCache {
    fn default() -> Self {
        Self::new()
    }
}
impl RasterCache {
    pub fn new() -> Self {
        Self {
            budget: Arc::new(Budget {
                used: AtomicUsize::new(0),
                limit: IMAGE_CACHE_BYTES,
            }),
            entries: Mutex::new(HashMap::new()),
        }
    }
    pub fn bytes_in_use(&self) -> usize {
        self.budget.used.load(Ordering::Acquire)
    }
    pub fn get(
        &self,
        source: &MediaReference,
        max_width: u32,
        max_height: u32,
    ) -> Result<Option<Arc<RasterFrame>>> {
        source.validate().map_err(|_| invalid())?;
        let (width, height) = contained_pixels(source.width, source.height, max_width, max_height)?;
        let key = RasterKey {
            owner: source.owner,
            version: source.version_id,
            sha256: source.sha256,
            width,
            height,
        };
        Ok(self
            .entries
            .try_lock()
            .map_err(|_| busy())?
            .get(&key)
            .filter(|frame| frame.source == *source)
            .cloned())
    }
    /// Clears cache ownership only. A currently painted/retained frame continues
    /// charging the same budget until the last physical owner releases its Arc.
    pub fn clear(&self) -> Result<()> {
        self.entries.try_lock().map_err(|_| busy())?.clear();
        Ok(())
    }
    /// Trusted worker supplies the actual streaming digest of this SAME pinned
    /// handle; caller-provided filenames or hashes alone are never verification.
    /// Full import validation precedes durable publication. This demand decode
    /// rechecks fixed version metadata and uses the existing single WIC slot.
    pub fn decode_verified(
        &self,
        source: &LockedFile,
        reference: &MediaReference,
        actual_sha256: [u8; 32],
        max_width: u32,
        max_height: u32,
        can_continue: impl Fn() -> bool,
    ) -> Result<Arc<RasterFrame>> {
        let budget = DecoderBudget::new()?;
        reference.validate().map_err(|_| invalid())?;
        if actual_sha256 != reference.sha256 || source.len()? != reference.bytes || !can_continue()
        {
            return Err(invalid());
        }
        budget.check(&can_continue)?;
        if let Some(frame) = self.get(reference, max_width, max_height)? {
            budget.check(&can_continue)?;
            return Ok(frame);
        }
        let _slot = budget.admit(&can_continue)?;
        let _com = ComScope::new()?;
        let check = || budget.check(&can_continue);
        let (width, height) =
            contained_pixels(reference.width, reference.height, max_width, max_height)?;
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(invalid)?;
        // Evict only under actual byte/entry pressure. A scene can have several
        // independently prepared images before its renderer retains their Arcs;
        // unconditional eviction here would prevent that scene ever becoming ready.
        // Bound metadata as well, including the 1x1-pixel image case.
        {
            let mut entries = self.entries.try_lock().map_err(|_| busy())?;
            if entries.len() >= IMAGE_CACHE_ENTRIES
                || self
                    .bytes_in_use()
                    .checked_add(bytes)
                    .is_none_or(|n| n > IMAGE_CACHE_BYTES)
            {
                entries.retain(|_, frame| Arc::strong_count(frame) > 1);
            }
            if entries.len() >= IMAGE_CACHE_ENTRIES {
                return Err(Error::from_hresult(E_OUTOFMEMORY));
            }
        }
        let reservation = self.budget.reserve(bytes)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(bytes)
            .map_err(|_| Error::from_hresult(E_OUTOFMEMORY))?;
        if pixels.capacity() != bytes {
            return Err(Error::from_hresult(E_OUTOFMEMORY));
        }
        pixels.resize(bytes, 0);
        check()?;
        // Streaming verification leaves this exact immutable handle at EOF.
        // Rewind it before WIC instead of opening the original pathname again.
        let mut pinned_reader = source.file();
        pinned_reader.seek(SeekFrom::Start(0))?;
        // SAFETY: the immutable source capability owns the file and ancestor
        // pins for the whole synchronous decode. All COM objects drop on this
        // worker before the immutable owned PBGRA frame leaves this function.
        unsafe {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let decoder = factory.CreateDecoderFromFileHandle(
                source.file().as_raw_handle() as usize,
                &GUID_VendorMicrosoft,
                WICDecodeMetadataCacheOnDemand,
            )?;
            let expected = match reference.format {
                ImageFormat::Png => GUID_ContainerFormatPng,
                ImageFormat::Jpeg => GUID_ContainerFormatJpeg,
                ImageFormat::Bmp => GUID_ContainerFormatBmp,
            };
            if decoder.GetContainerFormat()? != expected || decoder.GetFrameCount()? != 1 {
                return Err(invalid());
            }
            let frame = decoder.GetFrame(0)?;
            let (mut source_width, mut source_height) = (0, 0);
            frame.GetSize(&mut source_width, &mut source_height)?;
            if (source_width, source_height) != (reference.width, reference.height) {
                return Err(invalid());
            }
            let converter = factory.CreateFormatConverter()?;
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.,
                WICBitmapPaletteTypeCustom,
            )?;
            if (width, height) == (source_width, source_height) {
                converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
            } else {
                let scaler = factory.CreateBitmapScaler()?;
                scaler.Initialize(&converter, width, height, WICBitmapInterpolationModeFant)?;
                // One bounded output copy avoids repeatedly resampling the image
                // for each scanline. A native call is owned until it really returns;
                // deadline/cancellation never releases its live buffer early.
                scaler.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
            }
        }
        check()?;
        for (index, pixel) in pixels.chunks_exact(4).enumerate() {
            if pixel[..3].iter().any(|channel| *channel > pixel[3]) {
                return Err(invalid());
            }
            if index % 16_384 == 0 {
                check()?;
            }
        }
        let key = RasterKey {
            owner: reference.owner,
            version: reference.version_id,
            sha256: reference.sha256,
            width,
            height,
        };
        let frame = Arc::new(RasterFrame {
            key,
            source: reference.clone(),
            pixels,
            _reservation: reservation,
        });
        self.entries
            .try_lock()
            .map_err(|_| busy())?
            .insert(key, frame.clone());
        Ok(frame)
    }
}

#[cfg(test)]
#[path = "raster_tests.rs"]
mod tests;
