//! Bounded, transport-free TUF verification. Run on the existing worker pool.
//!
//! The bootstrap root must come from the trusted installation, never a peer or
//! an imported update. No publishing private key is compiled into this module.
//! The caller owns durable trust/checkpoint persistence and installer eligibility;
//! a verified target is not authorization to install it.
use ring::{digest, signature};
use sigstore_crypto::{CryptoProvider, HashContext, SigningScheme};
use sigstore_tuf::TrustedMetadataSet;
use sigstore_types::HashAlgorithm;

pub const MAX_METADATA_BYTES: usize = 256 * 1024;
pub const MAX_METADATA_SET_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ROOT_UPDATES: usize = 32;
pub const MAX_TARGETS: usize = 16;
pub const MAX_PAYLOAD_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateVerificationError {
    Budget,
    InvalidMetadata,
    ClockRollback,
    ProviderConflict,
    InvalidTarget,
}

/// Borrowed untrusted metadata. File fetching and archive handling stay outside
/// this verifier; all four TUF roles are verified before a catalog is returned.
pub struct UpdateMetadata<'a> {
    pub roots: &'a [&'a [u8]],
    pub timestamp: &'a [u8],
    pub snapshot: &'a [u8],
    pub targets: &'a [u8],
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedUpdateTarget {
    path: String,
    length: u64,
    sha256: [u8; 32],
    custom: Option<serde_json::Value>,
}
impl VerifiedUpdateTarget {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn length(&self) -> u64 {
        self.length
    }
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
    /// Signed product-specific release fields still require domain validation.
    pub fn custom(&self) -> Option<&serde_json::Value> {
        self.custom.as_ref()
    }
}

/// Own this across refreshes: rebuilding from a root alone loses rollback floors.
/// Restoring a process must first replay its authenticated stored metadata before
/// considering peer candidates. This adapter does not persist school data.
pub struct UpdateTrust {
    trusted: TrustedMetadataSet,
    accepted_time: Option<i64>,
}
impl UpdateTrust {
    pub fn from_installed_root(root: &[u8]) -> Result<Self, UpdateVerificationError> {
        if root.len() > MAX_METADATA_BYTES {
            return Err(UpdateVerificationError::Budget);
        }
        preflight(root)?;
        install_provider()?;
        let trusted = TrustedMetadataSet::from_root(root)
            .map_err(|_| UpdateVerificationError::InvalidMetadata)?;
        Ok(Self {
            trusted,
            accepted_time: None,
        })
    }

    /// Re-establish rollback floors by replaying a stored, complete refresh
    /// from the immutable installation anchor, including every root rotation.
    /// The writer must atomically persist that chain, the three metadata files,
    /// and `accepted_time`; passing only a cached latest root is not restoration.
    /// Validation uses the previously accepted time so an expired authenticated
    /// checkpoint can still establish floors. A subsequent peer refresh must
    /// pass today's time and independently satisfy all current expiry checks.
    pub fn restore_from_checkpoint(
        installed_root: &[u8],
        checkpoint: UpdateMetadata<'_>,
        accepted_time: i64,
    ) -> Result<Self, UpdateVerificationError> {
        let mut trust = Self::from_installed_root(installed_root)?;
        trust.refresh(checkpoint, accepted_time)?;
        Ok(trust)
    }

    pub fn root_bytes(&self) -> &[u8] {
        self.trusted.root_bytes()
    }
    pub fn accepted_time(&self) -> Option<i64> {
        self.accepted_time
    }

