//! One bounded update protocol driver on the existing Net/IOCP/SSPI owners.
//! File handles and all disk I/O remain in the original shared worker pool.
use crate::update_packages::OwnedMetadata;
use fc_domain::https_settings::HttpsConfiguration;
use fc_ports::{
    http::{Header, RequestHead, RequestToken, ResponseHead},
    update_discovery::UpdateGroupAuthenticator,
    RandomPort,
};
use fc_security::update_group::UpdateGroupKey;
use fc_windows::{
    lan_discovery::DiscoveryResponder,
    tls_frontdoor::{TlsHttpEvent, TlsHttpHost, TlsListenerScope},
    update_cache::VerifiedTarget,
};
use std::{
    collections::{BTreeMap, VecDeque},
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
mod client;
mod server;
mod wire;
use client::Client;
use server::{Challenge, ServerRecord};
const CHUNK: usize = 65536;
const META: usize = 256 * 1024;
const META_TOTAL: usize = 2 * 1024 * 1024;
#[derive(Clone)]
pub struct NetworkConfiguration {
    pub auth: Arc<UpdateGroupKey>,
    pub node: [u8; 16],
    pub listener: HttpsConfiguration,
    pub certificate_sha256: [u8; 32],
    pub seeds: Vec<Ipv4Addr>,
    pub download: bool,
    pub share: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    pub node: [u8; 16],
    pub address: Ipv4Addr,
    pub port: u16,
    pub certificate_sha256: [u8; 32],
}
#[derive(Clone)]
pub struct PublishedRepository {
    catalog: Arc<[u8]>,
    metadata: BTreeMap<String, Arc<[u8]>>,
    blobs: Vec<VerifiedTarget>,
}
impl PublishedRepository {
    pub fn new(metadata: OwnedMetadata, blobs: Vec<VerifiedTarget>) -> Result<Self, String> {
        if metadata.roots.len() > 32
            || metadata
                .roots
                .windows(2)
                .any(|w| w[0].0.checked_add(1) != Some(w[1].0))
            || blobs.len() > 16
            || blobs
                .iter()
                .any(|t| t.length == 0 || t.length > 128 * 1024 * 1024 || t.sha256 == [0; 32])
        {
            return Err("update repository bounds".into());
        }
        let roots = metadata.roots.iter().map(|(v, _)| *v).collect::<Vec<_>>();
        let mut files = BTreeMap::new();
        for (v, bytes) in metadata.roots {
            if v == 0 {
                return Err("invalid root version".into());
            }
            files.insert(format!("{v}.root.json"), Arc::<[u8]>::from(bytes));
        }
        for (name, bytes) in [
            ("timestamp.json", metadata.timestamp),
            ("snapshot.json", metadata.snapshot),
            ("targets.json", metadata.targets),
        ] {
            files.insert(name.into(), Arc::<[u8]>::from(bytes));
        }
        if files.values().any(|b| b.is_empty() || b.len() > META)
            || files.values().map(|b| b.len()).sum::<usize>() > META_TOTAL
        {
            return Err("update metadata bounds".into());
        }
        let catalog = serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"roots":roots}))
            .map_err(|_| "catalog encoding")?;
        Ok(Self {
            catalog: catalog.into(),
            metadata: files,
            blobs,
        })
    }
}
pub enum NetworkOperation {
    OpenUpload {
        target: VerifiedTarget,
        offset: u64,
    },
    ReadUpload {
        handle: u64,
        offset: u64,
        length: usize,
    },
    BeginDownload {
        target: VerifiedTarget,
    },
    AppendDownload {
        handle: u64,
        offset: u64,
        bytes: Vec<u8>,
    },
    FinishDownload {
        handle: u64,
    },
    Close {
        handle: u64,
    },
}
pub struct NetworkWork {
    pub id: u64,
    pub operation: NetworkOperation,
}
pub enum NetworkIoResult {
    Opened { handle: u64, offset: u64 },
    Bytes { bytes: Vec<u8>, eof: bool },
    Written { offset: u64 },
    Finished,
    Closed,
}
pub struct NetworkReply {
    pub id: u64,
    pub result: Result<NetworkIoResult, String>,
}
pub enum NetworkEvent {
    Progress { received: u64, total: u64 },
    Metadata { peer: Peer, metadata: OwnedMetadata },
    Downloaded { target: VerifiedTarget },
    Status(&'static str),
}
struct Candidate {
    peer: Peer,
    seen: Instant,
    checked: Option<Instant>,
}
struct Shared {
    configuration: Option<NetworkConfiguration>,
    revision: u64,
    repository: Option<PublishedRepository>,
    check: bool,
    candidates: Vec<Candidate>,
    nonce: Option<([u8; 16], Instant)>,
    listener_ready: bool,
    requested: Option<(Peer, VerifiedTarget)>,
    events: VecDeque<NetworkEvent>,
    metadata_pending: bool,
    work: Option<NetworkWork>,
    busy: Option<u64>,
    reply: Option<NetworkReply>,
    next: u64,
    stopping: bool,
    file_owners_returned: bool,
    network_idle: bool,
    teaching_active: bool,
}
#[derive(Clone)]
pub struct NetworkControl(Arc<Mutex<Shared>>);
impl Default for NetworkControl {
    fn default() -> Self {
        Self::new()
    }
}
impl NetworkControl {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Shared {
            configuration: None,
            revision: 0,
            repository: None,
            check: false,
            candidates: Vec::new(),
            nonce: None,
            listener_ready: false,
            requested: None,
            events: VecDeque::new(),
            metadata_pending: false,
            work: None,
            busy: None,
            reply: None,
            next: 0,
            stopping: false,
            file_owners_returned: true,
            network_idle: true,
            teaching_active: true,
        })))
    }
    pub fn set_configuration(
        &self,
        configuration: Option<NetworkConfiguration>,
    ) -> Result<(), String> {
        if configuration.as_ref().is_some_and(|c| {
            c.node == [0; 16]
                || c.certificate_sha256 == [0; 32]
                || c.seeds.len() > 32
                || c.seeds.iter().any(|ip| !ip.is_private())
                || !c
                    .listener
                    .host
                    .as_str()
                    .parse::<Ipv4Addr>()
                    .is_ok_and(|ip| ip.is_private())
        }) {
            return Err("invalid update network configuration".into());
        }
        let mut s = self.0.lock().map_err(|_| "update control lock")?;
        s.configuration = configuration;
        s.revision = s
            .revision
            .checked_add(1)
            .ok_or("update revision exhausted")?;
        s.candidates.clear();
        s.nonce = None;
        s.requested = None;
        Ok(())
    }
    pub fn publish(&self, repository: Option<PublishedRepository>) -> Result<(), String> {
        self.0.lock().map_err(|_| "update control lock")?.repository = repository;
        Ok(())
    }
    pub fn check_now(&self) {
        if let Ok(mut s) = self.0.lock() {
            s.check = true;
            for p in &mut s.candidates {
                p.checked = None;
            }
        }
    }
    pub fn request_blob(&self, peer: Peer, target: VerifiedTarget) -> Result<(), String> {
        if !peer.address.is_private()
            || peer.port == 0
            || target.length == 0
            || target.length > 128 * 1024 * 1024
            || target.sha256 == [0; 32]
        {
            return Err("invalid verified download target".into());
        }
        let mut s = self.0.lock().map_err(|_| "update control lock")?;
        if s.stopping || !s.configuration.as_ref().is_some_and(|c| c.download) {
            return Err("payload downloading disabled".into());
        }
        if s.requested.is_some() {
            return Err("one update download request".into());
        }
        s.requested = Some((peer, target));
        Ok(())
    }
    pub fn stop(&self) {
        if let Ok(mut s) = self.0.lock() {
            s.stopping = true;
            s.requested = None;
            if let Some(work) = s.work.take() {
                s.reply = Some(NetworkReply {
                    id: work.id,
                    result: Err("update stopped before file admission".into()),
                });
            }
        }
    }
    /// Called only after the original worker has physically dropped every file
    /// owner; a cancellation request or unpolled ticket is insufficient.
    pub fn confirm_file_owners_returned(&self) {
        if let Ok(mut s) = self.0.lock() {
            s.file_owners_returned = true;
        }
    }
    pub fn is_idle(&self) -> bool {
        self.0.lock().ok().is_some_and(|s| {
            s.network_idle
                && s.busy.is_none()
                && s.work.is_none()
                && s.reply.is_none()
                && (!s.stopping || s.file_owners_returned)
        })
    }
    pub fn take_event(&self) -> Option<NetworkEvent> {
        let mut s = self.0.lock().ok()?;
        let e = s.events.pop_front()?;
        if matches!(e, NetworkEvent::Metadata { .. }) {
            s.metadata_pending = false;
        }
        Some(e)
    }
    /// Unknown teaching activity keeps the conservative rate until the native
    /// owner explicitly publishes a known idle state.
    pub fn set_teaching_active(&self, active: bool) {
        if let Ok(mut s) = self.0.lock() {
            s.teaching_active = active;
        }
    }
    pub fn has_work(&self) -> bool {
        self.0
            .lock()
            .ok()
            .is_some_and(|s| s.work.is_some() || s.busy.is_some() || !s.network_idle)
    }
    pub fn take_work(&self) -> Option<NetworkWork> {
        self.0.lock().ok()?.work.take()
    }
    pub fn complete_work(&self, reply: NetworkReply) {
        if let Ok(mut s) = self.0.lock() {
            if s.busy == Some(reply.id) && s.reply.is_none() {
                s.reply = Some(reply);
            }
        }
    }
    fn work(&self, operation: NetworkOperation) -> Option<u64> {
        let mut s = self.0.lock().ok()?;
        if s.stopping || s.busy.is_some() || s.reply.is_some() {
            return None;
        }
        s.next = s.next.checked_add(1)?;
        let id = s.next;
        s.busy = Some(id);
        s.file_owners_returned = false;
        s.work = Some(NetworkWork { id, operation });
        Some(id)
    }
    fn reply(&self) -> Option<NetworkReply> {
        let mut s = self.0.lock().ok()?;
        let r = s.reply.take()?;
        s.busy = None;
        Some(r)
    }
    fn event(&self, event: NetworkEvent) {
        if let Ok(mut s) = self.0.lock() {
            match event {
                NetworkEvent::Progress { .. } => s
                    .events
                    .retain(|e| !matches!(e, NetworkEvent::Progress { .. })),
                NetworkEvent::Status(_) => {
                    s.events.retain(|e| !matches!(e, NetworkEvent::Status(_)))
                }
                NetworkEvent::Metadata { .. } => {
                    if s.metadata_pending {
                        return;
                    }
                    s.metadata_pending = true;
                }
                NetworkEvent::Downloaded { .. } => (),
            }
            if s.events.len() >= 4 {
                if let Some(i) = s.events.iter().position(|e| {
                    matches!(e, NetworkEvent::Status(_) | NetworkEvent::Progress { .. })
                }) {
                    s.events.remove(i);
                } else {
                    return;
                }
            }
            s.events.push_back(event);
        }
    }
    pub fn discovery_enabled(&self) -> bool {
        self.0
            .lock()
            .ok()
            .is_some_and(|s| !s.stopping && s.configuration.is_some())
    }
    pub fn discovery_message(&self, bytes: &[u8], source: Ipv4Addr) -> Option<Vec<u8>> {
        use fc_protocol::update_discovery::*;
        let mut s = self.0.lock().ok()?;
        let c = s.configuration.clone()?;
        if !source.is_private() {
            return None;
        }
        if let Ok(q) = decode_query(bytes, c.auth.as_ref()) {
            if !c.share || !s.listener_ready || q.node == c.node {
                return None;
            }
            let address = c.listener.host.as_str().parse().ok()?;
            return encode_reply(
                Reply {
                    node: c.node,
                    nonce: q.nonce,
                    address,
                    port: c.listener.port,
                    certificate_sha256: c.certificate_sha256,
                },
                c.auth.as_ref(),
            )
            .ok()
            .map(|v| v.to_vec());
        }
        let (nonce, at) = s.nonce?;
        if Instant::now().duration_since(at) > Duration::from_secs(90) {
            return None;
        }
        let reply = decode_reply(bytes, nonce, source, c.auth.as_ref()).ok()?;
        if reply.node == c.node {
            return None;
        }
        let peer = Peer {
            node: reply.node,
            address: reply.address,
            port: reply.port,
            certificate_sha256: reply.certificate_sha256,
        };
        s.candidates
            .retain(|p| p.seen.elapsed() < Duration::from_secs(90));
        if let Some(p) = s.candidates.iter_mut().find(|p| p.peer.node == peer.node) {
            if p.peer != peer {
                p.checked = None;
            }
            p.peer = peer;
            p.seen = Instant::now();
        } else if s.candidates.len() < 64 {
            s.candidates.push(Candidate {
                peer,
                seen: Instant::now(),
                checked: None,
            });
        }
        None
    }
}
fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0; N];
    fc_windows::SystemRandom
        .fill(&mut b)
        .map_err(|_| "update randomness unavailable")?;
    if b.iter().all(|v| *v == 0) {
        return Err("zero update randomness".into());
    }
    Ok(b)
}
#[derive(Clone, Copy)]
enum IoOwner {
    Server(RequestToken),
    Client,
    Cleanup,
}
pub struct UpdateNetwork {
    control: NetworkControl,
    revision: u64,
    configuration: Option<NetworkConfiguration>,
    next_discovery: Instant,
    servers: Vec<ServerRecord>,
    challenges: Vec<Challenge>,
    client: Option<Client>,
    pending: Option<(u64, IoOwner)>,
    close_handles: VecDeque<u64>,
    stopping: bool,
    retry_at: Instant,
    retry_blob: Option<(Peer, VerifiedTarget, u8)>,
    next_upload_at: Instant,
    server_cursor: usize,
}
impl UpdateNetwork {
    pub fn new(control: NetworkControl) -> Self {
        let delay = random::<2>().map_or(0, |n| u16::from_le_bytes(n) % 31);
        Self {
            control,
            revision: u64::MAX,
            configuration: None,
            next_discovery: Instant::now() + Duration::from_secs(delay as u64),
            servers: Vec::new(),
            challenges: Vec::new(),
            client: None,
            pending: None,
            close_handles: VecDeque::new(),
            stopping: false,
            retry_at: Instant::now(),
            retry_blob: None,
            next_upload_at: Instant::now(),
            server_cursor: 0,
        }
    }
    pub fn control(&self) -> &NetworkControl {
        &self.control
    }
    pub fn is_idle(&self) -> bool {
        self.servers.is_empty()
            && self.client.is_none()
            && self.pending.is_none()
            && self.close_handles.is_empty()
    }
    pub fn stop(&mut self, host: &mut TlsHttpHost) {
        self.control.stop();
        self.stopping = true;
        for r in &mut self.servers {
            r.closed = true;
            let _ = host.cancel(r.token);
        }
        if let Some(c) = &mut self.client {
            c.stop(host);
        }
    }
    pub fn poll(&mut self, host: &mut TlsHttpHost) -> Result<(), String> {
        if self.control.0.lock().is_ok_and(|s| s.stopping) && !self.stopping {
            self.stop(host);
        }
        let (revision, configuration, check) = {
            let mut s = self.control.0.lock().map_err(|_| "update control lock")?;
            (
                s.revision,
                if s.revision != self.revision {
                    s.configuration.clone()
                } else {
                    None
                },
                std::mem::take(&mut s.check),
            )
        };
        if revision != self.revision {
            self.revision = revision;
            self.configuration = configuration;
            self.retry_blob = None;
            self.next_discovery = Instant::now()
                + Duration::from_secs(
                    random::<2>().map_or(0, |b| (u16::from_le_bytes(b) % 31) as u64),
                );
            self.challenges.clear();
            for r in &mut self.servers {
                r.closed = true;
                let _ = host.cancel(r.token);
            }
            if let Some(c) = &mut self.client {
                c.stop(host);
            }
            if !host.is_stopping() {
                if let Err(error) = host.configure_update(
                    self.configuration
                        .as_ref()
                        .filter(|c| c.share && !self.stopping)
                        .map(|c| &c.listener),
                ) {
                    if error == fc_windows::tls_frontdoor::ConfiguredTlsError::Cleanup {
                        return Err("update native cleanup retained owner".into());
                    }
                    self.control.event(NetworkEvent::Status(
                        "更新分享入口暂不可用，仍可检查内网版本",
                    ));
                }
            }
        }

        if check {
            self.next_discovery = Instant::now();
            self.retry_at = Instant::now();
        }
        if let Ok(mut s) = self.control.0.lock() {
            s.listener_ready = host.update_phase()
                == fc_windows::tls_frontdoor::LanListenerPhase::Listening
                && !self.stopping;
        }
        if let Some(reply) = self.control.reply() {
            let (_, owner) = self
                .pending
                .take()
                .filter(|(id, _)| *id == reply.id)
                .ok_or("unexpected update file receipt")?;
            match owner {
                IoOwner::Cleanup => {}
                IoOwner::Server(token) => self.server_reply(token, reply.result, host)?,
                IoOwner::Client => {
                    if let Some(c) = &mut self.client {
                        c.receipt(reply.result, &self.control, &mut self.close_handles)?;
                    }
                }
            }
        }
        self.poll_servers(host)?;
        self.poll_client(host)?;
        if !self.stopping && self.pending.is_none() {
            if let Some(handle) = self.close_handles.pop_front() {
                if let Some(id) = self.control.work(NetworkOperation::Close { handle }) {
                    self.pending = Some((id, IoOwner::Cleanup));
                } else {
                    self.close_handles.push_front(handle);
                }
            }
        }
        if self.stopping
            && self.control.0.lock().is_ok_and(|s| s.file_owners_returned)
            && self.pending.is_none()
        {
            self.close_handles.clear();
            for r in &mut self.servers {
                r.abandon_returned_file_owner();
            }
        }
        self.challenges
            .retain(|c| c.created.elapsed() < Duration::from_secs(30));
        let idle = self.is_idle();
        if let Ok(mut s) = self.control.0.lock() {
            s.network_idle = idle;
        }
        Ok(())
    }
    /// Invoke on the same discovery socket after teacher discovery setup. All
    /// received packets are dispatched once via NetworkControl::discovery_message.
    pub fn poll_discovery(&mut self, responder: &mut Option<DiscoveryResponder>) {
        if self.stopping || !self.control.discovery_enabled() {
            return;
        }
        if responder.is_none() {
            *responder = DiscoveryResponder::bind().ok();
        }
        if Instant::now() < self.next_discovery {
            return;
        }
        let Some(c) = &self.configuration else {
            return;
        };
        let Ok(nonce) = random::<16>() else {
            return;
        };
        let query = fc_protocol::update_discovery::encode_query(
            fc_protocol::update_discovery::Query {
                node: c.node,
                nonce,
            },
            c.auth.as_ref(),
        );
        let jitter = random::<2>().map_or(600, |b| 480 + u16::from_le_bytes(b) % 241);
        self.next_discovery = Instant::now() + Duration::from_secs(jitter as u64);
        if let Ok(mut s) = self.control.0.lock() {
            s.nonce = Some((nonce, Instant::now()));
            s.candidates
                .retain(|p| p.seen.elapsed() < Duration::from_secs(90));
        }
        if let (Some(socket), Ok(query)) = (responder.as_ref(), query) {
            let _ = socket.send_update_query(&query, Ipv4Addr::BROADCAST);
            for seed in &c.seeds {
                let _ = socket.send_update_query(&query, *seed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::https_settings::{HttpsCertificateRef, HttpsHost, HttpsSourceNetworks};
    /// Real native TLS and sockets; the file-port fixture is memory-only so the
    /// test cannot mutate a classroom cache or execute an installer.
    #[test]
    #[ignore = "explicit Windows fixture supplies its temporary certificate and actual private IPv4"]
    fn native_https_group_challenge_range_resume_and_cleanup() {
        let address: Ipv4Addr = std::env::var("FC_UPDATE_TEST_IPV4")
            .expect("owned IPv4 fixture")
            .parse()
            .unwrap();
        let thumbprint = std::env::var("FC_UPDATE_TEST_CERT").expect("owned certificate fixture");
        let reserve = |address: Ipv4Addr| {
            let socket = std::net::TcpListener::bind((address, 0)).unwrap();
            let port = socket.local_addr().unwrap().port();
            drop(socket);
            port
        };
        let certificate = HttpsCertificateRef::parse(&thumbprint).unwrap();
        let local = HttpsConfiguration::new(
            HttpsHost::parse("127.0.0.1").unwrap(),
            reserve(Ipv4Addr::LOCALHOST),
            certificate,
        )
        .unwrap()
        .with_source_networks(HttpsSourceNetworks::parse("127.0.0.1/32").unwrap());
        let configuration = NetworkConfiguration {
            auth: Arc::new(UpdateGroupKey::new([41; 16], [42; 32]).unwrap()),
            node: [43; 16],
            listener: HttpsConfiguration::new(
                HttpsHost::parse(&address.to_string()).unwrap(),
                reserve(address),
                certificate,
            )
            .unwrap()
            .with_source_networks(
                HttpsSourceNetworks::parse("10.0.0.0/8,172.16.0.0/12,192.168.0.0/16").unwrap(),
            ),
            certificate_sha256: fc_windows::tls_frontdoor::certificate_der_sha256(
                fc_windows::tls_frontdoor::CertificateThumbprint::parse(&thumbprint).unwrap(),
            )
            .unwrap(),
            seeds: Vec::new(),
            download: true,
            share: true,
        };
        let control = NetworkControl::new();
        control
            .set_configuration(Some(configuration.clone()))
            .unwrap();
        control.set_teaching_active(false);
        let payload = (0..(192 * 1024 + 37))
            .map(|i| ((i * 31 + 7) % 251) as u8)
            .collect::<Vec<_>>();
        let target = VerifiedTarget {
            sha256: fc_security::sha256(&payload),
            length: payload.len() as u64,
        };
        control
            .publish(Some(
                PublishedRepository::new(
                    OwnedMetadata {
                        roots: Vec::new(),
                        timestamp: b"{}".to_vec(),
                        snapshot: b"{}".to_vec(),
                        targets: b"{}".to_vec(),
                    },
                    vec![target],
                )
                .unwrap(),
            ))
            .unwrap();
        let mut host = TlsHttpHost::open_local_and_lan(&local, None).unwrap();
        let mut network = UpdateNetwork::new(control.clone());
        let deadline = Instant::now() + Duration::from_secs(20);
        while host.update_phase() != fc_windows::tls_frontdoor::LanListenerPhase::Listening {
            host.poll().unwrap();
            network.poll(&mut host).unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let peer = Peer {
            node: configuration.node,
            address,
            port: configuration.listener.port,
            certificate_sha256: configuration.certificate_sha256,
        };
        control.request_blob(peer, target).unwrap();
        let prefix = 17033;
        let mut downloaded = payload[..prefix].to_vec();
        let mut opens = Vec::new();
        let mut handles = std::collections::BTreeSet::new();
        let mut interrupted = false;
        let mut completed = false;
        loop {
            for event in host.poll().unwrap() {
                let handled = network.service_update_event(&event, &mut host).unwrap();
                if !handled {
                    if let TlsHttpEvent::Retired(token) = event {
                        host.rearm(token).unwrap();
                    }
                }
            }
            network.poll(&mut host).unwrap();
            if let Some(work) = control.take_work() {
                let result = match work.operation {
                    NetworkOperation::OpenUpload {
                        target: actual,
                        offset,
                    } => {
                        assert_eq!(actual.sha256, target.sha256);
                        assert_eq!(actual.length, target.length);
                        assert!(offset >= prefix as u64);
                        opens.push(offset);
                        assert!(handles.insert(1));
                        NetworkIoResult::Opened { handle: 1, offset }
                    }
                    NetworkOperation::ReadUpload {
                        handle,
                        offset,
                        length,
                    } => {
                        assert_eq!(handle, 1);
                        assert!(handles.contains(&1));
                        assert!(length <= CHUNK);
                        let end = (offset as usize + length).min(payload.len());
                        NetworkIoResult::Bytes {
                            bytes: payload[offset as usize..end].to_vec(),
                            eof: end == payload.len(),
                        }
                    }
                    NetworkOperation::BeginDownload { target: actual } => {
                        assert_eq!(actual.sha256, target.sha256);
                        assert!(handles.insert(2));
                        NetworkIoResult::Opened {
                            handle: 2,
                            offset: downloaded.len() as u64,
                        }
                    }
                    NetworkOperation::AppendDownload {
                        handle,
                        offset,
                        bytes,
                    } => {
                        assert_eq!(handle, 2);
                        assert!(handles.contains(&2));
                        assert_eq!(offset, downloaded.len() as u64);
                        assert!(!bytes.is_empty() && bytes.len() <= CHUNK);
                        downloaded.extend_from_slice(&bytes);
                        NetworkIoResult::Written {
                            offset: downloaded.len() as u64,
                        }
                    }
                    NetworkOperation::FinishDownload { handle } => {
                        assert_eq!(handle, 2);
                        assert!(handles.remove(&2));
                        assert_eq!(downloaded, payload);
                        assert_eq!(fc_security::sha256(&downloaded), target.sha256);
                        NetworkIoResult::Finished
                    }
                    NetworkOperation::Close { handle } => {
                        assert!(handles.remove(&handle));
                        NetworkIoResult::Closed
                    }
                };
                control.complete_work(NetworkReply {
                    id: work.id,
                    result: Ok(result),
                });
            }
            if !interrupted && downloaded.len() > prefix && downloaded.len() < payload.len() {
                let token = network
                    .servers
                    .iter()
                    .find(|r| !r.closed)
                    .map(|r| r.token)
                    .expect("live upload to interrupt");
                host.cancel(token).unwrap();
                interrupted = true;
            }
            while let Some(event) = control.take_event() {
                if let NetworkEvent::Downloaded { target: actual } = event {
                    assert_eq!(actual.sha256, target.sha256);
                    completed = true;
                }
            }
            if completed && network.is_idle() {
                break;
            }
            assert!(Instant::now()<deadline,"bounded native update exchange timed out: downloaded={}, completed={}, handles={handles:?}, opens={opens:?}",downloaded.len(),completed);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(interrupted);
        assert!(opens.len() >= 2);
        assert!(opens[1] > opens[0]);
        assert!(handles.is_empty());
        network.stop(&mut host);
        control.confirm_file_owners_returned();
        network.poll(&mut host).unwrap();
        assert!(network.is_idle());
        host.request_stop().unwrap();
        while !host.poll_shutdown().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(matches!(
            fc_windows::update_windows_baseline(),
            Ok(fc_domain::updates::WindowsBaseline::Windows11)
        ));
    }
}
