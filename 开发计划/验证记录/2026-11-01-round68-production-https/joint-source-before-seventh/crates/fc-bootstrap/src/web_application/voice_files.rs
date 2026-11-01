//! Worker-owned incremental voice upload. Network bytes enter as already arrived
//! bounded chunks; no socket Read or file owner crosses to the network pump.
use super::shared_upload::{quarantine_cleanup, PhysicalUploadPermit};
use super::*;
use crate::{
    backend::VoiceService,
    voice_files::{ControlledVoiceFiles, PendingVoiceUpload, VoiceUploadStage},
    web_transfer::{ExecutionFence, TransferLifetime, UploadAck, UploadKind, VOICE_BUDGET},
};
use fc_application::identity::SessionLease;
use fc_domain::voice::*;
use fc_ports::http::{RequestToken, CHUNK_BYTES};
use fc_protocol::web::voice as wire;
use fc_runtime::worker::JobContext;
use std::{
    io::{self, Read},
    path::Path,
    sync::Arc,
    time::Instant,
};
#[cfg(test)]
mod voice_stream_tests;
pub(super) struct VoiceFiles {
    service: VoiceService,
    files: ControlledVoiceFiles,
}
/// Held only in the shared worker's fixed upload registry. The one permit
/// outlives the exact file and every pending validation/publication pin.
pub struct ReceivingWebVoiceUpload {
    binding: Arc<VoiceFiles>,
    stage: Option<VoiceUploadStage>,
    identity: Option<(u32, u64)>,
    lease: SessionLease,
    reservation: VoiceAsset,
    generation: u64,
    expected: u64,
    offset: u64,
    sequence: u64,
    life: TransferLifetime,
    failed: bool,
    handed_off: bool,
    permit: Option<PhysicalUploadPermit>,
}
impl Drop for ReceivingWebVoiceUpload {
    fn drop(&mut self) {
        if !self.handed_off {
            if let Some(permit) = self.permit.take() {
                cleanup_voice(
                    self.binding.clone(),
                    self.reservation.clone(),
                    self.stage.take(),
                    self.identity,
                    permit,
                );
            }
        }
    }
}
/// Opaque proof and exact inode pin. No old per-job fence is retained here.
pub struct PreparedWebVoiceUpload {
    binding: Arc<VoiceFiles>,
    pending: Option<PendingVoiceUpload>,
    lease: SessionLease,
    reservation: VoiceAsset,
    generation: u64,
    life: TransferLifetime,
    identity: (u32, u64),
    attempted: bool,
    _physical_permit: Option<PhysicalUploadPermit>,
}
impl Drop for PreparedWebVoiceUpload {
    fn drop(&mut self) {
        self.pending.take();
        if !self.attempted {
            if let Some(permit) = self._physical_permit.take() {
                cleanup_voice(
                    self.binding.clone(),
                    self.reservation.clone(),
                    None,
                    Some(self.identity),
                    permit,
                );
            }
        }
    }
}
fn unavailable() -> WebResponse {
    failure(503, responses::ErrorCode::ServiceUnavailable)
}
fn unknown() -> WebResponse {
    failure(503, responses::ErrorCode::ResultUnknown)
}
fn invalid() -> WebResponse {
    failure(400, responses::ErrorCode::InvalidRequest)
}
fn active(fence: &ExecutionFence) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fence())).unwrap_or(false)
}
fn cleanup_voice(
    binding: Arc<VoiceFiles>,
    reservation: VoiceAsset,
    mut stage: Option<VoiceUploadStage>,
    mut identity: Option<(u32, u64)>,
    permit: PhysicalUploadPermit,
) {
    let mut cleanup = move || {
        if identity.is_none() {
            if let Some(original) = stage.as_ref() {
                match original.identity() {
                    Ok(saved) => identity = Some(saved),
                    Err(_) => return false,
                }
            }
        }
        stage.take();
        identity.is_none_or(|expected| {
            binding
                .files
                .discard_unpublished_exact(&reservation, expected)
                .is_ok()
        })
    };
    if cleanup() {
        drop(cleanup);
        drop(permit);
    } else {
        quarantine_cleanup(permit, cleanup);
    }
}
fn scoped_service(binding: &VoiceFiles, generation: u64, fence: ExecutionFence) -> VoiceService {
    let original = binding.service.clone();
    binding.service.with_execution_fence(Arc::new(move || {
        active(&fence) && original.generation() == generation && original.is_admitting()
    }))
}
impl ReceivingWebVoiceUpload {
    pub fn token(&self) -> RequestToken {
        self.life.token()
    }
    pub fn asset_id(&self) -> u64 {
        self.reservation.id.get()
    }
    pub fn upload_id(&self) -> u64 {
        self.asset_id()
    }
    pub fn deadline(&self) -> Instant {
        self.life.deadline()
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn expected_bytes(&self) -> u64 {
        self.expected
    }
    /// Shared worker/final coordinator only. Negative cleanup remains possible
    /// after request cancellation without inventing a fresh JobContext. A false
    /// result retains the exact owner/permit; Drop transfers it to quarantine.
    pub fn abort(&mut self) -> bool {
        if self.handed_off {
            return true;
        }
        self.failed = true;
        if self.identity.is_none() {
            if let Some(stage) = self.stage.as_ref() {
                match stage.identity() {
                    Ok(identity) => self.identity = Some(identity),
                    Err(_) => return false,
                }
            }
        }
        self.stage.take();
        if self.identity.is_some_and(|identity| {
            self.binding
                .files
                .discard_unpublished_exact(&self.reservation, identity)
                .is_err()
        }) {
            return false;
        }
        self.handed_off = true;
        self.permit.take();
        true
    }
    fn checkpoint(&self, fence: &ExecutionFence) -> Result<(), WebResponse> {
        if self.failed || !self.life.is_active() || !active(fence) {
            return Err(unknown());
        }
        scoped_service(&self.binding, self.generation, fence.clone())
            .check_web_upload(self.lease, &self.reservation, self.generation)
            .map_err(|_| unknown())?;
        if !self.life.is_active() || !active(fence) {
            return Err(unknown());
        }
        Ok(())
    }
    pub fn write_chunk(
        &mut self,
        token: RequestToken,
        sequence: u64,
        offset: u64,
        bytes: &[u8],
        context: &JobContext,
    ) -> Result<UploadAck, WebResponse> {
        self.write_with_fence(token, sequence, offset, bytes, self.life.job_fence(context))
    }
    fn write_with_fence(
        &mut self,
        token: RequestToken,
        sequence: u64,
        offset: u64,
        bytes: &[u8],
        fence: ExecutionFence,
    ) -> Result<UploadAck, WebResponse> {
        let result = (|| {
            self.checkpoint(&fence)?;
            let next_sequence = self.sequence.checked_add(1).ok_or_else(invalid)?;
            let next_offset = self
                .offset
                .checked_add(bytes.len() as u64)
                .ok_or_else(invalid)?;
            if token != self.token()
                || sequence != next_sequence
                || offset != self.offset
                || bytes.is_empty()
                || bytes.len() > CHUNK_BYTES
                || next_offset > self.expected
            {
                return Err(invalid());
            }
            self.stage
                .as_mut()
                .ok_or_else(invalid)?
                .write_chunk(bytes)
                .map_err(|_| unknown())?;
            self.checkpoint(&fence)?;
            self.offset = next_offset;
            self.sequence = next_sequence;
            Ok(UploadAck::written(
                token,
                UploadKind::Voice,
                self.asset_id(),
                sequence,
                next_offset,
            ))
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn finish(self, context: &JobContext) -> Result<PreparedWebVoiceUpload, WebResponse> {
        let fence = self.life.job_fence(context);
        self.finish_with_fence(fence)
    }
    fn finish_with_fence(
        mut self,
        fence: ExecutionFence,
    ) -> Result<PreparedWebVoiceUpload, WebResponse> {
        self.checkpoint(&fence)?;
        if self.offset != self.expected {
            return Err(invalid());
        }
        let stage = self.stage.take().ok_or_else(invalid)?;
        let authorize = || self.checkpoint(&fence).is_ok();
        let pending = self
            .binding
            .files
            .finish_body_stage(stage, &authorize)
            .map_err(|_| {
                if self.checkpoint(&fence).is_err() {
                    unknown()
                } else {
                    failure(400, responses::ErrorCode::InvalidWav)
                }
            })?;
        if pending.pcm().byte_length != self.expected {
            return Err(invalid());
        }
        self.checkpoint(&fence)?;
        self.handed_off = true;
        Ok(PreparedWebVoiceUpload {
            binding: self.binding.clone(),
            pending: Some(pending),
            lease: self.lease,
            reservation: self.reservation.clone(),
            generation: self.generation,
            life: self.life.clone(),
            identity: self
                .identity
                .expect("original stage identity before finish"),
            attempted: false,
            _physical_permit: Some(self.permit.take().expect("one physical voice owner")),
        })
    }
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
    pub fn begin_voice_upload(
        &self,
        head: &RequestHead,
        life: TransferLifetime,
    ) -> Result<ReceivingWebVoiceUpload, WebResponse> {
        let life = life.bounded(VOICE_BUDGET).map_err(|_| unavailable())?;
        let fence = life.execution_fence();
        self.begin_voice_with_fence(head, life, fence)
    }
    pub fn begin_voice_upload_in_job(
        &self,
        head: &RequestHead,
        life: TransferLifetime,
        context: &JobContext,
    ) -> Result<ReceivingWebVoiceUpload, WebResponse> {
        let life = life.bounded(VOICE_BUDGET).map_err(|_| unavailable())?;
        let fence = life.job_fence(context);
        self.begin_voice_with_fence(head, life, fence)
    }
    fn begin_voice_with_fence(
        &self,
        head: &RequestHead,
        life: TransferLifetime,
        fence: ExecutionFence,
    ) -> Result<ReceivingWebVoiceUpload, WebResponse> {
        if head.token != life.token() || !life.is_active() || !active(&fence) {
            return Err(unavailable());
        }
        let (lease, class) = voice::upload_auth(self, head)?;
        let binding = self
            .voice_files
            .try_lock()
            .map_err(|_| unavailable())?
            .as_ref()
            .cloned()
            .ok_or_else(unavailable)?;
        let generation = binding.service.generation();
        let service = scoped_service(&binding, generation, fence.clone());
        service
            .check_web_target(lease, class, generation)
            .map_err(|e| voice::error(e, false))?;
        let caps = service
            .web_capabilities(lease)
            .map_err(|e| voice::error(e, false))?;
        let expected = head.content_length.ok_or_else(invalid)?;
        if caps.runtime_generation != generation || !caps.admitting {
            return Err(unavailable());
        }
        if expected == 0 || expected > caps.limits.max_wav_bytes() {
            return Err(failure(400, responses::ErrorCode::InvalidWav));
        }
        let permit = PhysicalUploadPermit::take()
            .ok_or_else(|| failure(429, responses::ErrorCode::Capacity))?;
        let reservation = service
            .reserve(lease, class)
            .map_err(|e| voice::error(e, true))?;
        let mut owner = ReceivingWebVoiceUpload {
            binding,
            stage: None,
            identity: None,
            lease,
            reservation,
            generation,
            expected,
            offset: 0,
            sequence: 0,
            life,
            failed: false,
            handed_off: false,
            permit: Some(permit),
        };
        owner.checkpoint(&fence)?;
        owner.stage = Some(
            owner
                .binding
                .files
                .begin_body_stage(owner.reservation.clone())
                .map_err(|_| unknown())?,
        );
        owner.identity = Some(
            owner
                .stage
                .as_ref()
                .expect("created stage")
                .identity()
                .map_err(|_| unknown())?,
        );
        owner.checkpoint(&fence)?;
        Ok(owner)
    }
    /// Finite local File/Cursor only; never a potentially blocking socket Read.
    pub fn prepare_voice_upload<R: Read>(
        &self,
        head: &RequestHead,
        mut input: R,
    ) -> Result<PreparedWebVoiceUpload, WebResponse> {
        let life = TransferLifetime::local(head.token, VOICE_BUDGET).map_err(|_| unavailable())?;
        let mut owner = self.begin_voice_upload(head, life)?;
        let mut buffer = [0; CHUNK_BYTES];
        loop {
            let fence = owner.life.execution_fence();
            owner.checkpoint(&fence)?;
            let remaining = owner.expected - owner.offset;
            let limit =
                usize::try_from(remaining.min(CHUNK_BYTES as u64)).map_err(|_| invalid())?;
            if limit == 0 {
                let mut extra = [0; 1];
                if input.read(&mut extra).map_err(|_| invalid())? != 0 {
                    return Err(invalid());
                }
                owner.checkpoint(&fence)?;
                return owner.finish_with_fence(fence);
            }
            let bytes = input.read(&mut buffer[..limit]).map_err(|_| invalid())?;
            owner.checkpoint(&fence)?;
            if bytes == 0 {
                return Err(invalid());
            }
            owner.write_with_fence(
                head.token,
                owner.sequence + 1,
                owner.offset,
                &buffer[..bytes],
                fence,
            )?;
        }
    }
    pub fn commit_voice_upload(&self, prepared: PreparedWebVoiceUpload) -> WebResponse {
        let fence = prepared.life.execution_fence();
        self.commit_voice_with_fence(prepared, fence)
    }
    pub fn commit_voice_upload_in_job(
        &self,
        prepared: PreparedWebVoiceUpload,
        context: &JobContext,
    ) -> WebResponse {
        let fence = prepared.life.job_fence(context);
        self.commit_voice_with_fence(prepared, fence)
    }
    fn commit_voice_with_fence(
        &self,
        mut prepared: PreparedWebVoiceUpload,
        fence: ExecutionFence,
    ) -> WebResponse {
        let binding = self
            .voice_files
            .try_lock()
            .ok()
            .and_then(|configured| configured.as_ref().cloned());
        if !binding
            .as_ref()
            .is_some_and(|b| Arc::ptr_eq(b, &prepared.binding))
        {
            return unavailable();
        }
        let service = scoped_service(&prepared.binding, prepared.generation, fence.clone());
        if !prepared.life.is_active()
            || service
                .check_web_upload(prepared.lease, &prepared.reservation, prepared.generation)
                .is_err()
            || !active(&fence)
        {
            return unknown();
        }
        let pending = prepared
            .pending
            .take()
            .expect("opaque pending voice proof consumed once");
        let (proof, _pin) = pending.into_parts();
        prepared.attempted = true;
        match service.publish(prepared.lease, proof) {
            Ok(asset) => {
                if service
                    .check_web_upload(prepared.lease, &asset, prepared.generation)
                    .is_err()
                    || !prepared.life.is_active()
                    || !active(&fence)
                {
                    return unknown();
                }
                match wire::uploaded(&asset) {
                    Ok(body) => response(201, Ok(body), None),
                    Err(_) => unknown(),
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
