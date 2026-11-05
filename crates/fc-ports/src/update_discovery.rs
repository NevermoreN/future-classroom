//! School membership proof, distinct from publisher trust and local install intent.
pub trait UpdateGroupAuthenticator {
    fn group_id(&self) -> [u8; 16];
    fn authenticate(&self, message: &[u8]) -> [u8; 32];
    fn verify(&self, message: &[u8], tag: &[u8; 32]) -> bool;
}
