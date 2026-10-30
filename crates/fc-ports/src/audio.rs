//! One stream owned by the audio execution domain. No native handle crosses this port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioAuthorization {
    Authorized,
    Deferred,
    Revoked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioOutputError {
    DeviceUnavailable,
    InvalidSource,
    Unauthorized,
    Busy,
    Io,
    Driver(u32),
    CleanupPending,
    /// Adapter retains the same bounded source/stream; no first Write succeeded.
    /// Poll resumes it and emits FirstWrite after authority becomes Authorized.
    Deferred,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioOutputProgress {
    Idle,
    Playing {
        generation: u64,
    },
    Deferred {
        generation: u64,
    },
    FirstWrite {
        generation: u64,
    },
    /// All bytes naturally returned AND native unprepare/close completed. This
    /// carries release confirmation too; the coordinator persists NaturalComplete
    /// and then Released separately, preserving both durable transitions.
    NaturallyCompleted {
        generation: u64,
    },
    Released {
        generation: u64,
    },
    /// Driver ownership remains. Refuse new work; poll for late return or terminate
    /// the process in its existing shutdown deadline, retaining native allocations.
    Retained {
        generation: u64,
    },
}

pub trait AudioOutputPort {
    /// Owned immutable, validated canonical PCM16/mono/16 kHz source. The platform
    /// adapter must bind validation and streaming to the same read-only file handle.
    type Source;
    /// Runs only on the audio domain. Recheck the shared health/authority capability
    /// immediately before each native submission, including every repeat's first write.
    /// Ok guarantees the FIRST native Write succeeded; Err guarantees there was
    /// no successful Write in this call. Return immediately after that first write,
    /// so failure while feeding later blocks cannot hide an already-started effect.
    fn start(
        &mut self,
        source: Self::Source,
        generation: u64,
        authorize: &mut dyn FnMut() -> AudioAuthorization,
    ) -> Result<(), AudioOutputError>;
    /// Wait at most 50 ms in the audio domain; never call from UI or the writer.
    /// Natural completion means every byte was submitted and returned without reset.
    fn poll(
        &mut self,
        wait_ms: u32,
        authorize: &mut dyn FnMut() -> AudioAuthorization,
    ) -> Result<AudioOutputProgress, AudioOutputError>;
    /// Invalidate this generation BEFORE reset. Canceled DONE may only release
    /// ownership; it can never emit NaturallyCompleted. Exact generation is required.
    fn stop(&mut self, generation: u64) -> Result<AudioOutputProgress, AudioOutputError>;
}
