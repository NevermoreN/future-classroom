//! Authenticated voice ports and the existing native execution domain, silence only.
use super::*;
use fc_application::identity::SessionLease;
use fc_bootstrap::{
    backend::VoiceService,
    voice_runtime::{NativeFactory, RuntimePhase, VoiceRuntime},
    web_application::WebResponse,
};
use fc_domain::voice::{VoiceJobId, VoiceState};
use std::{io::Cursor, path::Path, time::Instant};

fn text(response: &WebResponse) -> &str {
    std::str::from_utf8(&response.body).unwrap_or("")
}
fn field(response: &WebResponse, name: &str) -> Result<u64, String> {
    text(response)
        .split(&format!("\"{name}\":\""))
        .nth(1)
        .and_then(|s| s.split('"').next())
        .and_then(|s| s.parse().ok())
        .filter(|n| *n != 0)
        .ok_or_else(|| format!("voice response missing {name}"))
}
fn silence(frames: u32) -> Vec<u8> {
    let data = frames * 2;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend(b"RIFF");
    bytes.extend((data + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(16_000u32.to_le_bytes());
    bytes.extend(32_000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.resize(44 + data as usize, 0);
    bytes
}
struct RuntimeOwner {
    runtime: VoiceRuntime,
    service: VoiceService,
}
impl RuntimeOwner {
    fn close(&self) -> Result<(), String> {
        check(self.service.clear_waker())?;
        self.runtime.shutdown();
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            check(self.runtime.reap_nonblocking())?;
            let status = check(self.runtime.status())?;
            if status.phase == RuntimePhase::Closed
                && !status.faulted
                && !status.physical_thread_running
                && status.ack_generation >= status.stop_generation
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err("voice physical runtime shutdown did not drain".into())
    }
}
impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
struct Ports<'a> {
    backend: &'a mut Backend,
    app: &'a WebApplication,
    cookie: &'a str,
    csrf: &'a str,
}
impl Ports<'_> {
    fn json(&mut self, method: &str, path: &str, bytes: &[u8]) -> Result<WebResponse, String> {
        Ok(self.app.dispatch(
            self.backend,
            &head(method, path, bytes, Some(self.cookie), Some(self.csrf))?,
            bytes,
        ))
    }
    fn upload(&self, wav: &[u8], class: u64, csrf: &str) -> Result<WebResponse, String> {
        let mut request = head(
            "POST",
            &format!("voice/uploads?classId={class}"),
            wav,
            Some(self.cookie),
            Some(csrf),
        )?;
        request
            .headers
            .iter_mut()
            .find(|h| h.name == "Content-Type")
            .ok_or("missing voice type")?
            .value = b"audio/wav".to_vec();
        Ok(self.app.dispatch_voice_upload(&request, Cursor::new(wav)))
    }
    fn submit(
        &mut self,
        uploaded: &WebResponse,
        command: u64,
        total: u8,
    ) -> Result<Vec<u8>, String> {
        let asset = field(uploaded, "assetId")?;
        let revision = field(uploaded, "assetRevision")?;
        let bytes = format!(r#"{{"schemaVersion":1,"commandId":"{command}","assetId":"{asset}","expectedAssetRevision":"{revision}","classId":"7","repeatTotal":{total}}}"#).into_bytes();
        let response = self.json("POST", "voice/broadcasts", &bytes)?;
        require(
            response.head.status == 200
                && text(&response).contains("\"COMMITTED\"")
                && field(&response, "jobId")? == command,
            &format!(
                "voice submit failed {} {}",
                response.head.status,
                text(&response)
            ),
        )?;
        Ok(bytes)
    }
}
pub(super) fn run(
    backend: &mut Backend,
    app: &mut WebApplication,
    root: &Path,
    cookie: &str,
    csrf: &str,
    lease: SessionLease,
    clock: Arc<CurrentClock>,
) -> Result<(), String> {
    check(app.configure_voice_files(backend, root))?;
    let service = backend.voice_service();
    let owner = RuntimeOwner {
        runtime: VoiceRuntime::new(Arc::new(NativeFactory::new(
            service.clone(),
            root.to_owned(),
            clock,
        ))),
        service: service.clone(),
    };
    check(service.set_waker(owner.runtime.weak_waker()))?;
    check(owner.runtime.set_available(true))?;
    let deadline = Instant::now() + Duration::from_secs(8);
    while !service.is_admitting() || check(owner.runtime.status())?.physical_thread_running {
        check(owner.runtime.reap_nonblocking())?;
        require(Instant::now() < deadline, "voice startup fence timeout")?;
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut ports = Ports {
        backend,
        app,
        cookie,
        csrf,
    };
    let capabilities = ports.json("GET", "voice/capabilities", b"")?;
    require(
        capabilities.head.status == 200
            && text(&capabilities).contains("VOICE_CAPABILITIES")
            && text(&capabilities).contains("UNVERIFIED"),
        "voice capability/device truth boundary failed",
    )?;
    let wav = silence(8_000);
    let mut wrong = csrf.as_bytes().to_vec();
    wrong[0] = if wrong[0] == b'a' { b'b' } else { b'a' };
    require(
        ports
            .upload(&wav, 7, check(std::str::from_utf8(&wrong))?)?
            .head
            .status
            == 403,
        "voice upload bypassed CSRF",
    )?;
    let mut invalid = wav.clone();
    invalid[22] = 2;
    require(
        ports.upload(&invalid, 7, csrf)?.head.status == 400,
        "voice invalid WAV accepted",
    )?;
    require(
        ports.upload(&wav, 8, csrf)?.head.status == 403,
        "voice unmanaged target accepted",
    )?;
    let uploaded = ports.upload(&wav, 7, csrf)?;
    require(
        uploaded.head.status == 201,
        &format!(
            "voice upload failed {} {}",
            uploaded.head.status,
            text(&uploaded)
        ),
    )?;
    let original = ports.submit(&uploaded, 99_001, 3)?;
    let id = check(VoiceJobId::new(99_001))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        check(owner.runtime.reap_nonblocking())?;
        let job = check(service.status(lease, id))?;
        if job.state.terminal() {
            require(
                job.state == VoiceState::Completed && job.completed_repeats == 3,
                &format!("voice natural count failed: {job:?}"),
            )?;
            break;
        }
        require(
            Instant::now() < deadline,
            "voice natural completion timeout",
        )?;
        std::thread::sleep(Duration::from_millis(20));
    }
    let replay = ports.json("POST", "voice/broadcasts", &original)?;
    require(
        replay.head.status == 200 && text(&replay).contains("COMPLETED"),
        "voice duplicate restarted old task",
    )?;
    let current = ports.json("GET", "voice/broadcasts/99001", b"")?;
    require(
        current.head.status == 200 && text(&current).contains("COMPLETED"),
        "voice GET original command failed",
    )?;
    let second = ports.upload(&silence(16_000), 7, csrf)?;
    require(second.head.status == 201, "voice second upload failed")?;
    ports.submit(&second, 99_002, 10)?;
    let second_id = check(VoiceJobId::new(99_002))?;
    let deadline = Instant::now() + Duration::from_secs(8);
    let playing = loop {
        check(owner.runtime.reap_nonblocking())?;
        let job = check(service.status(lease, second_id))?;
        if job.state == VoiceState::Playing {
            break job;
        }
        require(
            !job.state.terminal() && Instant::now() < deadline,
            "voice did not reach actual first write",
        )?;
        std::thread::sleep(Duration::from_millis(10));
    };
    let cancel = format!(
        r#"{{"schemaVersion":1,"commandId":"99003","expectedRevision":"{}"}}"#,
        playing.revision.get()
    );
    let stopped = ports.json("POST", "voice/broadcasts/99002/cancel", cancel.as_bytes())?;
    require(
        stopped.head.status == 200 && text(&stopped).contains("CANCEL_REQUESTED"),
        "voice cancellation not admitted",
    )?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        check(owner.runtime.reap_nonblocking())?;
        let job = check(service.status(lease, second_id))?;
        if job.state.terminal() {
            require(
                job.state == VoiceState::Canceled && job.completed_repeats < 10,
                "voice cancel DONE counted remaining repeats",
            )?;
            break;
        }
        require(Instant::now() < deadline, "voice native return timeout")?;
        std::thread::sleep(Duration::from_millis(10));
    }
    let list = ports.json("GET", "voice/broadcasts", b"")?;
    require(
        list.head.status == 200
            && text(&list).contains("VOICE_JOBS")
            && text(&list).contains("99001")
            && text(&list).contains("99002"),
        "voice list lost owned recent tasks",
    )?;
    owner.close()?;
    require(
        !check(owner.runtime.status())?.physical_thread_running,
        "voice physical runtime still alive",
    )?;
    drop(owner);
    require(
        !service.is_admitting(),
        "closed voice runtime still admitted",
    )?;
    let after = ports.json("GET", "voice/broadcasts/99002", b"")?;
    require(
        after.head.status == 200 && text(&after).contains("CANCELED"),
        "closed admission hid terminal voice status",
    )?;
    println!("WEB_VOICE_PASS real_DPAPI_SQLite_exact_Web_target_CSRF_canonical_WAV_upload_native_zero_PCM_three_repeats_query_cancel_driver_return=PASS synthetic_request_metadata=true actual_microphone=NOT_RUN listener_TLS_browser=NOT_RUN");
    Ok(())
}
