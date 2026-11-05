//! Bounded HTTPS readiness endpoint; classroom enrollment and relay are not yet enabled.
use super::{Result, SchoolConfig};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

/// Four fixed workers, sixteen queued connections, 8 KiB headers, five-second
/// absolute handshake/header budget. Overflow closes immediately without work.
pub fn serve(state: &Path, password: &[u8]) -> Result<()> {
    let (config, tls) = super::load(state, password)?;
    let listener = TcpListener::bind(SocketAddr::new(config.published_ip, config.port))?;
    let root = Arc::new(std::fs::read(state.join("root.cer"))?);
    let authority = SocketAddr::new(config.published_ip, config.port).to_string();
    let config = Arc::new(config);
    let tls = Arc::new(tls);
    let (sender, receiver) = mpsc::sync_channel::<(TcpStream, Instant)>(16);
    let receiver = Arc::new(Mutex::new(receiver));
    for _ in 0..4 {
        let (receiver, tls, config, root, authority) = (
            receiver.clone(),
            tls.clone(),
            config.clone(),
            root.clone(),
            authority.clone(),
        );
        std::thread::spawn(move || loop {
            let next = match receiver.lock() {
                Ok(receiver) => receiver.recv(),
                Err(_) => return,
            };
            let Ok((socket, accepted)) = next else { return };
            // Errors are deliberately not printed: no request headers or secrets in logs.
            let _ = handle(socket, accepted, tls.clone(), &config, &authority, &root);
        });
    }
    println!("School gateway certificate foundation listening at https://{authority}/pwa/ (classroom enrollment and relay not enabled)");
    for socket in listener.incoming() {
        let socket = socket?;
        let _ = sender.try_send((socket, Instant::now()));
    }
    Ok(())
}
struct DeadlineSocket {
    socket: TcpStream,
    deadline: Instant,
}
impl Read for DeadlineSocket {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "header deadline"))?;
        self.socket.set_read_timeout(Some(remaining))?;
        self.socket.read(buf)
    }
}
impl Write for DeadlineSocket {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "write deadline"))?;
        self.socket.set_write_timeout(Some(remaining))?;
        self.socket.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}
fn handle(
    socket: TcpStream,
    accepted: Instant,
    tls: Arc<rustls::ServerConfig>,
    config: &SchoolConfig,
    authority: &str,
    root: &[u8],
) -> Result<()> {
    let socket = DeadlineSocket {
        socket,
        deadline: accepted + Duration::from_secs(5),
    };
    let mut stream = rustls::StreamOwned::new(rustls::ServerConnection::new(tls)?, socket);
    let mut bytes = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= 8192 {
            return Err("headers too large".into());
        }
        stream.read_exact(&mut byte)?;
        bytes.push(byte[0]);
    }
    let request = std::str::from_utf8(&bytes)?;
    let mut lines = request.split("\r\n");
    let first = lines.next().ok_or("missing request")?;
    let fields = first.split(' ').collect::<Vec<_>>();
    if fields.len() != 3 || fields[0] != "GET" || fields[2] != "HTTP/1.1" {
        return Err("unsupported request".into());
    }
    let mut host = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or("invalid header")?;
        if name.eq_ignore_ascii_case("host") {
            if host.replace(value.trim()).is_some() {
                return Err("duplicate host".into());
            }
        }
        if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value.trim() != "0")
        {
            return Err("GET body unsupported".into());
        }
    }
    if host != Some(authority) {
        return Err("unexpected authority".into());
    }
    let (status, content_type, body) = match fields[1] {
        "/pwa/api/v1/rooms" => (200, "application/json", serde_json::to_vec(&serde_json::json!({"schema":1,"schoolId":config.school_id,"schoolName":config.school_name,"rooms":[],"enrollmentEnabled":false,"relayEnabled":false}))?),
        "/pwa/root.cer" => (200, "application/pkix-cert", root.to_vec()),
        "/pwa/" => (200, "text/html; charset=utf-8", "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>未来教室 · 学校入口</title><h1>学校 HTTPS 入口已就绪</h1><p>当前为证书基础组件，教室登记、选班、教师中转和 PWA 尚未启用。</p><p>首次手机信任需要管理员提供学校根证书并核对指纹，不能忽略证书警告登录。</p></html>".as_bytes().to_vec()),
        _ => (404, "text/plain; charset=utf-8", b"Not found".to_vec()),
    };
    write!(stream, "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'; base-uri 'none'\r\nConnection: close\r\n\r\n", if status == 200 {"OK"} else {"Not Found"}, body.len())?;
    stream.write_all(&body)?;
    stream.conn.send_close_notify();
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
pub(super) fn test_https_directory(
    _config: &SchoolConfig,
    tls: rustls::ServerConfig,
    state: &Path,
) {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let config: SchoolConfig =
        serde_json::from_slice(&std::fs::read(state.join("school.json")).unwrap()).unwrap();
    let authority = SocketAddr::new(config.published_ip, config.port).to_string();
    let authority_request = authority.clone();
    let ip = config.published_ip;
    let server = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        handle(
            socket,
            Instant::now(),
            Arc::new(tls),
            &config,
            &authority,
            &[],
        )
        .unwrap();
    });
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(
            CertificateDer::from_pem_slice(&std::fs::read(state.join("root.pem")).unwrap())
                .unwrap(),
        )
        .unwrap();
    let client = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut stream = rustls::StreamOwned::new(
        rustls::ClientConnection::new(Arc::new(client), ip.into()).unwrap(),
        socket,
    );
    write!(
        stream,
        "GET /pwa/api/v1/rooms HTTP/1.1\r\nHost: {authority_request}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("\"rooms\":[]"));
    assert!(response.contains("\"relayEnabled\":false"));
    server.join().unwrap();
}
