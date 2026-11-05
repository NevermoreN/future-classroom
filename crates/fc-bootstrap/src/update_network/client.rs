use super::wire::{Expected, ResponseParser};
use super::*;
use fc_windows::tls_frontdoor::{UpdatePeerIdentity, UpdateTlsCall, UpdateTlsStep};
#[derive(Clone, Copy)]
pub(super) enum FetchPurpose {
    Metadata,
    Blob(VerifiedTarget),
}
#[derive(Clone)]
enum Resource {
    Catalog,
    Metadata(String),
    Blob(VerifiedTarget, u64),
}
impl Resource {
    fn path(&self) -> String {
        match self {
            Self::Catalog => "/update/v1/catalog".into(),
            Self::Metadata(name) => format!("/update/v1/metadata/{name}"),
            Self::Blob(t, _) => format!("/update/v1/blobs/{}", wire::hex(&t.sha256)),
        }
    }
    fn range(&self) -> String {
        match self {
            Self::Blob(_, offset) if *offset > 0 => format!("bytes={offset}-"),
            _ => String::new(),
        }
    }
    fn expected(&self) -> Expected {
        match self {
            Self::Catalog => Expected::Catalog,
            Self::Metadata(_) => Expected::Metadata,
            Self::Blob(target, offset) => Expected::Blob {
                target: *target,
                offset: *offset,
            },
        }
    }
}
enum DiskState {
    None,
    Begin,
    WaitBegin,
    Append(Vec<u8>),
    WaitAppend(u64),
    Finish,
    WaitFinish,
}
pub(super) struct Client {
    pub peer: Peer,
    pub purpose: FetchPurpose,
    session: Option<Session>,
    resource: Resource,
    challenge_request: bool,
    nonce: [u8; 16],
    challenge: [u8; 16],
    body: Vec<u8>,
    metadata: OwnedMetadata,
    names: VecDeque<String>,
    total_metadata: usize,
    handle: Option<u64>,
    offset: u64,
    disk: DiskState,
    pub done: bool,
    pub failed: bool,
    attempt: u8,
    stopped: bool,
    last_body: bool,
}
impl Client {
    fn new(peer: Peer, purpose: FetchPurpose) -> Self {
        let blob = match purpose {
            FetchPurpose::Blob(t) => Some(t),
            _ => None,
        };
        Self {
            peer,
            purpose,
            session: None,
            resource: blob.map_or(Resource::Catalog, |t| Resource::Blob(t, 0)),
            challenge_request: true,
            nonce: [0; 16],
            challenge: [0; 16],
            body: Vec::new(),
            metadata: OwnedMetadata {
                roots: Vec::new(),
                timestamp: Vec::new(),
                snapshot: Vec::new(),
                targets: Vec::new(),
            },
            names: VecDeque::new(),
            total_metadata: 0,
            handle: None,
            offset: 0,
            disk: if blob.is_some() {
                DiskState::Begin
            } else {
                DiskState::None
            },
            done: false,
            failed: false,
            attempt: 0,
            stopped: false,
            last_body: false,
        }
    }
    pub fn metadata(peer: Peer) -> Self {
        Self::new(peer, FetchPurpose::Metadata)
    }
    pub fn blob(peer: Peer, target: VerifiedTarget) -> Self {
        Self::new(peer, FetchPurpose::Blob(target))
    }
    pub fn stop(&mut self, host: &mut TlsHttpHost) {
        self.stopped = true;
        if let Some(s) = &mut self.session {
            s.close(host);
        }
    }
    fn fail(
        &mut self,
        host: &mut TlsHttpHost,
        control: &NetworkControl,
        handles: &mut VecDeque<u64>,
    ) {
        self.failed = true;
        self.stopped = true;
        if let Some(s) = &mut self.session {
            s.close(host);
        }
        if let Some(handle) = self.handle.take() {
            handles.push_back(handle);
        }
        control.event(NetworkEvent::Status(
            "内网更新连接中断，保留已下载内容等待重试",
        ));
    }
    pub fn receipt(
        &mut self,
        result: Result<NetworkIoResult, String>,
        control: &NetworkControl,
        handles: &mut VecDeque<u64>,
    ) -> Result<(), String> {
        match result {
            Ok(NetworkIoResult::Opened { handle, offset }) => {
                if !matches!(self.disk, DiskState::WaitBegin) {
                    handles.push_back(handle);
                    return Err("unexpected update download owner".into());
                }
                let FetchPurpose::Blob(target) = self.purpose else {
                    handles.push_back(handle);
                    return Err("unexpected update download".into());
                };
                if offset > target.length {
                    handles.push_back(handle);
                    return Err("download prefix exceeds target".into());
                }
                self.handle = Some(handle);
                self.offset = offset;
                self.resource = Resource::Blob(target, offset);
                self.disk = if offset == target.length {
                    DiskState::Finish
                } else {
                    DiskState::None
                };
            }
            Ok(NetworkIoResult::Written { offset }) => {
                let DiskState::WaitAppend(expected) = self.disk else {
                    return Err("unexpected update write receipt".into());
                };
                if offset != expected {
                    return Err("update file write offset mismatch".into());
                }
                self.offset = offset;
                if let FetchPurpose::Blob(target) = self.purpose {
                    control.event(NetworkEvent::Progress {
                        received: offset,
                        total: target.length,
                    });
                }
                self.disk = DiskState::None;
            }
            Ok(NetworkIoResult::Finished) => {
                if !matches!(self.disk, DiskState::WaitFinish) {
                    return Err("unexpected update completion".into());
                }
                self.handle = None;
                self.disk = DiskState::None;
                self.done = true;
                if let FetchPurpose::Blob(target) = self.purpose {
                    control.event(NetworkEvent::Downloaded { target });
                }
            }
            Ok(NetworkIoResult::Closed) => (),
            Ok(_) => return Err("unexpected update file receipt".into()),
            Err(_) => {
                self.disk = DiskState::None;
                self.failed = true;
                self.stopped = true;
                if let Some(h) = self.handle.take() {
                    handles.push_back(h);
                }
                control.event(NetworkEvent::Status("内网更新文件处理失败，保留原版本"));
            }
        }
        Ok(())
    }
    fn operation(&mut self) -> Option<NetworkOperation> {
        match std::mem::replace(&mut self.disk, DiskState::None) {
            DiskState::Begin => {
                let FetchPurpose::Blob(target) = self.purpose else {
                    return None;
                };
                self.disk = DiskState::WaitBegin;
                Some(NetworkOperation::BeginDownload { target })
            }
            DiskState::Append(bytes) => {
                let handle = self.handle?;
                let end = self.offset.checked_add(bytes.len() as u64)?;
                self.disk = DiskState::WaitAppend(end);
                Some(NetworkOperation::AppendDownload {
                    handle,
                    offset: self.offset,
                    bytes,
                })
            }
            DiskState::Finish => {
                let handle = self.handle?;
                self.disk = DiskState::WaitFinish;
                Some(NetworkOperation::FinishDownload { handle })
            }
            state => {
                self.disk = state;
                None
            }
        }
    }
    pub fn poll(
        &mut self,
        host: &mut TlsHttpHost,
        configuration: &NetworkConfiguration,
        control: &NetworkControl,
        handles: &mut VecDeque<u64>,
        may_work: bool,
    ) -> Result<Option<NetworkOperation>, String> {
        if self.stopped {
            if let Some(s) = &mut self.session {
                s.close(host);
                if s.retired(host)? {
                    self.session = None;
                }
            }
            if let Some(h) = self.handle.take() {
                handles.push_back(h);
            }
            self.done = self.session.is_none()
                && !matches!(
                    self.disk,
                    DiskState::WaitBegin | DiskState::WaitAppend(_) | DiskState::WaitFinish
                );
            return Ok(None);
        }
        if may_work {
            if let Some(op) = self.operation() {
                return Ok(Some(op));
            }
        }
        if !matches!(self.disk, DiskState::None) {
            return Ok(None);
        }
        if self.done {
            return Ok(None);
        }
        if self.session.is_none() {
            if self.last_body {
                self.finish_resource(control, configuration)?;
                if self.done {
                    return Ok(None);
                }
            }
            if matches!(self.disk, DiskState::Finish) {
                return Ok(None);
            }
            if self.challenge_request {
                self.nonce = random()?;
                self.challenge = [0; 16];
            }
            let path = if self.challenge_request {
                "/update/v1/challenge".into()
            } else {
                self.resource.path()
            };
            let range = if self.challenge_request {
                String::new()
            } else {
                self.resource.range()
            };
            let request = wire::request(
                configuration,
                self.peer,
                self.nonce,
                self.challenge,
                &path,
                &range,
            );
            let expected = if self.challenge_request {
                Expected::Challenge
            } else {
                self.resource.expected()
            };
            if !host.update_fetch_available() {
                return Ok(None);
            }
            match Session::new(host, self.peer, request, expected) {
                Ok(s) => self.session = Some(s),
                Err(_) => {
                    self.fail(host, control, handles);
                    return Ok(None);
                }
            }
        }
        let rate = if control.0.lock().is_ok_and(|s| s.teaching_active) {
            1024 * 1024
        } else {
            4 * 1024 * 1024
        };
        let result = self.session.as_mut().unwrap().poll(host, rate);
        match result {
            Ok(Some(bytes)) => {
                if matches!(self.resource, Resource::Blob(..)) && !self.challenge_request {
                    if bytes.len() > CHUNK {
                        return Err("update file chunk bound".into());
                    }
                    self.disk = DiskState::Append(bytes);
                } else {
                    let max = if self.challenge_request {
                        104
                    } else {
                        match self.resource {
                            Resource::Catalog => 4096,
                            _ => META,
                        }
                    };
                    if self.body.len() + bytes.len() > max {
                        return Err("update response memory bound".into());
                    }
                    self.body.extend_from_slice(&bytes);
                }
            }
            Ok(None) => (),
            Err(_) => {
                self.fail(host, control, handles);
                return Ok(None);
            }
        }
        if self.session.as_ref().is_some_and(|s| s.complete) {
            self.last_body = true;
        }
        if self.session.as_mut().is_some_and(|s| s.closing)
            && self.session.as_mut().unwrap().retired(host)?
        {
            self.session = None;
            if matches!(self.disk, DiskState::None) {
                self.finish_resource(control, configuration)?;
            }
        }
        Ok(None)
    }
    fn finish_resource(
        &mut self,
        control: &NetworkControl,
        configuration: &NetworkConfiguration,
    ) -> Result<(), String> {
        self.last_body = false;
        if self.challenge_request {
            self.challenge =
                wire::parse_challenge(configuration, self.peer, self.nonce, &self.body)?;
            self.body.clear();
            self.challenge_request = false;
            return Ok(());
        }
        match &self.resource {
            Resource::Catalog => {
                let roots = wire::root_versions(&self.body)?;
                self.names = roots
                    .into_iter()
                    .map(|n| format!("{n}.root.json"))
                    .chain([
                        "timestamp.json".into(),
                        "snapshot.json".into(),
                        "targets.json".into(),
                    ])
                    .collect();
            }
            Resource::Metadata(name) => {
                self.total_metadata += self.body.len();
                if self.total_metadata > META_TOTAL {
                    return Err("metadata total bound".into());
                }
                let bytes = std::mem::take(&mut self.body);
                match name.as_str() {
                    "timestamp.json" => self.metadata.timestamp = bytes,
                    "snapshot.json" => self.metadata.snapshot = bytes,
                    "targets.json" => self.metadata.targets = bytes,
                    _ => {
                        let version = name
                            .strip_suffix(".root.json")
                            .ok_or("metadata filename")?
                            .parse()
                            .map_err(|_| "root filename")?;
                        self.metadata.roots.push((version, bytes));
                    }
                }
            }
            Resource::Blob(target, _) => {
                if self.offset != target.length {
                    return Err("update body/file receipt mismatch".into());
                }
                self.disk = DiskState::Finish;
                return Ok(());
            }
        }
        self.body.clear();
        self.challenge_request = true;
        if let Some(name) = self.names.pop_front() {
            self.resource = Resource::Metadata(name);
        } else {
            let metadata = std::mem::replace(
                &mut self.metadata,
                OwnedMetadata {
                    roots: Vec::new(),
                    timestamp: Vec::new(),
                    snapshot: Vec::new(),
                    targets: Vec::new(),
                },
            );
            control.event(NetworkEvent::Metadata {
                peer: self.peer,
                metadata,
            });
            self.done = true;
        }
        Ok(())
    }
}
struct Session {
    token: RequestToken,
    request: Vec<u8>,
    parser: ResponseParser,
    started: Instant,
    last_activity: Instant,
    connected: bool,
    handshake_started: bool,
    handshake_complete: bool,
    request_sent: bool,
    pending_tls: bool,
    cipher: Vec<u8>,
    wire: Vec<u8>,
    wire_pos: usize,
    need_read: bool,
    pub complete: bool,
    pub closing: bool,
    next_read: Instant,
}
impl Session {
    fn new(
        host: &mut TlsHttpHost,
        peer: Peer,
        request: Vec<u8>,
        expected: Expected,
    ) -> Result<Self, String> {
        let token = host.begin_update_fetch(
            UpdatePeerIdentity {
                address: peer.address,
                certificate_sha256: peer.certificate_sha256,
            },
            peer.port,
        )?;
        Ok(Self {
            token,
            request,
            parser: ResponseParser::new(expected),
            started: Instant::now(),
            last_activity: Instant::now(),
            connected: false,
            handshake_started: false,
            handshake_complete: false,
            request_sent: false,
            pending_tls: false,
            cipher: Vec::new(),
            wire: Vec::new(),
            wire_pos: 0,
            need_read: false,
            complete: false,
            closing: false,
            next_read: Instant::now(),
        })
    }
    fn close(&mut self, host: &mut TlsHttpHost) {
        if !self.closing {
            let _ = host.close_update_fetch(self.token);
            self.closing = true;
        }
    }
    fn retired(&mut self, host: &mut TlsHttpHost) -> Result<bool, String> {
        if self.closing && host.update_fetch_retired(self.token)? {
            if !host.is_stopping() {
                host.release_update_fetch(self.token)?;
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn poll(&mut self, host: &mut TlsHttpHost, rate: u64) -> Result<Option<Vec<u8>>, String> {
        if self.closing {
            return Ok(None);
        }
        let now = Instant::now();
        if now.duration_since(self.started) > Duration::from_secs(600)
            || now.duration_since(self.last_activity) > Duration::from_secs(15)
        {
            return Err("update transfer timeout".into());
        }
        if !self.connected {
            if now.duration_since(self.started) > Duration::from_secs(5) {
                return Err("update connect timeout".into());
            }
            self.connected = host.update_fetch_connected(self.token)?;
            if !self.connected {
                return Ok(None);
            }
        }
        if self.wire_pos < self.wire.len() {
            match host.update_fetch_write(self.token, &self.wire[self.wire_pos..]) {
                Ok(0) => return Err("update socket write zero".into()),
                Ok(n) => {
                    self.wire_pos += n;
                    self.last_activity = now;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                Err(_) => return Err("update socket write".into()),
            }
            if self.wire_pos < self.wire.len() {
                return Ok(None);
            }
            self.wire.clear();
            self.wire_pos = 0;
        }
        if self.pending_tls {
            if let Some(result) = host.poll_update_tls(self.token)? {
                self.pending_tls = false;
                match result.step {
                    UpdateTlsStep::NeedInput => self.need_read = true,
                    UpdateTlsStep::Handshake { complete } => {
                        if result.consumed > self.cipher.len() {
                            return Err("update handshake consumed bound".into());
                        }
                        self.cipher.drain(..result.consumed);
                        self.handshake_complete = complete;
                        self.wire = result.output;
                        self.wire_pos = 0;
                        self.need_read = false;
                    }
                    UpdateTlsStep::Encrypted => {
                        self.wire = result.output;
                        self.wire_pos = 0;
                    }
                    UpdateTlsStep::Plaintext => {
                        if result.consumed == 0 || result.consumed > self.cipher.len() {
                            return Err("update plaintext consumed bound".into());
                        }
                        self.cipher.drain(..result.consumed);
                        self.need_read = false;
                        let body = self.parser.feed(&result.output)?;
                        if self.parser.complete() {
                            self.complete = true;
                            self.close(host);
                        }
                        if !body.is_empty() {
                            return Ok(Some(body));
                        }
                    }
                    UpdateTlsStep::PeerClosed => {
                        return Err("update peer closed before complete response".into())
                    }
                }
            } else {
                return Ok(None);
            }
        }
        if self.closing || !self.wire.is_empty() {
            return Ok(None);
        }
        if !self.pending_tls {
            let call = if !self.handshake_started {
                self.handshake_started = true;
                Some((UpdateTlsCall::Handshake, &[][..]))
            } else if self.handshake_complete && !self.request_sent {
                self.request_sent = true;
                Some((UpdateTlsCall::Encrypt, self.request.as_slice()))
            } else if !self.cipher.is_empty() && !self.need_read {
                Some((
                    if self.handshake_complete {
                        UpdateTlsCall::Decrypt
                    } else {
                        UpdateTlsCall::Handshake
                    },
                    self.cipher.as_slice(),
                ))
            } else {
                None
            };
            if let Some((call, bytes)) = call {
                host.submit_update_tls(self.token, call, bytes)?;
                self.pending_tls = true;
                return Ok(None);
            }
        }
        if now < self.next_read {
            return Ok(None);
        }
        let capacity = CHUNK
            .checked_sub(self.cipher.len())
            .ok_or("update ciphertext bound")?;
        if capacity == 0 {
            return Err("update TLS record exceeds frame".into());
        }
        let mut bytes = vec![0u8; capacity];
        match host.update_fetch_read(self.token, &mut bytes) {
            Ok(0) => return Err("update EOF before complete response".into()),
            Ok(n) => {
                bytes.truncate(n);
                self.cipher.extend_from_slice(&bytes);
                self.need_read = false;
                self.last_activity = now;
                self.next_read = now + Duration::from_micros((n as u64 * 1_000_000) / rate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(_) => return Err("update socket read".into()),
        }
        Ok(None)
    }
}
impl UpdateNetwork {
    pub(super) fn poll_client(&mut self, host: &mut TlsHttpHost) -> Result<(), String> {
        if self.client.is_none() && !self.stopping && Instant::now() >= self.retry_at {
            let mut shared = self.control.0.lock().map_err(|_| "update control lock")?;
            if let Some(configuration) = &self.configuration {
                if configuration.download {
                    if let Some((peer, target, attempt)) = self.retry_blob.take() {
                        let mut client = Client::blob(peer, target);
                        client.attempt = attempt;
                        self.client = Some(client);
                    } else if let Some((peer, target)) = shared.requested.take() {
                        self.client = Some(Client::blob(peer, target));
                    }
                }
                if self.client.is_none() && !shared.metadata_pending {
                    shared
                        .candidates
                        .retain(|c| c.seen.elapsed() < Duration::from_secs(90));
                    if let Some(candidate) =
                        shared.candidates.iter_mut().find(|c| c.checked.is_none())
                    {
                        candidate.checked = Some(Instant::now());
                        self.client = Some(Client::metadata(candidate.peer));
                    }
                }
            }
        }

        let Some(mut client) = self.client.take() else {
            return Ok(());
        };
        if self.stopping
            || self
                .configuration
                .as_ref()
                .is_none_or(|c| !c.download && matches!(client.purpose, FetchPurpose::Blob(_)))
        {
            client.stop(host);
        }
        if let Some(configuration) = &self.configuration {
            let may_work = self.pending.is_none() && self.close_handles.is_empty();
            match client.poll(
                host,
                configuration,
                &self.control,
                &mut self.close_handles,
                may_work,
            ) {
                Ok(Some(operation)) => {
                    let id = self
                        .control
                        .work(operation)
                        .ok_or("single update file mailbox busy")?;
                    self.pending = Some((id, IoOwner::Client));
                }
                Ok(None) => (),
                Err(_) => client.fail(host, &self.control, &mut self.close_handles),
            }
        } else {
            client.stop(host);
            if let Some(s) = &mut client.session {
                if s.retired(host)? {
                    client.session = None;
                    client.done = true;
                }
            }
        }
        if client.done && client.session.is_none() {
            if client.failed {
                if let FetchPurpose::Blob(target) = client.purpose {
                    if client.attempt < 3 {
                        self.retry_blob = Some((client.peer, target, client.attempt + 1));
                        self.retry_at = Instant::now()
                            + Duration::from_secs([5, 15, 60][client.attempt as usize]);
                    }
                } else {
                    self.retry_at = Instant::now() + Duration::from_secs(5);
                }
            }
        } else {
            self.client = Some(client);
        }
        Ok(())
    }
}
