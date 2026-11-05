use super::*;
pub(super) struct Challenge {
    pub created: Instant,
    node: [u8; 16],
    nonce: [u8; 16],
    challenge: [u8; 16],
    address: Ipv4Addr,
}
enum Body {
    Memory { bytes: Arc<[u8]>, position: usize },
    Blob { target: VerifiedTarget },
}
pub(super) struct ServerRecord {
    pub token: RequestToken,
    pub closed: bool,
    retired: bool,
    body_done: bool,
    body: Body,
    headers: ResponseHead,
    handle: Option<u64>,
    offset: u64,
    end: u64,
    pending_end: Option<u64>,
    work: bool,
    ready: bool,
    started: bool,
    chunk: Option<Vec<u8>>,
}
impl UpdateNetwork {
    pub fn service_update_event(
        &mut self,
        event: &TlsHttpEvent,
        host: &mut TlsHttpHost,
    ) -> Result<bool, String> {
        let token = match event {
            TlsHttpEvent::Head { head, .. } => head.token,
            TlsHttpEvent::Body { token, .. } | TlsHttpEvent::Closed { token } => *token,
            TlsHttpEvent::ResponseReady(t)
            | TlsHttpEvent::ResponseComplete(t)
            | TlsHttpEvent::Retired(t) => *t,
        };
        if let TlsHttpEvent::Head {
            head,
            scope,
            admitted_at,
        } = event
        {
            if *scope == TlsListenerScope::UpdateFetch {
                host.cancel(token)?;
                return Ok(true);
            }
            if *scope != TlsListenerScope::UpdateServe {
                return Ok(false);
            }
            if self.stopping
                || self.servers.len() >= 4
                || self.server_head(head, *admitted_at, host).is_err()
            {
                let _ = host.respond(
                    token,
                    ResponseHead {
                        status: 403,
                        headers: vec![Header {
                            name: "Cache-Control".into(),
                            value: b"no-store".to_vec(),
                        }],
                        content_length: 0,
                    },
                    &[],
                    true,
                );
            }
            return Ok(true);
        }
        let Some(index) = self.servers.iter().position(|r| r.token == token) else {
            return Ok(false);
        };
        let record = &mut self.servers[index];
        match event {
            TlsHttpEvent::Body {
                sequence,
                offset,
                bytes,
                eof,
                ..
            } => {
                if *offset != 0 || !bytes.is_empty() || !*eof {
                    record.closed = true;
                    host.cancel(token)?;
                } else {
                    host.ack_body(token, *sequence, 0)?;
                    record.body_done = true;
                    record.ready = true;
                }
                Ok(true)
            }
            TlsHttpEvent::ResponseReady(_) => {
                if let Some(end) = record.pending_end.take() {
                    record.offset = end;
                }
                record.ready = true;
                Ok(true)
            }
            TlsHttpEvent::ResponseComplete(_) => {
                if let Some(end) = record.pending_end.take() {
                    record.offset = end;
                }
                record.ready = false;
                if let Some(handle) = record.handle.take() {
                    self.close_handles.push_back(handle);
                }
                Ok(true)
            }
            TlsHttpEvent::Closed { .. } => {
                record.closed = true;
                if !record.work {
                    if let Some(handle) = record.handle.take() {
                        self.close_handles.push_back(handle);
                    }
                }
                Ok(false)
            }
            TlsHttpEvent::Retired(_) => {
                record.retired = true;
                record.closed = true;
                Ok(false)
            }
            _ => Ok(true),
        }
    }
    fn server_head(
        &mut self,
        head: &RequestHead,
        admitted_at: Instant,
        host: &mut TlsHttpHost,
    ) -> Result<(), String> {
        let config = self
            .configuration
            .as_ref()
            .filter(|c| c.share)
            .ok_or("update sharing disabled")?;
        if head.method != "GET"
            || head.has_body
            || head.content_length.unwrap_or(0) != 0
            || head.target.len() > 160
            || wire::header(head, "cookie")?.is_some()
            || wire::header(head, "authorization")?.is_some()
        {
            return Err("update request method or credentials".into());
        }
        let IpAddr::V4(address) = host.request_peer(head.token)? else {
            return Err("update source IPv4 required".into());
        };
        if !address.is_private() {
            return Err("update source network".into());
        }
        let node = wire::unhex::<16>(wire::header(head, "x-fc-node")?.ok_or("update node")?)
            .ok_or("update node encoding")?;
        let nonce = wire::unhex::<16>(wire::header(head, "x-fc-nonce")?.ok_or("update nonce")?)
            .ok_or("update nonce encoding")?;
        let challenge =
            wire::unhex::<16>(wire::header(head, "x-fc-challenge")?.ok_or("update challenge")?)
                .ok_or("update challenge encoding")?;
        let tag = wire::unhex::<32>(wire::header(head, "x-fc-proof")?.ok_or("update proof")?)
            .ok_or("update proof encoding")?;
        if node == [0; 16] || nonce == [0; 16] {
            return Err("zero update identity".into());
        }
        let range = wire::header(head, "range")?
            .map(std::str::from_utf8)
            .transpose()
            .map_err(|_| "range encoding")?
            .unwrap_or("");
        if !wire::verify_proof(
            config.auth.as_ref(),
            node,
            nonce,
            challenge,
            &head.target,
            range,
            &tag,
        ) {
            return Err("update membership proof".into());
        }
        let mut headers = vec![
            Header {
                name: "Cache-Control".into(),
                value: b"no-store".to_vec(),
            },
            Header {
                name: "Content-Type".into(),
                value: b"application/octet-stream".to_vec(),
            },
        ];
        let (body, offset, end, status) = if head.target == "/update/v1/challenge" {
            if challenge != [0; 16] || !range.is_empty() || self.challenges.len() >= 64 {
                return Err("challenge capacity or request".into());
            }
            let challenge = random()?;
            let response = wire::challenge_response(config, node, nonce, challenge);
            self.challenges.push(Challenge {
                created: Instant::now(),
                node,
                nonce,
                challenge,
                address,
            });
            let length = response.len() as u64;
            (
                Body::Memory {
                    bytes: response.into(),
                    position: 0,
                },
                0,
                length,
                200,
            )
        } else {
            let i = self
                .challenges
                .iter()
                .position(|c| {
                    c.node == node
                        && c.nonce == nonce
                        && c.challenge == challenge
                        && c.address == address
                        && c.created.elapsed() < Duration::from_secs(30)
                })
                .ok_or("missing or replayed resource challenge")?;
            self.challenges.swap_remove(i);
            let repository = self
                .control
                .0
                .lock()
                .map_err(|_| "update repository lock")?
                .repository
                .clone()
                .ok_or("update repository unavailable")?;
            if head.target == "/update/v1/catalog" {
                if !range.is_empty() {
                    return Err("metadata range refused".into());
                }
                let length = repository.catalog.len() as u64;
                (
                    Body::Memory {
                        bytes: repository.catalog,
                        position: 0,
                    },
                    0,
                    length,
                    200,
                )
            } else if let Some(name) = head.target.strip_prefix("/update/v1/metadata/") {
                if !wire::metadata_name(name) || !range.is_empty() {
                    return Err("metadata path or range".into());
                }
                let bytes = repository
                    .metadata
                    .get(name)
                    .ok_or("unknown metadata")?
                    .clone();
                let length = bytes.len() as u64;
                (Body::Memory { bytes, position: 0 }, 0, length, 200)
            } else if let Some(name) = head.target.strip_prefix("/update/v1/blobs/") {
                let hash = wire::unhex::<32>(name.as_bytes()).ok_or("blob hash path")?;
                let target = *repository
                    .blobs
                    .iter()
                    .find(|t| t.sha256 == hash)
                    .ok_or("blob unavailable")?;
                let offset = if range.is_empty() {
                    0
                } else {
                    wire::range(range.as_bytes(), target.length).ok_or("blob range refused")?
                };
                let etag = format!("\"{}\"", wire::hex(&hash));
                for field in ["if-range", "if-match"] {
                    if wire::header(head, field)?.is_some_and(|v| v != etag.as_bytes()) {
                        return Err("blob ETag precondition".into());
                    }
                }
                headers.push(Header {
                    name: "ETag".into(),
                    value: etag.into_bytes(),
                });
                headers.push(Header {
                    name: "Accept-Ranges".into(),
                    value: b"bytes".to_vec(),
                });
                if !range.is_empty() {
                    headers.push(Header {
                        name: "Content-Range".into(),
                        value: format!("bytes {offset}-{}/{}", target.length - 1, target.length)
                            .into_bytes(),
                    });
                }
                (
                    Body::Blob { target },
                    offset,
                    target.length,
                    if range.is_empty() { 200 } else { 206 },
                )
            } else {
                return Err("unknown update resource".into());
            }
        };
        host.admit_body(head.token, 0, admitted_at + Duration::from_secs(120))?;
        self.servers.push(ServerRecord {
            token: head.token,
            closed: false,
            retired: false,
            body_done: false,
            body,
            headers: ResponseHead {
                status,
                headers,
                content_length: end - offset,
            },
            handle: None,
            offset,
            end,
            pending_end: None,
            work: false,
            ready: false,
            started: false,
            chunk: None,
        });
        Ok(())
    }
    pub(super) fn poll_servers(&mut self, host: &mut TlsHttpHost) -> Result<(), String> {
        let count = self.servers.len();
        let start = self.server_cursor;
        for step in 0..count {
            let index = (start + step) % count;
            let record = &mut self.servers[index];
            if record.closed {
                if !record.work {
                    if let Some(handle) = record.handle.take() {
                        self.close_handles.push_back(handle);
                    }
                }
                continue;
            }
            if !record.body_done
                || !record.ready
                || record.work
                || Instant::now() < self.next_upload_at
            {
                continue;
            }
            match &mut record.body {
                Body::Memory { bytes, position } => {
                    let end = (*position + CHUNK).min(bytes.len());
                    let final_chunk = end == bytes.len();
                    let part = &bytes[*position..end];
                    if !record.started {
                        host.respond(record.token, record.headers.clone(), part, final_chunk)?;
                        record.started = true;
                    } else {
                        host.send_response_body(record.token, part, final_chunk)?;
                    }
                    let rate = if self.control.0.lock().is_ok_and(|s| s.teaching_active) {
                        512 * 1024
                    } else {
                        2 * 1024 * 1024
                    };
                    self.next_upload_at = Instant::now()
                        + Duration::from_micros(part.len() as u64 * 1_000_000 / rate);
                    self.server_cursor = (index + 1) % count;
                    *position = end;
                    record.ready = false;
                }
                Body::Blob { target } => {
                    if let Some(bytes) = record.chunk.take() {
                        let end = record.offset + bytes.len() as u64;
                        let eof = end == record.end;
                        if !record.started {
                            host.respond(record.token, record.headers.clone(), &bytes, eof)?;
                            record.started = true;
                        } else {
                            host.send_response_body(record.token, &bytes, eof)?;
                        }
                        let rate = if self.control.0.lock().is_ok_and(|s| s.teaching_active) {
                            512 * 1024
                        } else {
                            2 * 1024 * 1024
                        };
                        self.next_upload_at = Instant::now()
                            + Duration::from_micros(bytes.len() as u64 * 1_000_000 / rate);
                        self.server_cursor = (index + 1) % count;
                        record.pending_end = Some(end);
                        record.ready = false;
                    } else if self.pending.is_none() && self.close_handles.is_empty() {
                        let operation = if let Some(handle) = record.handle {
                            NetworkOperation::ReadUpload {
                                handle,
                                offset: record.offset,
                                length: CHUNK.min((record.end - record.offset) as usize),
                            }
                        } else {
                            NetworkOperation::OpenUpload {
                                target: *target,
                                offset: record.offset,
                            }
                        };
                        if let Some(id) = self.control.work(operation) {
                            self.server_cursor = (index + 1) % count;
                            record.work = true;
                            self.pending = Some((id, IoOwner::Server(record.token)));
                        }
                    }
                }
            }
        }
        self.servers
            .retain(|r| !(r.closed && r.retired && !r.work && r.handle.is_none()));
        Ok(())
    }
    pub(super) fn server_reply(
        &mut self,
        token: RequestToken,
        result: Result<NetworkIoResult, String>,
        host: &mut TlsHttpHost,
    ) -> Result<(), String> {
        let Some(record) = self.servers.iter_mut().find(|r| r.token == token) else {
            if let Ok(NetworkIoResult::Opened { handle, .. }) = result {
                self.close_handles.push_back(handle);
            }
            return Err("missing update upload owner".into());
        };
        record.work = false;
        match result {
            Ok(NetworkIoResult::Opened { handle, offset }) => {
                if record.handle.is_some() || offset != record.offset {
                    self.close_handles.push_back(handle);
                    record.closed = true;
                    let _ = host.cancel(token);
                } else if record.closed {
                    self.close_handles.push_back(handle);
                } else {
                    record.handle = Some(handle);
                }
            }
            Ok(NetworkIoResult::Bytes { bytes, eof }) => {
                let end = record.offset + bytes.len() as u64;
                // A nonempty final Read need not observe physical file EOF.
                if bytes.is_empty()
                    || bytes.len() > CHUNK
                    || end > record.end
                    || eof && end != record.end
                {
                    record.closed = true;
                    let _ = host.cancel(token);
                } else if !record.closed {
                    record.chunk = Some(bytes);
                }
            }
            _ => {
                record.closed = true;
                let _ = host.cancel(token);
            }
        }
        Ok(())
    }
}

impl ServerRecord {
    pub(super) fn abandon_returned_file_owner(&mut self) {
        self.handle = None;
    }
}
