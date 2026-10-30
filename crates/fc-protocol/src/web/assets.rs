//! Bounded build manifest, never a filesystem path supplied by a request.
use crate::{parse, ProtocolError};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_ASSETS: usize = 64;
pub const MAX_BUNDLE_BYTES: u64 = 5 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ManifestDto {
    schema_version: u16,
    api_version: u16,
    ui_compatibility_version: u16,
    assets: Vec<AssetDto>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AssetDto {
    path: String,
    size_bytes: u64,
    sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub assets: Vec<Asset>,
}
pub fn valid_path(path: &str) -> bool {
    if path == "index.html" {
        return true;
    }
    let Some(name) = path.strip_prefix("assets/") else {
        return false;
    };
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        })
    {
        return false;
    }
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && mime(path).is_some()
}

pub fn mime(path: &str) -> Option<&'static str> {
    if path == "index.html" {
        return Some("text/html; charset=utf-8");
    }
    match path.rsplit_once('.')?.1 {
        "js" => Some("text/javascript; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "svg" => Some("image/svg+xml"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}
impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let dto: ManifestDto = parse(bytes)?;
        if (
            dto.schema_version,
            dto.api_version,
            dto.ui_compatibility_version,
        ) != (1, 1, 1)
        {
            return Err(ProtocolError::UnsupportedSchema);
        }
        if dto.assets.is_empty() || dto.assets.len() > MAX_ASSETS {
            return Err(ProtocolError::TooLarge);
        }
        let mut paths = BTreeSet::new();
        let mut total = 0u64;
        let mut assets = Vec::new();
        for asset in dto.assets {
            if !valid_path(&asset.path) || !paths.insert(asset.path.to_ascii_lowercase()) {
                return Err(ProtocolError::InvalidCommand);
            }
            total = total
                .checked_add(asset.size_bytes)
                .filter(|n| *n <= MAX_BUNDLE_BYTES)
                .ok_or(ProtocolError::TooLarge)?;
            if asset.size_bytes == 0 || asset.sha256.len() != 64 {
                return Err(ProtocolError::InvalidCommand);
            }
            let mut digest = [0; 32];
            for (out, pair) in digest
                .iter_mut()
                .zip(asset.sha256.as_bytes().chunks_exact(2))
            {
                let hex = |b| match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    _ => None,
                };
                *out = hex(pair[0]).ok_or(ProtocolError::InvalidCommand)? * 16
                    + hex(pair[1]).ok_or(ProtocolError::InvalidCommand)?;
            }
            assets.push(Asset {
                path: asset.path,
                size_bytes: asset.size_bytes,
                sha256: digest,
            });
        }
        if !paths.contains("index.html") {
            return Err(ProtocolError::InvalidCommand);
        }
        Ok(Self { assets })
    }
}
impl Asset {
    pub fn verify(&self, bytes: &[u8]) -> bool {
        self.size_bytes == bytes.len() as u64
            && self.sha256 == <[u8; 32]>::from(Sha256::digest(bytes))
    }
    pub fn etag(&self) -> String {
        let hex: String = self.sha256.iter().map(|b| format!("{b:02x}")).collect();
        format!("\"{hex}\"")
    }
    pub fn cache_control(&self) -> &'static str {
        let fingerprinted = self
            .path
            .strip_prefix("assets/")
            .and_then(|s| s.rsplit_once('.'))
            .and_then(|(stem, _)| stem.rsplit_once('-'))
            .is_some_and(|(_, hash)| {
                hash.len() >= 8 && hash.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            });
        if fingerprinted {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    fn entry(path: &str, size: u64) -> String {
        format!(r#"{{"path":"{path}","sizeBytes":{size},"sha256":"{HASH}"}}"#)
    }
    fn doc(entries: &[String]) -> String {
        format!(
            r#"{{"schemaVersion":1,"apiVersion":1,"uiCompatibilityVersion":1,"assets":[{}]}}"#,
            entries.join(",")
        )
    }
    #[test]
    fn manifest_integrity_versions_duplicates_and_windows_aliases() {
        let valid = doc(&[entry("index.html", 3), entry("assets/app-01234567.js", 3)]);
        let manifest = Manifest::parse(valid.as_bytes()).unwrap();
        assert!(manifest.assets[0].verify(b"abc"));
        assert!(!manifest.assets[0].verify(b"abd"));
        assert!(!manifest.assets[0].verify(b"abcd"));
        assert_eq!(manifest.assets[0].cache_control(), "no-cache");
        assert!(manifest.assets[1].cache_control().contains("immutable"));
        assert_eq!(
            Manifest::parse(
                valid
                    .replace("\"apiVersion\":1", "\"apiVersion\":2")
                    .as_bytes()
            ),
            Err(ProtocolError::UnsupportedSchema)
        );
        for entries in [
            vec![entry("index.html", 3), entry("index.html", 3)],
            vec![
                entry("index.html", 3),
                entry("assets/A.js", 3),
                entry("assets/a.js", 3),
            ],
            vec![entry("assets/app.js", 3)],
        ] {
            assert!(Manifest::parse(doc(&entries).as_bytes()).is_err());
        }
        assert!(Manifest::parse(
            valid
                .replace(
                    "\"schemaVersion\":1",
                    "\"schemaVersion\":1,\"schemaVersion\":1"
                )
                .as_bytes()
        )
        .is_err());
        assert!(Manifest::parse(
            valid
                .replace(
                    "\"apiVersion\":1",
                    "\"apiVersion\":1,\"pathOverride\":\"secret\""
                )
                .as_bytes()
        )
        .is_err());
    }
    #[test]
    fn manifest_caps_files_total_bytes_and_rejects_nonstatic_paths() {
        let mut entries = vec![entry("index.html", 3)];
        entries.extend((0..63).map(|i| entry(&format!("assets/a{i}.js"), 3)));
        assert!(Manifest::parse(doc(&entries).as_bytes()).is_ok());
        entries.push(entry("assets/overflow.js", 3));
        assert_eq!(
            Manifest::parse(doc(&entries).as_bytes()),
            Err(ProtocolError::TooLarge)
        );
        assert!(Manifest::parse(doc(&[entry("index.html", MAX_BUNDLE_BYTES)]).as_bytes()).is_ok());
        assert_eq!(
            Manifest::parse(
                doc(&[
                    entry("index.html", MAX_BUNDLE_BYTES),
                    entry("assets/a.js", 1)
                ])
                .as_bytes()
            ),
            Err(ProtocolError::TooLarge)
        );
        for path in [
            "",
            "../index.html",
            "assets/../index.html",
            "assets/%2e.js",
            "assets/a.js?v=1",
            "assets/a.js#x",
            "assets/a\\b.js",
            "api/v1/status",
            "assets/CON.js",
            "assets/LPT1.js",
            "assets/.hidden.js",
            "assets/app.js.map",
            "assets/a.JS",
            "assets/sub/a.js",
        ] {
            assert!(!valid_path(path), "{path}");
        }
    }
}
