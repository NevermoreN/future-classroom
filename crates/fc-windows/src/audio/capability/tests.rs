use super::*;
use windows::Win32::Media::{MMSYSERR_ERROR, MMSYSERR_INVALPARAM, MMSYSERR_NOMEM};

#[test]
fn query_uses_only_mapper_query_and_exact_playback_pcm() {
    let mut calls = 0;
    let result = query_using(|device, format, flags| {
        calls += 1;
        assert_eq!(device, WAVE_MAPPER);
        assert_eq!(flags, WAVE_FORMAT_QUERY);
        // Copy packed fields before asserting; no unaligned references.
        let actual = (
            format.wFormatTag,
            format.nChannels,
            format.nSamplesPerSec,
            format.nAvgBytesPerSec,
            format.nBlockAlign,
            format.wBitsPerSample,
            format.cbSize,
        );
        assert_eq!(actual, (1, 1, 16_000, 32_000, 2, 16, 0));
        MMSYSERR_NOERROR
    });
    assert_eq!(calls, 1);
    assert_eq!(result, Ok(DefaultOutputSupport::Supported));
}

#[test]
fn query_distinguishes_unsupported_format_from_unavailable_output() {
    assert_eq!(
        query_using(|_, _, _| WAVERR_BADFORMAT),
        Ok(DefaultOutputSupport::FormatUnsupported)
    );
    for code in [
        MMSYSERR_ALLOCATED,
        MMSYSERR_BADDEVICEID,
        MMSYSERR_NODRIVER,
        MMSYSERR_NOTENABLED,
        WAVERR_SYNC,
    ] {
        assert_eq!(
            query_using(|_, _, _| code),
            Ok(DefaultOutputSupport::Unavailable)
        );
    }
}

#[test]
fn query_preserves_real_driver_and_resource_failures() {
    for code in [
        MMSYSERR_ERROR,
        MMSYSERR_NOMEM,
        MMSYSERR_INVALPARAM,
        u32::MAX,
    ] {
        assert_eq!(
            query_using(|_, _, _| code),
            Err(AudioOutputError::Driver(code))
        );
    }
}
