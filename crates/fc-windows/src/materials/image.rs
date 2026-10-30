//! Bounded full WIC decode, without thumbnail generation or pixel caching.
use super::files::LockedFile;
use fc_domain::materials::MaterialKind;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use windows::{
    core::{Error, Result},
    Win32::{Foundation::*, Graphics::Imaging::*, System::Com::*},
};
static DECODER: Mutex<()> = Mutex::new(());
struct ComScope(bool);
impl ComScope {
    fn new() -> Result<Self> {
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
    let _slot = DECODER
        .try_lock()
        .map_err(|_| Error::from_hresult(ERROR_BUSY.to_hresult()))?;
    let _com = ComScope::new()?;
    let started = Instant::now();
    let cpu_start = crate::system::current_thread_cpu_time()?;
    let guid = match kind {
        MaterialKind::Png => GUID_ContainerFormatPng,
        MaterialKind::Jpeg => GUID_ContainerFormatJpeg,
        MaterialKind::Bmp => GUID_ContainerFormatBmp,
        _ => return Err(Error::from_hresult(E_INVALIDARG)),
    };
    if source.len()? > 10 * 1024 * 1024 || canceled() {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
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
            if canceled() || started.elapsed() > Duration::from_secs(30) {
                return Err(Error::from_hresult(ERROR_CANCELLED.to_hresult()));
            }
            if crate::system::current_thread_cpu_time()?.saturating_sub(cpu_start)
                >= Duration::from_secs(10)
            {
                return Err(Error::from_hresult(ERROR_TIMEOUT.to_hresult()));
            }
            let n = rows.min(height - y);
            let mut x = 0;
            while x < width {
                if canceled() {
                    return Err(Error::from_hresult(ERROR_CANCELLED.to_hresult()));
                }
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
        Ok(ImageValidation {
            width,
            height,
            decoded_bytes: u64::from(width) * u64::from(height) * 4,
        })
    }
}
