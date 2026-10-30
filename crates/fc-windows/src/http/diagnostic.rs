//! Explicit loopback-only synthetic transport probe. This listener accepts no
//! cookies/authorization, never authenticates, and never calls business routes.
use super::HttpHost;
use fc_ports::http::*;
use std::time::{Duration, Instant};
#[derive(Clone, Debug)]
pub struct HttpProbeReport {
    pub metrics: HttpMetrics,
    pub requests: u64,
    pub body_bytes: u64,
    pub response_bytes: u64,
    pub explicit_stop: bool,
    pub stale_token_rejected: bool,
    first_token: Option<RequestToken>,
}
impl HttpProbeReport {
    pub fn to_json(&self) -> String {
        let m = self.metrics;
        format!("{{\"generation\":{},\"submitted\":{},\"completions\":{},\"immediate_success\":{},\"pending\":{},\"immediate_errors\":{},\"canceled\":{},\"active_slots\":{},\"in_flight\":{},\"peak_in_flight\":{},\"native_allocation_bytes\":{},\"requests\":{},\"body_bytes\":{},\"response_bytes\":{},\"explicit_stop\":{},\"stale_token_rejected\":{}}}",m.generation,m.submitted,m.completions,m.immediate_success,m.pending,m.immediate_errors,m.canceled,m.active_slots,m.in_flight,m.peak_in_flight,m.native_allocation_bytes,self.requests,self.body_bytes,self.response_bytes,self.explicit_stop,self.stale_token_rejected)
    }
}
// A client may disconnect between its completion event and our next send/read.
// The transport already canceled that request and retains its I/O until Closed.
// Unexpected native/API failures still fail this diagnostic visibly.
fn request_result(result: Result<(), HttpError>) -> Result<(), HttpError> {
    match result {
        Err(HttpError::Native(code)) if super::native::request_disconnected(code) => Ok(()),
        other => other,
    }
}
struct Context {
    token: RequestToken,
    received: u64,
    sum: u64,
    download: u64,
    stop: bool,
}
/// Own one HTTP execution domain until the synthetic /stop response completes or
/// the bounded test deadline fires. The caller must arrange its own pre-existing
/// URL reservation; this function never changes URLACL, TLS or firewall settings.
pub fn serve_isolated(prefix: ListenPrefix, seconds: u32) -> Result<HttpProbeReport, HttpError> {
    serve_owned(prefix, seconds, None)
}
/// Two real native cycles in this process. The second actual Host rejects an
/// opaque token captured from the first; no client-supplied token is constructed.
pub fn serve_isolated_twice(
    prefix: ListenPrefix,
    seconds: u32,
) -> Result<[HttpProbeReport; 2], HttpError> {
    let first = serve_owned(prefix.clone(), seconds, None)?;
    if !first.explicit_stop {
        return Err(HttpError::Native(1460));
    }
    let old = first.first_token.ok_or(HttpError::Invariant)?;
    let second = serve_owned(prefix, seconds, Some(old))?;
    if !second.explicit_stop
        || !second.stale_token_rejected
        || second.metrics.generation <= first.metrics.generation
    {
        return Err(HttpError::Invariant);
    }
    Ok([first, second])
}
fn serve_owned(
    prefix: ListenPrefix,
    seconds: u32,
    stale: Option<RequestToken>,
) -> Result<HttpProbeReport, HttpError> {
    if !prefix.is_diagnostic() || seconds == 0 || seconds > 120 {
        return Err(HttpError::InvalidPrefix);
    }
    let base = prefix
        .as_str()
        .split_once("://")
        .and_then(|(_, s)| s.find('/').map(|i| s[i..].to_owned()))
        .ok_or(HttpError::InvalidPrefix)?;
    let mut host = HttpHost::open(prefix)?;
    let stale_token_rejected =
        stale.is_some_and(|t| host.receive_body(t, 0) == Err(HttpError::StaleRequest));
    if stale.is_some() && !stale_token_rejected {
        return Err(HttpError::Invariant);
    }
    let mut first_token = None;
    let deadline = Instant::now() + Duration::from_secs(u64::from(seconds));
    let mut contexts: Vec<Context> = Vec::with_capacity(MAX_REQUESTS);
    let mut requests = 0;
    let mut body_bytes = 0;
    let mut response_bytes = 0;
    let mut explicit_stop = false;
    loop {
        let events = host.poll(20)?;
        for event in events {
            match event {
                HttpEvent::Request(head) => {
                    requests += 1;
                    first_token.get_or_insert(head.token);
                    let secret = head.headers.iter().any(|h| {
                        [
                            "authorization",
                            "cookie",
                            "proxy-authorization",
                            "x-csrf-token",
                        ]
                        .iter()
                        .any(|n| h.name.eq_ignore_ascii_case(n))
                    });
                    let route = head.target.strip_prefix(&base).unwrap_or("");
                    if secret
                        || head.tls
                        || !matches!(
                            (head.method.as_str(), route),
                            ("GET", "health" | "download" | "stop") | ("POST", "echo")
                        )
                    {
                        request_result(host.respond(
                            head.token,
                            ResponseHead {
                                status: 400,
                                headers: Vec::new(),
                                content_length: 0,
                            },
                            &[],
                            false,
                        ))?;
                        continue;
                    }
                    if route == "echo" && head.has_body {
                        contexts.push(Context {
                            token: head.token,
                            received: 0,
                            sum: 0,
                            download: 0,
                            stop: false,
                        });
                        if let Err(error) = host.receive_body(head.token, 2 * 1024 * 1024) {
                            contexts.retain(|c| c.token != head.token);
                            if error == HttpError::Busy {
                                request_result(host.respond(
                                    head.token,
                                    ResponseHead {
                                        status: 503,
                                        headers: Vec::new(),
                                        content_length: 0,
                                    },
                                    &[],
                                    false,
                                ))?;
                            } else {
                                request_result(Err(error))?;
                            }
                        }
                    } else if route == "download" {
                        let total = 3 * CHUNK_BYTES as u64;
                        contexts.push(Context {
                            token: head.token,
                            received: 0,
                            sum: 0,
                            download: CHUNK_BYTES as u64,
                            stop: false,
                        });
                        request_result(host.respond(
                            head.token,
                            ResponseHead {
                                status: 200,
                                headers: Vec::new(),
                                content_length: total,
                            },
                            &[0x5a; CHUNK_BYTES],
                            true,
                        ))?;
                        response_bytes += CHUNK_BYTES as u64;
                    } else {
                        let body = if route == "echo" {
                            b"received=0;sum=0".as_slice()
                        } else {
                            b"synthetic-http-iocp".as_slice()
                        };
                        if route == "stop" {
                            contexts.push(Context {
                                token: head.token,
                                received: 0,
                                sum: 0,
                                download: 0,
                                stop: true,
                            });
                        }
                        request_result(host.respond(
                            head.token,
                            ResponseHead {
                                status: 200,
                                headers: Vec::new(),
                                content_length: body.len() as u64,
                            },
                            body,
                            false,
                        ))?;
                        response_bytes += body.len() as u64;
                    }
                }
                HttpEvent::BodyChunk { token, bytes, eof } => {
                    if bytes.iter().any(|b| *b != 0x5a) {
                        host.cancel(token, 13)?;
                        continue;
                    }
                    let context = contexts
                        .iter_mut()
                        .find(|c| c.token == token)
                        .ok_or(HttpError::Invariant)?;
                    context.received += bytes.len() as u64;
                    context.sum += bytes.iter().map(|b| u64::from(*b)).sum::<u64>();
                    body_bytes += bytes.len() as u64;
                    if eof {
                        let body = format!("received={};sum={}", context.received, context.sum);
                        request_result(host.respond(
                            token,
                            ResponseHead {
                                status: 200,
                                headers: Vec::new(),
                                content_length: body.len() as u64,
                            },
                            body.as_bytes(),
                            false,
                        ))?;
                        response_bytes += body.len() as u64;
                    } else {
                        request_result(host.receive_body(token, 2 * 1024 * 1024))?;
                    }
                }
                HttpEvent::ResponseReady(token) => {
                    let context = contexts
                        .iter_mut()
                        .find(|c| c.token == token)
                        .ok_or(HttpError::Invariant)?;
                    if context.download == 0 {
                        return Err(HttpError::Invariant);
                    }
                    context.download += CHUNK_BYTES as u64;
                    request_result(host.send_response_body(
                        token,
                        &[0x5a; CHUNK_BYTES],
                        context.download < 3 * CHUNK_BYTES as u64,
                    ))?;
                    response_bytes += CHUNK_BYTES as u64;
                }
                HttpEvent::ResponseComplete(token) => {
                    if contexts.iter().any(|c| c.token == token && c.stop) {
                        explicit_stop = true;
                        host.request_stop()?;
                    }
                    contexts.retain(|c| c.token != token);
                }
                HttpEvent::Closed { token, .. } => contexts.retain(|c| c.token != token),
            }
        }
        if host.state() == HttpState::Running && Instant::now() >= deadline {
            host.request_stop()?;
        }
        if host.state() == HttpState::Closed {
            return Ok(HttpProbeReport {
                metrics: host.metrics(),
                requests,
                body_bytes,
                response_bytes,
                explicit_stop,
                stale_token_rejected,
                first_token,
            });
        }
    }
}
