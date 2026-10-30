//! Development integration check for a caller-owned, freshly built web/dist.
//! This is not a production filesystem adapter or a listener.
use fc_bootstrap::web_assets::{StaticAssets, StaticError};
use fc_ports::http::{Header, ListenPrefix, RequestHead, TokenIssuer};
use fc_protocol::web::{assets::Manifest, WebError};
use std::{fs, io::Read, path::Path};

fn bounded_file(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("expected a bounded regular build file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("build file grew beyond its bound".into());
    }
    Ok(bytes)
}
fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let root = args.next().ok_or("expected an owned web/dist directory")?;
    if args.next().is_some() {
        return Err("expected exactly one build directory".into());
    }
    let root = Path::new(&root);
    let manifest_bytes = bounded_file(&root.join("fc-assets.json"), 65536)?;
    let manifest = Manifest::parse(&manifest_bytes).map_err(|e| format!("manifest: {e:?}"))?;
    let mut payloads = Vec::new();
    for asset in &manifest.assets {
        payloads.push((
            asset.path.clone(),
            bounded_file(&root.join(&asset.path), asset.size_bytes)?,
        ));
    }
    let prefix = ListenPrefix::https("https://board.synthetic:443/fc/")
        .map_err(|e| format!("prefix: {e:?}"))?;
    let assets = StaticAssets::new(&prefix, &manifest_bytes, payloads)
        .map_err(|e| format!("payloads: {e:?}"))?;
    let mut issuer = TokenIssuer::new().map_err(|e| format!("issuer: {e:?}"))?;
    let mut request = |path: &str| -> Result<RequestHead, String> {
        Ok(RequestHead {
            token: issuer.issue(0).map_err(|e| format!("token: {e:?}"))?,
            method: "GET".into(),
            target: format!("/fc/{path}"),
            headers: vec![Header {
                name: "Host".into(),
                value: b"board.synthetic".to_vec(),
            }],
            tls: true,
            content_length: None,
            has_body: false,
        })
    };
    let mut total = 0;
    for asset in &manifest.assets {
        let head = request(&asset.path)?;
        let mut response = assets
            .dispatch(&head, b"")
            .map_err(|e| format!("dispatch: {e:?}"))?;
        response
            .head
            .validate()
            .map_err(|e| format!("response: {e:?}"))?;
        let mut body = Vec::new();
        while let Some(chunk) = response.body.next_chunk() {
            if chunk.len() > 65536 {
                return Err("oversized response chunk".into());
            }
            body.extend_from_slice(chunk);
        }
        if response.head.status != 200 || !asset.verify(&body) {
            return Err("served payload mismatch".into());
        }
        total += body.len();
        let mut cached = head;
        cached.headers.push(Header {
            name: "If-None-Match".into(),
            value: asset.etag().into_bytes(),
        });
        let mut response = assets
            .dispatch(&cached, b"")
            .map_err(|e| format!("cache: {e:?}"))?;
        if response.head.status != 304 || response.body.next_chunk().is_some() {
            return Err("invalid cache response".into());
        }
    }
    if !matches!(
        assets.dispatch(&request("api/v1/status")?, b""),
        Err(StaticError::Boundary(WebError::NotFound))
    ) {
        return Err("API unexpectedly routed to a static file".into());
    }
    println!("STATIC_BUILD_INTEGRATION_PASS assets={} bytes={total} synthetic_metadata_only=true listener_started=false", manifest.assets.len());
    Ok(())
}
