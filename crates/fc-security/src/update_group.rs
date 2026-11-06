//! Custom group keys isolate peer discovery; the default LAN group is public.
//! Neither group mode authorizes installation.
use fc_ports::update_discovery::UpdateGroupAuthenticator;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const DOMAIN: &[u8] = b"FutureClassroom.UpdateDiscovery.v1\0";

/// Custom keys come from the trusted deployment adapter and use CurrentUser
/// DPAPI persistence. The public LAN protocol key needs no secret persistence.
/// Neither mode is Debug or Serialize.
pub struct UpdateGroupKey {
    group: [u8; 16],
    secret: Zeroizing<[u8; 32]>,
}
impl UpdateGroupKey {
    /// Stable, public protocol namespace for installation-free LAN discovery.
    /// Anyone can derive these bytes: this HMAC is not a security boundary.
    /// Official TUF signatures and native source eligibility authorize updates;
    /// the discovery-advertised pin still binds the subsequent TLS endpoint.
    pub fn public_lan() -> Self {
        let digest = Sha256::digest(b"FutureClassroom.UpdatePublicLan.Group.v1\0");
        let mut group = [0; 16];
        group.copy_from_slice(&digest[..16]);
        Self {
            group,
            secret: Zeroizing::new(
                Sha256::digest(b"FutureClassroom.UpdatePublicLan.Hmac.v1\0").into(),
            ),
        }
    }
    pub fn new(group: [u8; 16], secret: [u8; 32]) -> Option<Self> {
        if group == [0; 16] || secret == [0; 32] {
            return None;
        }
        Some(Self {
            group,
            secret: Zeroizing::new(secret),
        })
    }
    fn mac(&self, message: &[u8]) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.as_ref())
            .expect("HMAC accepts a fixed 32-byte key");
        mac.update(DOMAIN);
        mac.update(&self.group);
        mac.update(message);
        mac
    }
}
impl UpdateGroupAuthenticator for UpdateGroupKey {
    fn group_id(&self) -> [u8; 16] {
        self.group
    }
    fn authenticate(&self, message: &[u8]) -> [u8; 32] {
        self.mac(message).finalize().into_bytes().into()
    }
    fn verify(&self, message: &[u8], tag: &[u8; 32]) -> bool {
        self.mac(message).verify_slice(tag).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_lan_instances_interoperate_without_joining_private_groups() {
        let first = UpdateGroupKey::public_lan();
        let second = UpdateGroupKey::public_lan();
        assert_ne!(first.group_id(), [0; 16]);
        assert_eq!(first.group_id(), second.group_id());
        let message = b"independent LAN discovery instance";
        let public_tag = first.authenticate(message);
        assert!(second.verify(message, &public_tag));
        assert!(!second.verify(b"different discovery", &public_tag));
        // Even a private group using the public ID remains isolated by its key.
        let private = UpdateGroupKey::new(first.group_id(), [3; 32]).unwrap();
        assert!(!private.verify(message, &public_tag));
        assert!(!first.verify(message, &private.authenticate(message)));
    }
    #[test]
    fn proof_binds_group_message_and_key() {
        let a = UpdateGroupKey::new([1; 16], [2; 32]).unwrap();
        let tag = a.authenticate(b"query");
        assert!(a.verify(b"query", &tag));
        assert!(!a.verify(b"reply", &tag));
        assert!(!UpdateGroupKey::new([3; 16], [2; 32])
            .unwrap()
            .verify(b"query", &tag));
        assert!(!UpdateGroupKey::new([1; 16], [3; 32])
            .unwrap()
            .verify(b"query", &tag));
        assert!(UpdateGroupKey::new([0; 16], [2; 32]).is_none());
        assert!(UpdateGroupKey::new([1; 16], [0; 32]).is_none());
    }
}
