//! Read-only WinMM format support; no output handle or audio-domain reservation.
use fc_ports::audio::{AudioOutputError, DefaultOutputSupport};
use windows::Win32::Media::{
    Audio::{
        waveOutOpen, MIDI_WAVE_OPEN_TYPE, WAVEFORMATEX, WAVERR_BADFORMAT, WAVERR_SYNC,
        WAVE_FORMAT_QUERY, WAVE_MAPPER,
    },
    MMSYSERR_ALLOCATED, MMSYSERR_BADDEVICEID, MMSYSERR_NODRIVER, MMSYSERR_NOERROR,
    MMSYSERR_NOTENABLED,
};

pub(super) fn canonical_pcm_format() -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: 1,
        nChannels: 1,
        nSamplesPerSec: 16_000,
        nAvgBytesPerSec: 32_000,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    }
}

/// Query the same mapper/PCM format used by playback without opening a device.
/// This synchronous driver call may block: execute on an existing worker and keep
/// its physical slot until it actually returns, including after cancellation.
/// A successful query is neither an output reservation nor proof of audible sound.
pub fn query_default_output_format() -> Result<DefaultOutputSupport, AudioOutputError> {
    query_using(|device, format, flags| {
        // SAFETY: QUERY does not open a device or retain this stack format. No
        // output-handle destination, callback, or callback instance is supplied.
        unsafe { waveOutOpen(None, device, format, None, None, flags) }
    })
}

fn query_using(
    query: impl FnOnce(u32, &WAVEFORMATEX, MIDI_WAVE_OPEN_TYPE) -> u32,
) -> Result<DefaultOutputSupport, AudioOutputError> {
    let format = canonical_pcm_format();
    match query(WAVE_MAPPER, &format, WAVE_FORMAT_QUERY) {
        MMSYSERR_NOERROR => Ok(DefaultOutputSupport::Supported),
        WAVERR_BADFORMAT => Ok(DefaultOutputSupport::FormatUnsupported),
        MMSYSERR_ALLOCATED | MMSYSERR_BADDEVICEID | MMSYSERR_NODRIVER | MMSYSERR_NOTENABLED
        | WAVERR_SYNC => Ok(DefaultOutputSupport::Unavailable),
        code => Err(AudioOutputError::Driver(code)),
    }
}

#[cfg(test)]
mod tests;
