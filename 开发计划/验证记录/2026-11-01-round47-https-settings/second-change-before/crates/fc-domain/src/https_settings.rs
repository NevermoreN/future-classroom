//! Installation-local HTTPS configuration. These values never open a listener,
//! select a different Windows identity, or confer certificate/readiness authority.
use crate::{identity::IdentityError, ids::Revision};
use std::net::IpAddr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpsSettingsError {
    InvalidHost,
    InvalidPort,
    InvalidCertificate,
    InvalidState,
    RevisionConflict { current: Revision },
    IdentityRevisionConflict { current: Revision },
    Identity(IdentityError),
    ConfigurationUnavailable,
    Overflow,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsHost(String);
impl HttpsHost {
    pub fn parse(value: &str) -> Result<Self, HttpsSettingsError> {
        if value.is_empty() || value.len() > 253 || !value.is_ascii() {
            return Err(HttpsSettingsError::InvalidHost);
        }
        let ip = value
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .unwrap_or(value);
        if let Ok(ip) = ip.parse::<IpAddr>() {
            if (value.starts_with('[') && !ip.is_ipv6())
                || ip.is_unspecified()
                || ip.is_multicast()
                || matches!(ip, IpAddr::V4(v) if v.is_broadcast())
            {
                return Err(HttpsSettingsError::InvalidHost);
            }
            return Ok(Self(ip.to_string()));
        }
        let dns = value.strip_suffix('.').unwrap_or(value);
        if dns.is_empty()
            || dns.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            || dns.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(HttpsSettingsError::InvalidHost);
        }
        Ok(Self(dns.to_ascii_lowercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn allocation_bytes(&self) -> usize {
        self.0.capacity()
    }
}
/// Exact Windows CurrentUser MY lookup identity, not a signature algorithm or
/// proof of trust. No store path, key container, PFX or password is accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpsCertificateRef([u8; 20]);
impl HttpsCertificateRef {
    pub fn parse(value: &str) -> Result<Self, HttpsSettingsError> {
        if value.len() != 40 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(HttpsSettingsError::InvalidCertificate);
        }
        let mut bytes = [0; 20];
        for (n, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[n * 2..n * 2 + 2], 16)
                .map_err(|_| HttpsSettingsError::InvalidCertificate)?;
        }
        Ok(Self(bytes))
    }
    pub fn bytes(&self) -> &[u8; 20] {
        &self.0
    }
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsConfiguration {
    pub host: HttpsHost,
    pub port: u16,
    pub certificate: HttpsCertificateRef,
}
impl HttpsConfiguration {
    pub fn new(
        host: HttpsHost,
        port: u16,
        certificate: HttpsCertificateRef,
    ) -> Result<Self, HttpsSettingsError> {
        let value = Self {
            host,
            port,
            certificate,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), HttpsSettingsError> {
        if self.port == 0 {
            return Err(HttpsSettingsError::InvalidPort);
        }
        if HttpsHost::parse(self.host.as_str())? != self.host {
            return Err(HttpsSettingsError::InvalidHost);
        }
        Ok(())
    }
    /// A display value only. This does not prove DNS, certificate name coverage,
    /// an IPv6 listener, a network allowlist, or a usable teacher connection.
    pub fn origin(&self) -> String {
        if self.host.as_str().contains(':') {
            format!("https://[{}]:{}", self.host.as_str(), self.port)
        } else {
            format!("https://{}:{}", self.host.as_str(), self.port)
        }
    }
    pub fn allocation_bytes(&self) -> usize {
        self.host.allocation_bytes()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpsSettings {
    pub revision: Revision,
    pub configuration: Option<HttpsConfiguration>,
}
impl HttpsSettings {
    pub fn validate(&self) -> Result<(), HttpsSettingsError> {
        if self.revision.get() == 0 && self.configuration.is_some() {
            return Err(HttpsSettingsError::InvalidState);
        }
        if let Some(c) = &self.configuration {
            c.validate()?;
        }
        Ok(())
    }
    pub fn transition(
        &self,
        configuration: Option<HttpsConfiguration>,
        expected: Revision,
    ) -> Result<Self, HttpsSettingsError> {
        self.validate()?;
        if expected != self.revision {
            return Err(HttpsSettingsError::RevisionConflict {
                current: self.revision,
            });
        }
        if let Some(c) = &configuration {
            c.validate()?;
        }
        Ok(Self {
            revision: self
                .revision
                .next()
                .map_err(|_| HttpsSettingsError::Overflow)?,
            configuration,
        })
    }
}
#[cfg(test)]
mod tests;
