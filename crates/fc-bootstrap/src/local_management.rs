//! Process-owned discovery for the verified loopback management listener.
//! No URL or path supplied by a browser can redirect the tray's browser action.
use std::sync::atomic::{AtomicU16, Ordering};

static PORT: AtomicU16 = AtomicU16::new(0);

pub fn url(settings: bool) -> Option<String> {
    let port = PORT.load(Ordering::Acquire);
    (port != 0).then(|| {
        format!(
            "https://127.0.0.1:{port}/fc/#{}",
            if settings {
                "/admin/display"
            } else {
                "/admin/status"
            }
        )
    })
}

/// The network owner publishes only after native bind/TLS readiness and clears
/// before stopping admission. This does not grant application authorization.
pub(crate) fn publish(port: u16) {
    PORT.store(port, Ordering::Release);
}

#[cfg(test)]
mod tests {
    #[test]
    fn route_is_always_a_loopback_https_origin() {
        super::publish(18443);
        assert_eq!(
            super::url(true).as_deref(),
            Some("https://127.0.0.1:18443/fc/#/admin/display")
        );
        assert_eq!(
            super::url(false).as_deref(),
            Some("https://127.0.0.1:18443/fc/#/admin/status")
        );
        super::publish(0);
        assert!(super::url(true).is_none());
    }
}
