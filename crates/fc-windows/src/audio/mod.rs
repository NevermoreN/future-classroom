//! WinMM single-stream output. All calls belong to the existing audio domain.
//! The driver owns submitted pointers until DONE and successful UnprepareHeader.
//! No callback function, extra thread, volume change, input device, or whole-file Vec.
use crate::materials::files::LockedFile;
use fc_ports::audio::{
    AudioOutputError as Error, AudioOutputPort, AudioOutputProgress as Progress,
};
use std::{
    cell::UnsafeCell,
    io::{Read, Seek, SeekFrom},
    marker::PhantomData,
    path::Path,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows::{
    core::PSTR,
    Win32::{
        Foundation::*,
        Media::{
            Audio::*, MMSYSERR_ALLOCATED, MMSYSERR_BADDEVICEID, MMSYSERR_NODRIVER,
            MMSYSERR_NOTENABLED,
        },
        System::Threading::*,
    },
};

pub const BUFFER_BYTES: usize = 16 * 1024;
pub const BUFFER_COUNT: usize = 3;
const HEADER_BYTES: u32 = std::mem::size_of::<WAVEHDR>() as u32;
const CLEANUP_LIMIT: Duration = Duration::from_secs(2);
static POISONED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
fn open_error(code: u32) -> Error {
    match code {
        MMSYSERR_ALLOCATED | MMSYSERR_BADDEVICEID | MMSYSERR_NODRIVER | MMSYSERR_NOTENABLED
        | WAVERR_BADFORMAT | WAVERR_SYNC => Error::DeviceUnavailable,
        _ => Error::Driver(code),
    }
}
struct Reservation;
impl Reservation {
    fn take() -> Result<Self, Error> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::Busy)?;
        Ok(Self)
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

/// Open/validate/stream the same pinned local inode. Share-write and share-delete
/// are denied by LockedFile for its complete lifetime, including repeat submission.
pub struct VerifiedWave {
    source: LockedFile,
    bytes: u32,
}
impl VerifiedWave {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let mut source = LockedFile::open(path, 1_920_044).map_err(|_| Error::InvalidSource)?;
        let length = source.len().map_err(|_| Error::Io)?;
        let mut h = [0; 44];
        source
            .file_mut()
            .read_exact(&mut h)
            .map_err(|_| Error::InvalidSource)?;
        let short = |i| u16::from_le_bytes([h[i], h[i + 1]]);
        let long = |i| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
        let bytes = long(40);
        if &h[..4] != b"RIFF"
            || &h[8..16] != b"WAVEfmt "
            || &h[36..40] != b"data"
            || long(16) != 16
            || short(20) != 1
            || short(22) != 1
            || long(24) != 16_000
            || long(28) != 32_000
            || short(32) != 2
            || short(34) != 16
            || !(16_000..=1_920_000).contains(&bytes)
            || bytes % 2 != 0
            || length != u64::from(bytes) + 44
            || u64::from(long(4)) + 8 != length
        {
            return Err(Error::InvalidSource);
        }
        Ok(Self { source, bytes })
    }
    pub fn data_bytes(&self) -> u32 {
        self.bytes
    }
    pub fn rewind(&mut self) -> Result<(), Error> {
        self.source
            .file_mut()
            .seek(SeekFrom::Start(44))
            .map_err(|_| Error::Io)?;
        Ok(())
    }
}
impl Read for VerifiedWave {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.source.file_mut().read(b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BufferState {
    Allocated,
    Prepared,
    Queued,
    Returned,
    Unprepared,
}

// WAVEHDR is packed in windows-rs. Align the fixed allocation so dwFlags is a
// naturally aligned DWORD on x86/x64. UnsafeCell permits native field mutation;
// no reference to its header or PCM is kept across a submission/wait.
#[repr(C, align(8))]
struct Memory {
    header: UnsafeCell<WAVEHDR>,
    pcm: UnsafeCell<[u8; BUFFER_BYTES]>,
}
struct Buffer {
    memory: Box<Memory>,
    state: BufferState,
}
impl Buffer {
    fn new() -> Self {
        let memory = Box::new(Memory {
            header: UnsafeCell::new(WAVEHDR::default()),
            pcm: UnsafeCell::new([0; BUFFER_BYTES]),
        });
        // SAFETY: not yet published; both pointees live in this fixed allocation.
        unsafe {
            (*memory.header.get()).lpData = PSTR(memory.pcm.get().cast::<u8>());
        }
        Self {
            memory,
            state: BufferState::Allocated,
        }
    }
    fn header(&self) -> *mut WAVEHDR {
        self.memory.header.get()
    }
    fn fill(&mut self, input: &mut dyn Read, length: usize) -> Result<(), Error> {
        debug_assert!(matches!(
            self.state,
            BufferState::Allocated | BufferState::Returned
        ));
        // SAFETY: driver never owns PCM in these states. Length is bounded by the
        // fixed allocation. No safe header/PCM reference survives this method.
        unsafe {
            input
                .read_exact(&mut (&mut *self.memory.pcm.get())[..length])
                .map_err(|_| Error::Io)?;
            (*self.header()).dwBufferLength = length as u32;
        }
        Ok(())
    }
}

trait Driver: Sized {
    fn open() -> Result<Self, Error>;
    fn prepare(&mut self, header: *mut WAVEHDR) -> u32;
    fn write(&mut self, header: *mut WAVEHDR) -> u32;
    fn returned(&self, header: *mut WAVEHDR) -> bool;
    fn reset(&mut self) -> u32;
    fn unprepare(&mut self, header: *mut WAVEHDR) -> u32;
    /// On success, also closes the event; never close it before waveOutClose.
    fn close(&mut self) -> u32;
    fn wait(&mut self, milliseconds: u32) -> Result<(), Error>;
    fn poison(&self);
}
struct NativeDriver {
    device: HWAVEOUT,
    event: HANDLE,
    _thread: PhantomData<Rc<()>>,
    _reservation: Reservation,
}
impl Driver for NativeDriver {
    fn open() -> Result<Self, Error> {
        if POISONED.load(Ordering::Acquire) {
            return Err(Error::CleanupPending);
        }
        let reservation = Reservation::take()?;
        let format = WAVEFORMATEX {
            wFormatTag: 1,
            nChannels: 1,
            nSamplesPerSec: 16_000,
            nAvgBytesPerSec: 32_000,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        // SAFETY: query does not retain the format or open a device. This call is
        // performed only in the audio execution domain; WAVE_MAPPER chooses output.
        let query =
            unsafe { waveOutOpen(None, WAVE_MAPPER, &format, None, None, WAVE_FORMAT_QUERY) };
        if query != 0 {
            return Err(open_error(query));
        }
        // SAFETY: unnamed auto-reset event, solely owned here; no inherited handle.
        let event = unsafe { CreateEventW(None, false, false, None) }.map_err(|_| Error::Io)?;
        let mut device = HWAVEOUT::default();
        // SAFETY: format remains valid for synchronous open. CALLBACK_EVENT retains
        // event value only; event lives until successful device close, including error.
        let code = unsafe {
            waveOutOpen(
                Some(&mut device),
                WAVE_MAPPER,
                &format,
                Some(event.0 as usize),
                None,
                CALLBACK_EVENT,
            )
        };
        if code != 0 {
            // SAFETY: failed open never acquired the event or any buffers.
            let _ = unsafe { CloseHandle(event) };
            return Err(open_error(code));
        }
        Ok(Self {
            device,
            event,
            _thread: PhantomData,
            _reservation: reservation,
        })
    }
    fn prepare(&mut self, h: *mut WAVEHDR) -> u32 {
        // SAFETY: Engine owns stable aligned header+PCM and keeps them through unprepare.
        unsafe { waveOutPrepareHeader(self.device, h, HEADER_BYTES) }
    }
    fn write(&mut self, h: *mut WAVEHDR) -> u32 {
        // SAFETY: prepared stable buffer; Engine forbids PCM mutation until DONE.
        unsafe { waveOutWrite(self.device, h, HEADER_BYTES) }
    }
    fn returned(&self, h: *mut WAVEHDR) -> bool {
        // SAFETY: header allocation is pinned and its dwFlags offset is aligned.
        // Driver mutates flags; no Rust shared/exclusive reference aliases its writes.
        // An event wait is the OS completion barrier; volatile forbids cached flag reads.
        unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*h).dwFlags)) & WHDR_DONE != 0 }
    }
    fn reset(&mut self) -> u32 {
        // SAFETY: device stays solely in this domain and all buffer addresses remain live.
        unsafe { waveOutReset(self.device) }
    }
    fn unprepare(&mut self, h: *mut WAVEHDR) -> u32 {
        // SAFETY: Engine calls only for prepared-never-queued or confirmed-returned buffers.
        unsafe { waveOutUnprepareHeader(self.device, h, HEADER_BYTES) }
    }
    fn close(&mut self) -> u32 {
        // SAFETY: Engine calls only after all prepared buffers were successfully unprepared.
        if !self.device.0.is_null() {
            let code = unsafe { waveOutClose(self.device) };
            if code != 0 {
                return code;
            }
            self.device = HWAVEOUT::default();
        }
        // SAFETY: waveOutClose succeeded and can no longer signal this event.
        // On failure retain the event and retry just this close, never waveOutClose
        // a second time on an already invalid device handle.
        if unsafe { CloseHandle(self.event) }.is_err() {
            return 1;
        }
        self.event = HANDLE::default();
        0
    }
    fn wait(&mut self, ms: u32) -> Result<(), Error> {
        // SAFETY: the event is live until this driver's successful close.
        let wait = unsafe { WaitForSingleObject(self.event, ms.min(50)) };
        if wait == WAIT_FAILED {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
    fn poison(&self) {
        POISONED.store(true, Ordering::Release);
    }
}

struct Stream<D: Driver> {
    driver: D,
    buffers: [Buffer; BUFFER_COUNT],
    source: Box<dyn Read>,
    remaining: usize,
    generation: u64,
    canceled: bool,
    cleaning: Option<Instant>,
    natural: bool,
    closed: bool,
    failure: Option<u32>,
}
impl<D: Driver> Stream<D> {
    fn cancel(&mut self) {
        // Invalidate before reset, even when the reset fails. DONE is ownership only.
        self.canceled = true;
        self.natural = false;
        if self.cleaning.is_none() {
            self.cleaning = Some(Instant::now());
            let code = self.driver.reset();
            if code != 0 {
                self.failure.get_or_insert(code);
                self.driver.poison();
            }
        }
    }
    fn fill(&mut self, authorize: &mut dyn FnMut() -> bool, maximum: usize) -> Result<(), Error> {
        let mut submitted = 0;
        for buffer in &mut self.buffers {
            if self.remaining == 0 {
                break;
            }
            if !matches!(buffer.state, BufferState::Allocated | BufferState::Returned) {
                continue;
            }
            let length = BUFFER_BYTES.min(self.remaining);
            buffer.fill(self.source.as_mut(), length)?;
            if buffer.state == BufferState::Allocated {
                let code = self.driver.prepare(buffer.header());
                if code != 0 {
                    self.driver.poison();
                    return Err(Error::Driver(code));
                }
                buffer.state = BufferState::Prepared;
            }
            // Reusing a prepared header may shorten dwBufferLength but never exceed
            // its original prepared capacity. Every first write/repeat rechecks authority.
            if !authorize() {
                return Err(Error::Unauthorized);
            }
            let code = self.driver.write(buffer.header());
            if code != 0 {
                self.driver.poison();
                return Err(Error::Driver(code));
            }
            buffer.state = BufferState::Queued;
            self.remaining -= length;
            submitted += 1;
            if submitted == maximum {
                break;
            }
        }
        Ok(())
    }
    fn returns(&mut self) {
        for buffer in &mut self.buffers {
            if buffer.state == BufferState::Queued && self.driver.returned(buffer.header()) {
                buffer.state = BufferState::Returned;
            }
        }
    }
    fn cleanup(&mut self) -> Progress {
        self.returns();
        for buffer in &mut self.buffers {
            if matches!(buffer.state, BufferState::Prepared | BufferState::Returned) {
                let code = self.driver.unprepare(buffer.header());
                if code == 0 {
                    buffer.state = BufferState::Unprepared;
                } else if code != WAVERR_STILLPLAYING {
                    self.failure.get_or_insert(code);
                    self.natural = false;
                    self.driver.poison();
                }
            }
        }
        if self
            .buffers
            .iter()
            .all(|b| matches!(b.state, BufferState::Allocated | BufferState::Unprepared))
        {
            let code = self.driver.close();
            if code == 0 {
                self.closed = true;
                if self.natural && !self.canceled && self.failure.is_none() {
                    return Progress::NaturallyCompleted {
                        generation: self.generation,
                    };
                }
                return Progress::Released {
                    generation: self.generation,
                };
            } else if code != WAVERR_STILLPLAYING {
                self.failure.get_or_insert(code);
                self.natural = false;
                self.driver.poison();
            }
        }
        if self
            .cleaning
            .is_some_and(|start| start.elapsed() >= CLEANUP_LIMIT)
        {
            self.driver.poison();
            Progress::Retained {
                generation: self.generation,
            }
        } else {
            Progress::Playing {
                generation: self.generation,
            }
        }
    }
}
impl<D: Driver> Drop for Stream<D> {
    fn drop(&mut self) {
        // Engine removes only closed streams normally. This fallback applies during
        // unwind too: never free a pointer/event potentially retained by the driver.
        // The actual ownership transfer is performed by Engine::drop before this Drop.
        debug_assert!(self.closed, "live driver stream must be retained by Engine");
    }
}
struct Engine<D: Driver> {
    stream: Option<Stream<D>>,
    poisoned: bool,
}
impl<D: Driver> Engine<D> {
    fn new() -> Self {
        Self {
            stream: None,
            poisoned: false,
        }
    }
    fn start(
        &mut self,
        source: Box<dyn Read>,
        bytes: usize,
        generation: u64,
        authorize: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::CleanupPending);
        }
        if self.stream.is_some() {
            return Err(Error::Busy);
        }
        if generation == 0 || bytes == 0 {
            return Err(Error::InvalidSource);
        }
        if !authorize() {
            return Err(Error::Unauthorized);
        }
        self.stream = Some(Stream {
            driver: D::open()?,
            buffers: std::array::from_fn(|_| Buffer::new()),
            source,
            remaining: bytes,
            generation,
            canceled: false,
            cleaning: None,
            natural: false,
            closed: false,
            failure: None,
        });
        let stream = self.stream.as_mut().expect("just initialized");
        // Return immediately after the first successful Write. No later fallible
        // operation can turn this successful native start into an ambiguous Err;
        // application can durably record FirstWrite before polling/feeding the rest.
        if let Err(error) = stream.fill(authorize, 1) {
            stream.cancel();
            self.poisoned |=
                stream.failure.is_some() || matches!(error, Error::Driver(_) | Error::Io);
            return Err(error);
        }
        Ok(())
    }
    fn poll(
        &mut self,
        wait_ms: u32,
        authorize: &mut dyn FnMut() -> bool,
    ) -> Result<Progress, Error> {
        let Some(stream) = self.stream.as_mut() else {
            return Ok(Progress::Idle);
        };
        if let Err(error) = stream.driver.wait(wait_ms) {
            stream.driver.poison();
            self.poisoned = true;
            stream.cancel();
            return Err(error);
        }
        stream.returns();
        if stream.cleaning.is_none() {
            if !authorize() {
                stream.cancel();
            } else {
                if let Err(error) = stream.fill(authorize, BUFFER_COUNT) {
                    self.poisoned |= matches!(error, Error::Driver(_) | Error::Io);
                    stream.cancel();
                    return Err(error);
                }
                if stream.remaining == 0
                    && stream
                        .buffers
                        .iter()
                        .all(|b| b.state != BufferState::Queued)
                {
                    stream.natural = true;
                    stream.cleaning = Some(Instant::now());
                }
            }
        }
        let progress = if stream.cleaning.is_some() {
            stream.cleanup()
        } else {
            Progress::Playing {
                generation: stream.generation,
            }
        };
        if matches!(progress, Progress::Retained { .. }) {
            self.poisoned = true;
        }
        let failure = stream.failure.take();
        self.poisoned |= failure.is_some();
        if stream.closed {
            self.stream = None;
        }
        failure.map_or(Ok(progress), |code| Err(Error::Driver(code)))
    }
    fn stop(&mut self, generation: u64) -> Result<Progress, Error> {
        let Some(stream) = self.stream.as_mut() else {
            return Ok(Progress::Idle);
        };
        if stream.generation != generation {
            return Err(Error::Unauthorized);
        }
        stream.cancel();
        let progress = stream.cleanup();
        if matches!(progress, Progress::Retained { .. }) {
            self.poisoned = true;
        }
        let failure = stream.failure.take();
        self.poisoned |= failure.is_some();
        if stream.closed {
            self.stream = None;
        }
        failure.map_or(Ok(progress), |code| Err(Error::Driver(code)))
    }
}
impl<D: Driver> Drop for Engine<D> {
    fn drop(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            stream.cancel();
            if !matches!(
                stream.cleanup(),
                Progress::Released { .. } | Progress::NaturallyCompleted { .. }
            ) {
                stream.driver.poison();
                // Bounded to this sole 3-buffer stream; poison forbids a replacement
                // native device even from a new adapter. Process shutdown is the limit.
                std::mem::forget(stream);
            }
        }
    }
}

pub struct WinMmAudioOutput {
    engine: Engine<NativeDriver>,
}
impl WinMmAudioOutput {
    pub fn new() -> Self {
        Self {
            engine: Engine::new(),
        }
    }
}
impl Default for WinMmAudioOutput {
    fn default() -> Self {
        Self::new()
    }
}
impl AudioOutputPort for WinMmAudioOutput {
    type Source = VerifiedWave;
    fn start(
        &mut self,
        source: VerifiedWave,
        generation: u64,
        auth: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        let bytes = source.bytes as usize;
        self.engine.start(Box::new(source), bytes, generation, auth)
    }
    fn poll(&mut self, wait_ms: u32, auth: &mut dyn FnMut() -> bool) -> Result<Progress, Error> {
        self.engine.poll(wait_ms, auth)
    }
    fn stop(&mut self, generation: u64) -> Result<Progress, Error> {
        self.engine.stop(generation)
    }
}

mod diagnostic;
pub use diagnostic::{silent_probe, WinMmProbeReport};
#[cfg(test)]
mod tests;
