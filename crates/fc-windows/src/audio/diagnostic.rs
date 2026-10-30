use super::*;
use std::{fs, io::Write, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinMmProbeReport {
    pub natural_completions: u32,
    pub canceled_completions: u32,
    pub submitted_pcm_bytes: u32,
    pub pcm_allocation_bytes: usize,
    pub source_cleanup: bool,
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Explicit opt-in diagnostic. Generated PCM is entirely zero, and no volume,
/// routing, recording, other process, or foreground state is changed. No-device
/// returns DeviceUnavailable for the caller to record NOT_RUN, never fake PASS.
pub fn silent_probe() -> Result<WinMmProbeReport, Error> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Io)?
        .as_nanos();
    let dir = Temporary(std::env::temp_dir().join(format!(
        "FutureClassroom-Test-WinMM-{}-{nonce}",
        std::process::id()
    )));
    fs::create_dir(&dir.0).map_err(|_| Error::Io)?;
    let path = dir.0.join("silent.wav");
    // 2.048 seconds exercises all three buffers AND stream refilling; do not
    // allocate the 64 KiB body, even in the actual device diagnostic.
    let bytes = 65_536u32;
    let mut h = [0u8; 44];
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
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| Error::Io)?;
        file.write_all(&h).map_err(|_| Error::Io)?;
        for _ in 0..16 {
            file.write_all(&[0u8; 4096]).map_err(|_| Error::Io)?;
        }
        file.sync_all().map_err(|_| Error::Io)?;
    }
    let mut output = WinMmAudioOutput::new();
    let mut authorization = || true;
    output.start(VerifiedWave::open(&path)?, 1, &mut authorization)?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match output.poll(50, &mut authorization)? {
            Progress::NaturallyCompleted { generation: 1 } => break,
            Progress::Playing { generation: 1 } if Instant::now() < deadline => {}
            _ => return Err(Error::CleanupPending),
        }
    }
    output.start(VerifiedWave::open(&path)?, 2, &mut authorization)?;
    if output.poll(0, &mut authorization)? != (Progress::Playing { generation: 2 }) {
        return Err(Error::CleanupPending);
    }
    // A stop is not natural completion even if reset returns WHDR_DONE immediately.
    let mut progress = output.stop(2)?;
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match progress {
            Progress::Released { generation: 2 } => break,
            Progress::Playing { generation: 2 } if Instant::now() < deadline => {
                progress = output.poll(50, &mut authorization)?;
            }
            _ => return Err(Error::CleanupPending),
        }
    }
    drop(output);
    fs::remove_file(&path).map_err(|_| Error::Io)?;
    fs::remove_dir(&dir.0).map_err(|_| Error::Io)?;
    Ok(WinMmProbeReport {
        natural_completions: 1,
        canceled_completions: 0,
        submitted_pcm_bytes: bytes,
        pcm_allocation_bytes: BUFFER_BYTES * BUFFER_COUNT,
        source_cleanup: !dir.0.exists(),
    })
}