    /// Commit verified state only after the complete refresh succeeds. A bad
    /// candidate cannot replace the prior catalog or erase its rollback floors.
    pub fn refresh(
        &mut self,
        metadata: UpdateMetadata<'_>,
        unix_seconds: i64,
    ) -> Result<Vec<VerifiedUpdateTarget>, UpdateVerificationError> {
        if metadata.roots.len() > MAX_ROOT_UPDATES {
            return Err(UpdateVerificationError::Budget);
        }
        let mut total = 0usize;
        for bytes in metadata.roots.iter().copied().chain([
            metadata.timestamp,
            metadata.snapshot,
            metadata.targets,
        ]) {
            if bytes.len() > MAX_METADATA_BYTES {
                return Err(UpdateVerificationError::Budget);
            }
            total = total
                .checked_add(bytes.len())
                .ok_or(UpdateVerificationError::Budget)?;
        }
        if total > MAX_METADATA_SET_BYTES {
            return Err(UpdateVerificationError::Budget);
        }
        for bytes in metadata.roots.iter().copied().chain([
            metadata.timestamp,
            metadata.snapshot,
            metadata.targets,
        ]) {
            preflight(bytes)?;
        }
        if self
            .accepted_time
            .is_some_and(|accepted| unix_seconds < accepted)
        {
            return Err(UpdateVerificationError::ClockRollback);
        }
        let now = jiff::Timestamp::from_second(unix_seconds)
            .map_err(|_| UpdateVerificationError::InvalidMetadata)?;
        let mut candidate = self.trusted.clone();
        let invalid = |_| UpdateVerificationError::InvalidMetadata;
        for root in metadata.roots {
            candidate.update_root(root).map_err(invalid)?;
        }
        candidate.check_root_expired(now).map_err(invalid)?;
        // sigstore-tuf 0.15.1 uses strict `<` in Role::is_expired. Enforce the
        // inclusive product deadline on authenticated roles too: equality is
        // already expired, never one extra clock tick of update authority.
        if candidate.root().expires <= now {
            return Err(UpdateVerificationError::InvalidMetadata);
        }
        match candidate.update_timestamp(metadata.timestamp, now) {
            Ok(()) | Err(sigstore_tuf::Error::EqualVersion { .. }) => {}
            Err(_) => return Err(UpdateVerificationError::InvalidMetadata),
        }
        candidate.check_timestamp_expired(now).map_err(invalid)?;
        if candidate
            .timestamp()
            .is_some_and(|role| role.expires <= now)
        {
            return Err(UpdateVerificationError::InvalidMetadata);
        }
        candidate
            .update_snapshot(metadata.snapshot, now)
            .map_err(invalid)?;
        candidate.check_snapshot_expired(now).map_err(invalid)?;
        if candidate.snapshot().is_some_and(|role| role.expires <= now) {
            return Err(UpdateVerificationError::InvalidMetadata);
        }
        let targets = candidate
            .update_targets(metadata.targets, now)
            .map_err(invalid)?;
        if targets.expires <= now {
            return Err(UpdateVerificationError::InvalidMetadata);
        }
        // D14 initially publishes only top-level complete payloads, no delegation.
        if targets.targets.len() > MAX_TARGETS {
            return Err(UpdateVerificationError::Budget);
        }
        if targets.delegations.is_some() {
            return Err(UpdateVerificationError::InvalidTarget);
        }
        let verified = targets
            .targets
            .iter()
            .map(|(path, target)| {
                if !valid_target_path(path)
                    || target.length == 0
                    || target.length > MAX_PAYLOAD_BYTES
                {
                    return Err(UpdateVerificationError::InvalidTarget);
                }
                let sha256 = target
                    .hashes
                    .get("sha256")
                    .and_then(|value| decode_sha256(value))
                    .ok_or(UpdateVerificationError::InvalidTarget)?;
                Ok(VerifiedUpdateTarget {
                    path: path.clone(),
                    length: target.length,
                    sha256,
                    custom: target.custom.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.trusted = candidate;
        self.accepted_time = Some(unix_seconds);
        Ok(verified)
    }
}

fn valid_target_path(path: &str) -> bool {
    path.len() <= 200
        && path.ends_with("/payload.zip")
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}
fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let nibble = |b: u8| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        };
        *slot = nibble(pair[0])? * 16 + nibble(pair[1])?;
    }
    Some(out)
}

// The upstream parser uses serde_json::Value, which otherwise overwrites
// duplicate keys. Reject ambiguity and excessive nesting/items before passing
// the original bytes to TUF; this does not encode or canonicalize signed JSON.
fn preflight(bytes: &[u8]) -> Result<(), UpdateVerificationError> {
    use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
    struct Budget<'a> {
        depth: usize,
        remaining: &'a mut usize,
    }
    impl<'de> DeserializeSeed<'de> for Budget<'_> {
        type Value = ();
        fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
            if self.depth > 32 || *self.remaining == 0 {
                return Err(D::Error::custom("metadata structure budget"));
            }
            *self.remaining -= 1;
            deserializer.deserialize_any(self)
        }
    }
    impl<'de> Visitor<'de> for Budget<'_> {
        type Value = ();
        fn expecting(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            out.write_str("bounded unambiguous JSON")
        }
        fn visit_unit<E: Error>(self) -> Result<(), E> {
            Ok(())
        }
        fn visit_bool<E: Error>(self, _: bool) -> Result<(), E> {
            Ok(())
        }
        fn visit_i64<E: Error>(self, _: i64) -> Result<(), E> {
            Ok(())
        }
        fn visit_u64<E: Error>(self, _: u64) -> Result<(), E> {
            Ok(())
        }
        fn visit_f64<E: Error>(self, _: f64) -> Result<(), E> {
            Ok(())
        }
        fn visit_str<E: Error>(self, _: &str) -> Result<(), E> {
            Ok(())
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
            let mut keys = std::collections::BTreeSet::new();
            while let Some(key) = map.next_key::<String>()? {
                if !keys.insert(key) {
                    return Err(M::Error::custom("duplicate metadata key"));
                }
                map.next_value_seed(Budget {
                    depth: self.depth + 1,
                    remaining: &mut *self.remaining,
                })?;
            }
            Ok(())
        }
        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<(), S::Error> {
            while seq
                .next_element_seed(Budget {
                    depth: self.depth + 1,
                    remaining: &mut *self.remaining,
                })?
                .is_some()
            {}
            Ok(())
        }
    }
    let mut remaining = 4096;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    Budget {
        depth: 0,
        remaining: &mut remaining,
    }
    .deserialize(&mut deserializer)
    .and_then(|()| deserializer.end())
    .map_err(|_| UpdateVerificationError::InvalidMetadata)
}

