//! Bounded full WIC decode, without thumbnail generation or pixel caching.
use super::files::LockedFile;
use fc_domain::materials::MaterialKind;
use std::{
    sync::{Mutex, MutexGuard, TryLockError},
    time::{Duration, Instant},
};
use windows::{
    core::{Error, Result},
    Win32::{Foundation::*, Graphics::Imaging::*, System::Com::*},
};
pub(super) static DECODER: Mutex<()> = Mutex::new(());

/// One physical decoder for import validation and demand rasterization. Waiting
/// uses only the caller's existing worker and its ORIGINAL per-file budget.
/// A deadline cannot return a live decoder slot before its synchronous owner exits.
pub(super) struct DecoderBudget {
    started: Instant,
    cpu_started: Duration,
}
impl DecoderBudget {
    pub(super) fn new() -> Result<Self> {
        let started = Instant::now();
        let cpu_started = crate::system::current_thread_cpu_time()?;
        Ok(Self {
            started,
            cpu_started,
        })
    }
    pub(super) fn check(&self, can_continue: &impl Fn() -> bool) -> Result<()> {
        if !can_continue() || self.started.elapsed() >= Duration::from_secs(30) {
            return Err(Error::from_hresult(ERROR_CANCELLED.to_hresult()));
        }
        if crate::system::current_thread_cpu_time()?.saturating_sub(self.cpu_started)
            >= Duration::from_secs(10)
        {
            return Err(Error::from_hresult(ERROR_TIMEOUT.to_hresult()));
        }
        Ok(())
    }
    pub(super) fn admit(
        &self,
        can_continue: &impl Fn() -> bool,
    ) -> Result<MutexGuard<'static, ()>> {
        loop {
            self.check(can_continue)?;
            match DECODER.try_lock() {
                Ok(slot) => {
                    // Cancellation can arrive while acquiring the slot. Do not
                    // create any COM/WIC object after a revoked/deadline waiter.
                    self.check(can_continue)?;
                    return Ok(slot);
                }
                Err(TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(TryLockError::Poisoned(_)) => return Err(Error::from_hresult(E_UNEXPECTED)),
            }
        }
    }
}
pub(super) struct ComScope(bool);
impl ComScope {
    pub(super) fn new() -> Result<Self> {
        let h = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if h == RPC_E_CHANGED_MODE {
            Ok(Self(false))
        } else {
            h.ok()?;
            Ok(Self(true))
        }
    }
}
impl Drop for ComScope {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageValidation {
    pub width: u32,
    pub height: u32,
    pub decoded_bytes: u64,
}
pub fn validate_image(
    source: &LockedFile,
    kind: MaterialKind,
    canceled: impl Fn() -> bool,
) -> Result<ImageValidation> {
    let budget = DecoderBudget::new()?;
    let can_continue = || !canceled();
    let guid = match kind {
        MaterialKind::Png => GUID_ContainerFormatPng,
        MaterialKind::Jpeg => GUID_ContainerFormatJpeg,
        MaterialKind::Bmp => GUID_ContainerFormatBmp,
        _ => return Err(Error::from_hresult(E_INVALIDARG)),
    };
    if source.len()? > 10 * 1024 * 1024 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let _slot = budget.admit(&can_continue)?;
    let _com = ComScope::new()?;
    budget.check(&can_continue)?;
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        use std::os::windows::io::AsRawHandle;
        let decoder = factory.CreateDecoderFromFileHandle(
            source.file().as_raw_handle() as usize,
            &GUID_VendorMicrosoft,
            WICDecodeMetadataCacheOnDemand,
        )?;
        if decoder.GetContainerFormat()? != guid || decoder.GetFrameCount()? != 1 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let frame = decoder.GetFrame(0)?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height)?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 8_000_000 {
            return Err(Error::from_hresult(E_INVALIDARG));
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
        let tile_width = width.min(4096);
        let stride = tile_width
            .checked_mul(4)
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
        // WIC may retain internal codec buffers; these scanline bytes are only our
        // explicit buffer, not a claim that the decoder's whole peak is 64KiB.
        let rows = (64 * 1024 / stride).max(1).min(height);
        let mut buffer = vec![0u8; (rows * stride) as usize];
        let mut y = 0;
        while y < height {
            budget.check(&can_continue)?;
            let n = rows.min(height - y);
            let mut x = 0;
            while x < width {
                budget.check(&can_continue)?;
                let columns = tile_width.min(width - x);
                let rect = WICRect {
                    X: x as i32,
                    Y: y as i32,
                    Width: columns as i32,
                    Height: n as i32,
                };
                converter.CopyPixels(&rect, stride, &mut buffer[..(n * stride) as usize])?;
                x += columns;
            }
            y += n;
        }
        budget.check(&can_continue)?;
        Ok(ImageValidation {
            width,
            height,
            decoded_bytes: u64::from(width) * u64::from(height) * 4,
        })
    }
}

#[cfg(test)]
#[path = "image_admission_tests.rs"]
mod admission_tests;
