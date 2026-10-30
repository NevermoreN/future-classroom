//! Immutable, verified static payloads. The source adapter supplies owned bytes
//! from a package; request paths are never opened on a filesystem. This module
//! starts no listener and supplies no evidence of TLS provisioning.
use fc_ports::http::{Header, ListenPrefix, RequestHead, ResponseHead, CHUNK_BYTES};
use fc_protocol::web::{
    assets::{Asset, Manifest},
    WebBoundary, WebError,
};
use std::{collections::BTreeMap, sync::Arc};

pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' blob: data:; media-src blob:; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'";
// RFC 9110 §13.1.2 uses weak comparison and permits lists; commas inside
// quoted opaque tags are data. Header bytes are already bounded by preflight.
fn cache_matches(mut value: &[u8], etag: &[u8]) -> Result<bool, StaticError> {
    let trim = |s: &[u8]| {
        s.iter()
            .position(|b| !b" \t".contains(b))
            .unwrap_or(s.len())
    };
    value = &value[trim(value)..];
    if value.first() == Some(&b'*') {
        return if value[1..].iter().all(|b| b" \t".contains(b)) {
            Ok(true)
        } else {
            Err(StaticError::Boundary(WebError::Headers))
        };
    }
    let mut matched = false;
    let mut tags = 0;
    while !value.is_empty() {
        // Ignore empty list elements, as required by RFC 9110 §5.6.1.
        if value[0] == b',' {
            value = &value[1..];
            value = &value[trim(value)..];
            continue;
        }
        value = value.strip_prefix(b"W/").unwrap_or(value);
        if value.first() != Some(&b'"') {
            return Err(StaticError::Boundary(WebError::Headers));
        }
        let end = value[1..]
            .iter()
            .position(|b| *b == b'"')
            .map(|n| n + 1)
            .ok_or(StaticError::Boundary(WebError::Headers))?;
        if !value[1..end].iter().all(|b| *b == 0x21 || *b >= 0x23) {
            return Err(StaticError::Boundary(WebError::Headers));
        }
        matched |= &value[..=end] == etag;
        tags += 1;
        value = &value[end + 1..];
        value = &value[trim(value)..];
        if !value.is_empty() && value[0] != b',' {
            return Err(StaticError::Boundary(WebError::Headers));
        }
    }
    if tags == 0 {
        return Err(StaticError::Boundary(WebError::Headers));
    }
    Ok(matched)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticError {
    InvalidManifest,
    PayloadMismatch,
    Boundary(WebError),
    NotFound,
}
struct PreparedAsset {
    metadata: Asset,
    bytes: Arc<[u8]>,
}
pub struct StaticAssets {
    boundary: WebBoundary,
    assets: BTreeMap<String, PreparedAsset>,
}
pub struct StaticResponse {
    pub head: ResponseHead,
    pub body: StaticBody,
}
/// Cloning a response source retains immutable shared bytes, never copies an
/// asset per connection. The native adapter copies at most one 64KiB piece.
pub struct StaticBody {
    bytes: Arc<[u8]>,
    offset: usize,
}
impl StaticBody {
    pub fn next_chunk(&mut self) -> Option<&[u8]> {
        if self.offset == self.bytes.len() {
            return None;
        }
        let start = self.offset;
        self.offset = (start + CHUNK_BYTES).min(self.bytes.len());
        Some(&self.bytes[start..self.offset])
    }
}
impl StaticAssets {
    pub fn new(
        prefix: &ListenPrefix,
        manifest_bytes: &[u8],
        payloads: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> Result<Self, StaticError> {
        let manifest = Manifest::parse(manifest_bytes).map_err(|_| StaticError::InvalidManifest)?;
        let boundary = WebBoundary::new(prefix).map_err(StaticError::Boundary)?;
        let mut expected: BTreeMap<_, _> = manifest
            .assets
            .into_iter()
            .map(|a| (a.path.clone(), a))
            .collect();
        let mut assets = BTreeMap::new();
        for (path, bytes) in payloads {
            let metadata = expected.remove(&path).ok_or(StaticError::PayloadMismatch)?;
            if !metadata.verify(&bytes) {
                return Err(StaticError::PayloadMismatch);
            }
            assets.insert(
                path,
                PreparedAsset {
                    metadata,
                    bytes: Arc::from(bytes),
                },
            );
        }
        if !expected.is_empty() {
            return Err(StaticError::PayloadMismatch);
        }
        Ok(Self { boundary, assets })
    }
    pub fn dispatch(&self, head: &RequestHead, body: &[u8]) -> Result<StaticResponse, StaticError> {
        let path = self
            .boundary
            .static_path(head)
            .map_err(StaticError::Boundary)?;
        if !body.is_empty() {
            return Err(StaticError::Boundary(WebError::Body));
        }
        let asset = self.assets.get(path).ok_or(StaticError::NotFound)?;
        let etag = asset.metadata.etag();
        let mut matches = head
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("if-none-match"));
        let cached = matches
            .next()
            .map(|h| cache_matches(&h.value, etag.as_bytes()))
            .transpose()?
            .unwrap_or(false);
        if matches.next().is_some() {
            return Err(StaticError::Boundary(WebError::Headers));
        }
        let bytes = if cached {
            Arc::from([])
        } else {
            asset.bytes.clone()
        };
        let header = |name: &str, value: &str| Header {
            name: name.into(),
            value: value.as_bytes().to_vec(),
        };
        Ok(StaticResponse {
            head: ResponseHead {
                status: if cached { 304 } else { 200 },
                content_length: bytes.len() as u64,
                headers: vec![
                    header(
                        "Content-Type",
                        fc_protocol::web::assets::mime(path).ok_or(StaticError::InvalidManifest)?,
                    ),
                    header("Cache-Control", asset.metadata.cache_control()),
                    header("ETag", &etag),
                    header("X-Content-Type-Options", "nosniff"),
                    header("Content-Security-Policy", CONTENT_SECURITY_POLICY),
                    header("Referrer-Policy", "no-referrer"),
                    header(
                        "Permissions-Policy",
                        "camera=(), geolocation=(), microphone=(self)",
                    ),
                ],
            },
            body: StaticBody { bytes, offset: 0 },
        })
    }
}

#[cfg(test)]
mod tests;