// Use the library's supported provider interface with existing, maintained
// crypto implementations. This is verification-only: no signing/key generation,
// TLS client, async executor, filesystem access, or process-global worker threads.
#[derive(Debug)]
struct UpdateCrypto;
static UPDATE_CRYPTO: UpdateCrypto = UpdateCrypto;
fn install_provider() -> Result<(), UpdateVerificationError> {
    let expected: &'static dyn CryptoProvider = &UPDATE_CRYPTO;
    let _ = sigstore_crypto::install_default_provider(expected);
    if sigstore_crypto::try_default_provider().is_some_and(|active| {
        std::ptr::eq(
            active as *const dyn CryptoProvider as *const (),
            expected as *const dyn CryptoProvider as *const (),
        )
    }) {
        Ok(())
    } else {
        Err(UpdateVerificationError::ProviderConflict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use serde_json::{json, Value};

    const NOW: i64 = 1_783_296_000; // 2026-07-06 UTC; no dependency on the host clock.
    const FUTURE: &str = "2030-01-01T00:00:00Z";
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    fn key(seed: u8) -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
    }
    fn envelope(signed: Value, keys: &[(&str, &Ed25519KeyPair)]) -> Vec<u8> {
        // Use the selected TUF implementation's canonical encoding, not a local substitute.
        let canonical = sigstore_tuf::canonical_json::to_canonical_bytes(&signed).unwrap();
        let signatures = keys
            .iter()
            .map(|(id, key)| json!({ "keyid": id, "sig": hex(key.sign(&canonical).as_ref()) }))
            .collect::<Vec<_>>();
        serde_json::to_vec(&json!({ "signed": signed, "signatures": signatures })).unwrap()
    }
    fn root(version: u64, current: &Ed25519KeyPair, previous: Option<&Ed25519KeyPair>) -> Vec<u8> {
        let current_id = hex(current.public_key().as_ref());
        let previous_id = previous.map(|key| hex(key.public_key().as_ref()));
        let mut signers = vec![(current_id.as_str(), current)];
        if let (Some(previous), Some(id)) = (previous, previous_id.as_deref()) {
            signers.push((id, previous));
        }
        // Tests use one ephemeral key for roles; production key custody is deliberately separate.
        let mut keys = serde_json::Map::new();
        keys.insert(
            current_id.clone(),
            json!({ "keytype": "ed25519", "scheme": "ed25519",
            "keyval": { "public": hex(current.public_key().as_ref()) } }),
        );
        let role = json!({ "keyids": [current_id], "threshold": 1 });
        envelope(
            json!({ "_type": "root", "spec_version": "1.0.36", "version": version,
                "expires": FUTURE, "consistent_snapshot": false, "keys": keys,
                "roles": { "root": role, "targets": role, "snapshot": role, "timestamp": role }
            }),
            &signers,
        )
    }
    fn pin(bytes: &[u8], version: u64) -> Value {
        json!({ "version": version, "length": bytes.len(), "hashes": { "sha256": hex(&crate::sha256(bytes)) } })
    }
    struct Bundle {
        timestamp: Vec<u8>,
        snapshot: Vec<u8>,
        targets: Vec<u8>,
    }
    impl Bundle {
        fn new(key: &Ed25519KeyPair, version: u64, snapshot_version: u64, expires: &str) -> Self {
            let id = hex(key.public_key().as_ref());
            let targets = envelope(
                json!({ "_type": "targets", "spec_version": "1.0.36",
                    "version": version, "expires": expires,
                    "targets": { "stable/1/payload.zip": { "length": 9,
                        "hashes": { "sha256": hex(&crate::sha256(b"a payload")) },
                        "custom": { "product": "future-classroom", "releaseSequence": version } } }
                }),
                &[(id.as_str(), key)],
            );
            let snapshot = envelope(
                json!({ "_type": "snapshot", "spec_version": "1.0.36",
                    "version": snapshot_version, "expires": expires,
                    "meta": { "targets.json": pin(&targets, version) }
                }),
                &[(id.as_str(), key)],
            );
            let timestamp = envelope(
                json!({ "_type": "timestamp", "spec_version": "1.0.36",
                    "version": version, "expires": expires,
                    "meta": { "snapshot.json": pin(&snapshot, snapshot_version) }
                }),
                &[(id.as_str(), key)],
            );
            Self {
                timestamp,
                snapshot,
                targets,
            }
        }
        fn metadata<'a>(&'a self, roots: &'a [&'a [u8]]) -> UpdateMetadata<'a> {
            UpdateMetadata {
                roots,
                timestamp: &self.timestamp,
                snapshot: &self.snapshot,
                targets: &self.targets,
            }
        }
    }

    #[test]
    fn authentic_catalog_rejects_tampered_and_mixed_metadata_without_replacing_trust() {
        let key = key(1);
        let root = root(1, &key, None);
        let bundle = Bundle::new(&key, 1, 1, FUTURE);
        let mut trust = UpdateTrust::from_installed_root(&root).unwrap();
        let verified = trust.refresh(bundle.metadata(&[]), NOW).unwrap();
        assert_eq!(verified.len(), 1);
        assert_eq!(verified[0].sha256(), crate::sha256(b"a payload"));
        assert_eq!(verified[0].path(), "stable/1/payload.zip");
        let mut tampered = Bundle::new(&key, 2, 2, FUTURE);
        tampered.targets[10] ^= 1;
        assert_eq!(
            trust.refresh(tampered.metadata(&[]), NOW + 1),
            Err(UpdateVerificationError::InvalidMetadata)
        );
        assert_eq!(trust.accepted_time(), Some(NOW));
        let mut changed_field = Bundle::new(&key, 2, 2, FUTURE);
        let mut json: Value = serde_json::from_slice(&changed_field.targets).unwrap();
        json["signed"]["targets"]["stable/1/payload.zip"]["custom"]["releaseSequence"] = json!(999);
        changed_field.targets = serde_json::to_vec(&json).unwrap();
        assert!(trust.refresh(changed_field.metadata(&[]), NOW + 1).is_err());
        let mut mixed = Bundle::new(&key, 2, 2, FUTURE);
        mixed.snapshot = bundle.snapshot.clone();
        assert!(trust.refresh(mixed.metadata(&[]), NOW + 1).is_err());
        let impostor = super::tests::key(99);
        assert!(trust
            .refresh(Bundle::new(&impostor, 2, 2, FUTURE).metadata(&[]), NOW + 1)
            .is_err());
        assert!(trust.refresh(bundle.metadata(&[]), NOW).is_ok());
    }

    #[test]
    fn rollback_and_stale_equal_timestamp_are_rejected() {
        let key = key(2);
        let mut trust = UpdateTrust::from_installed_root(&root(1, &key, None)).unwrap();
        let bundle = Bundle::new(&key, 2, 2, "2027-01-01T00:00:00Z");
        trust.refresh(bundle.metadata(&[]), NOW).unwrap();
        let mut restored =
            UpdateTrust::restore_from_checkpoint(&root(1, &key, None), bundle.metadata(&[]), NOW)
                .unwrap();
        assert!(restored
            .refresh(Bundle::new(&key, 1, 1, FUTURE).metadata(&[]), NOW)
            .is_err());
        assert_eq!(restored.accepted_time(), Some(NOW));
        assert_eq!(
            trust.refresh(bundle.metadata(&[]), NOW - 1),
            Err(UpdateVerificationError::ClockRollback)
        );
        assert!(trust
            .refresh(Bundle::new(&key, 1, 1, FUTURE).metadata(&[]), NOW)
            .is_err());
        // A signed newer timestamp cannot pin an older snapshot version.
        assert!(trust
            .refresh(Bundle::new(&key, 3, 1, FUTURE).metadata(&[]), NOW)
            .is_err());
        assert!(trust.refresh(bundle.metadata(&[]), 1_798_761_600).is_err()); // Timestamp is exactly expired; the root is still fresh.
    }

    #[test]
    fn root_rotation_requires_old_and_new_signatures_and_exact_next_version() {
        let old = key(3);
        let new = key(4);
        let installed = root(1, &old, None);
        let mut trust = UpdateTrust::from_installed_root(&installed).unwrap();
        let bundle = Bundle::new(&new, 1, 1, FUTURE);
        let skipped = root(3, &new, Some(&old));
        assert!(trust.refresh(bundle.metadata(&[&skipped]), NOW).is_err());
        let unsigned_by_old = root(2, &new, None);
        assert!(trust
            .refresh(bundle.metadata(&[&unsigned_by_old]), NOW)
            .is_err());
        assert_eq!(trust.root_bytes(), installed);
        let rotated = root(2, &new, Some(&old));
        assert!(trust.refresh(bundle.metadata(&[&rotated]), NOW).is_ok());
        assert_eq!(trust.root_bytes(), rotated);
    }

    #[test]
    fn root_threshold_counts_distinct_authorized_signers() {
        let first = key(6);
        let second = key(7);
        let third = key(8);
        let mut signed =
            serde_json::from_slice::<Value>(&root(1, &first, None)).unwrap()["signed"].clone();
        let ids = [&first, &second, &third].map(|key| hex(key.public_key().as_ref()));
        for (key, id) in [&second, &third].into_iter().zip(&ids[1..]) {
            signed["keys"][id] = json!({ "keytype": "ed25519", "scheme": "ed25519",
                "keyval": { "public": hex(key.public_key().as_ref()) } });
        }
        signed["roles"]["root"] = json!({ "keyids": ids, "threshold": 2 });
        let missing = envelope(signed.clone(), &[(ids[0].as_str(), &first)]);
        assert!(UpdateTrust::from_installed_root(&missing).is_err());
        let duplicated = envelope(
            signed.clone(),
            &[(ids[0].as_str(), &first), (ids[0].as_str(), &first)],
        );
        assert!(UpdateTrust::from_installed_root(&duplicated).is_err());
        let valid = envelope(
            signed,
            &[(ids[0].as_str(), &first), (ids[1].as_str(), &second)],
        );
        assert!(UpdateTrust::from_installed_root(&valid).is_ok());
    }

    #[test]
    fn untrusted_byte_budgets_are_enforced_before_parsing() {
        let root = vec![0; MAX_METADATA_BYTES + 1];
        assert!(matches!(
            UpdateTrust::from_installed_root(&root),
            Err(UpdateVerificationError::Budget)
        ));
        let key = key(5);
        let mut trust =
            UpdateTrust::from_installed_root(&super::tests::root(1, &key, None)).unwrap();
        let bundle = Bundle::new(&key, 1, 1, FUTURE);
        let roots = vec![b"bad".as_slice(); MAX_ROOT_UPDATES + 1];
        assert_eq!(
            trust.refresh(bundle.metadata(&roots), NOW),
            Err(UpdateVerificationError::Budget)
        );
        assert!(preflight(br#"{"signed":{"version":1,"version":2}}"#).is_err());
        assert!(preflight(br#"{"signed":{"version":1,"ver\u0073ion":2}}"#).is_err());
        assert!(preflight(format!("{}0{}", "[".repeat(34), "]".repeat(34)).as_bytes()).is_err());
    }
}
struct UpdateHash(digest::Context);
impl HashContext for UpdateHash {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        self.0.finish().as_ref().to_vec()
    }
}
impl CryptoProvider for UpdateCrypto {
    fn hash(&self, algorithm: HashAlgorithm) -> sigstore_crypto::Result<Box<dyn HashContext>> {
        let algorithm = match algorithm {
            HashAlgorithm::Sha2256 => &digest::SHA256,
            HashAlgorithm::Sha2384 => &digest::SHA384,
            HashAlgorithm::Sha2512 => &digest::SHA512,
            _ => {
                return Err(sigstore_crypto::Error::UnsupportedAlgorithm(
                    "update hash".into(),
                ))
            }
        };
        Ok(Box::new(UpdateHash(digest::Context::new(algorithm))))
    }
    fn verify_raw(
        &self,
        scheme: SigningScheme,
        public: &[u8],
        data: &[u8],
        sig: &[u8],
    ) -> sigstore_crypto::Result<()> {
        if scheme != SigningScheme::Ed25519 {
            return Err(sigstore_crypto::Error::UnsupportedAlgorithm(
                "updates require Ed25519".into(),
            ));
        }
        signature::UnparsedPublicKey::new(&signature::ED25519, public)
            .verify(data, sig)
            .map_err(|_| sigstore_crypto::Error::Verification("update signature".into()))
    }
    fn verify_prehashed(
        &self,
        _: SigningScheme,
        _: &[u8],
        _: &[u8],
        _: &[u8],
    ) -> sigstore_crypto::Result<()> {
        Err(sigstore_crypto::Error::UnsupportedAlgorithm(
            "Ed25519 requires the original message".into(),
        ))
    }
}
