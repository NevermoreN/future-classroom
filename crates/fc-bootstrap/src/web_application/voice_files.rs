//! Voice file worker over already-arrived finite local bytes. This adapter does
//! not prove cancellation/deadlines of a blocking socket Read; the disabled
//! transport's bounded physical staging/event scheduling remains a separate gate.
use super::shared_upload::PhysicalUploadPermit;
use super::*;
use crate::{
    backend::VoiceService,
    voice_files::{ControlledVoiceFiles, PendingVoiceUpload},
};
use fc_application::identity::SessionLease;
use fc_domain::voice::*;
use fc_protocol::web::voice as wire;
use std::{
    io::{self, Read},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
pub(super) struct VoiceFiles {
    service: VoiceService,
    files: ControlledVoiceFiles,
}
/// No Clone/Deserialize or public constructor. The physical permit survives the
/// old application and every validation/SQL/pin phase until the real owner drops.
pub struct PreparedWebVoiceUpload {
    binding: Arc<VoiceFiles>,
    pending: Option<PendingVoiceUpload>,
    lease: SessionLease,
    reservation: VoiceAsset,
    generation: u64,
    attempted: bool,
    _physical_permit: PhysicalUploadPermit,
}
impl Drop for PreparedWebVoiceUpload {
    fn drop(&mut self) {
        self.pending.take();
        if !self.attempted {
            let _ = self.binding.files.discard_unpublished(&self.reservation);
        }
    }
}
fn unavailable() -> WebResponse {
    failure(503, responses::ErrorCode::ServiceUnavailable)
}
impl WebApplication {
    pub fn configure_voice_files(&mut self, backend: &Backend, root: &Path) -> io::Result<()> {
        if !self.service.same_instance(&backend.web_service()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "voice backend binding",
            ));
        }
        let mut configured = self
            .voice_files
            .lock()
            .map_err(|_| io::Error::other("voice files unavailable"))?;
        if configured.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "voice files configured",
            ));
        }
        *configured = Some(Arc::new(VoiceFiles {
            service: backend.voice_service(),
            files: ControlledVoiceFiles::new(root)?,
        }));
        Ok(())
    }
    pub fn prepare_voice_upload<R: Read>(
        &self,
        head: &RequestHead,
        input: R,
    ) -> Result<PreparedWebVoiceUpload, WebResponse> {
        let (lease, class) = voice::upload_auth(self, head)?;
        let binding = self
            .voice_files
            .try_lock()
            .map_err(|_| unavailable())?
            .as_ref()
            .cloned()
            .ok_or_else(unavailable)?;
        let generation = binding.service.generation();
        binding
            .service
            .check_web_target(lease, class, generation)
            .map_err(|e| voice::error(e, false))?;
        let permit = PhysicalUploadPermit::take()
            .ok_or_else(|| failure(429, responses::ErrorCode::Capacity))?;
        let reservation = binding
            .service
            .reserve(lease, class)
            .map_err(|e| voice::error(e, true))?;
        let expected = head
            .content_length
            .ok_or_else(|| failure(400, responses::ErrorCode::InvalidWav))?;
        let started = Instant::now();
        let authorize = || {
            started.elapsed() < Duration::from_secs(30)
                && binding
                    .service
                    .check_web_upload(lease, &reservation, generation)
                    .is_ok()
        };
        // Read limit caps bytes before the trusted WAV adapter; one extra byte is
        // allowed only to reject framing mismatch, never to publish a larger object.
        struct Framed<R> {
            input: R,
            remaining: u64,
            read: u64,
            expected: u64,
        }
        impl<R: Read> Read for Framed<R> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if out.is_empty() {
                    return Ok(0);
                }
                if self.remaining == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "voice framing exceeded",
                    ));
                }
                let limit = out.len().min(self.remaining as usize);
                let n = self.input.read(&mut out[..limit])?;
                self.remaining -= n as u64;
                self.read += n as u64;
                if n == 0 && self.read != self.expected {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "voice framing truncated",
                    ));
                }
                Ok(n)
            }
        }
        let framed = Framed {
            input,
            remaining: expected + 1,
            read: 0,
            expected,
        };
        let pending = match binding
            .files
            .prepare_body(framed, reservation.clone(), &authorize)
        {
            Ok(p) => p,
            Err(_) => {
                let _ = binding.files.discard_unpublished(&reservation);
                if !authorize() {
                    return Err(failure(503, responses::ErrorCode::ResultUnknown));
                }
                return Err(failure(400, responses::ErrorCode::InvalidWav));
            }
        };
        if pending_published_bytes(&pending) != expected {
            drop(pending);
            let _ = binding.files.discard_unpublished(&reservation);
            return Err(failure(400, responses::ErrorCode::InvalidWav));
        }
        Ok(PreparedWebVoiceUpload {
            binding,
            pending: Some(pending),
            lease,
            reservation,
            generation,
            attempted: false,
            _physical_permit: permit,
        })
    }
    pub fn commit_voice_upload(&self, mut prepared: PreparedWebVoiceUpload) -> WebResponse {
        let binding = match self.voice_files.try_lock() {
            Ok(g) => g.as_ref().cloned(),
            Err(_) => None,
        };
        if !binding
            .as_ref()
            .is_some_and(|b| Arc::ptr_eq(b, &prepared.binding))
        {
            return unavailable();
        }
        if prepared
            .binding
            .service
            .check_web_upload(prepared.lease, &prepared.reservation, prepared.generation)
            .is_err()
        {
            return failure(503, responses::ErrorCode::ResultUnknown);
        }
        let pending = prepared
            .pending
            .take()
            .expect("opaque pending voice proof consumed once");
        let (proof, _pin) = pending.into_parts();
        prepared.attempted = true;
        match prepared.binding.service.publish(prepared.lease, proof) {
            Ok(asset) => {
                if prepared
                    .binding
                    .service
                    .check_web_upload(prepared.lease, &asset, prepared.generation)
                    .is_err()
                {
                    return failure(503, responses::ErrorCode::ResultUnknown);
                }
                match wire::uploaded(&asset) {
                    Ok(body) => response(201, Ok(body), None),
                    Err(_) => failure(503, responses::ErrorCode::ResultUnknown),
                }
            }
            Err(error) => voice::error(error, true),
        }
    }
    pub fn dispatch_voice_upload<R: Read>(&self, head: &RequestHead, input: R) -> WebResponse {
        match self.prepare_voice_upload(head, input) {
            Ok(prepared) => self.commit_voice_upload(prepared),
            Err(response) => response,
        }
    }
}
// The opaque pending proof exposes no audio bytes. Its validated PCM descriptor
// is used only to enforce the transport's exact finite Content-Length.
fn pending_published_bytes(pending: &PendingVoiceUpload) -> u64 {
    pending.pcm().byte_length
}
