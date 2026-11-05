//! Two exact listeners share the original IOCP, SSPI worker and four slots.
//! Only the LAN listener is replaced; pinned AcceptEx debt is drained first.
use super::*;
use fc_domain::https_settings::{HttpsConfiguration, HttpsSourceNetworks};
use std::net::SocketAddr;

impl Resources {
    pub(super) fn bare_accept_mut(&mut self, index: usize) -> Option<&mut accept::AcceptSlot> {
        if index == 0 {
            self.accept.as_mut()
        } else if index == 1 {
            self.lan_accept.as_mut()
        } else {
            self.update_accept.as_mut()
        }
    }
}
impl Owner {
    pub(super) fn listener(&self, index: usize) -> SOCKET {
        if index == 0 {
            self.resources.listener
        } else if index == 1 {
            self.resources.lan_listener
        } else {
            self.resources.update_listener
        }
    }
    pub(super) fn listener_key(index: usize) -> usize {
        if index == 0 {
            1
        } else {
            CONNECTION_SLOTS + index + 1
        }
    }
    pub(super) fn bare_accept(&self, index: usize) -> Option<&accept::AcceptSlot> {
        if index == 0 {
            self.resources.accept.as_ref()
        } else if index == 1 {
            self.resources.lan_accept.as_ref()
        } else {
            self.resources.update_accept.as_ref()
        }
    }
    pub(super) fn listener_family(&self, index: usize) -> ADDRESS_FAMILY {
        if index == 0 {
            return self.address_family();
        }
        match if index == 1 {
            self.lan_address
        } else {
            self.update_address
        } {
            Some(address) if address.is_ipv6() => AF_INET6,
            _ => AF_INET,
        }
    }
    pub(super) fn source_policy(&self, index: usize) -> Option<&HttpsSourceNetworks> {
        if index == 0 {
            self.source_networks.as_ref()
        } else {
            (if index == 1 {
                &self.lan_configuration
            } else {
                &self.update_configuration
            })
            .as_ref()
            .and_then(|c| c.source_networks.as_ref())
        }
    }
    pub(super) fn validate_lan(
        &self,
        configuration: &HttpsConfiguration,
    ) -> Result<SocketAddr, ConfiguredTlsError> {
        if !self.control_mode || self.stopping {
            return Err(ConfiguredTlsError::InvalidConfiguration);
        }
        let address = configured::endpoint(configuration)?;
        if address.ip().is_loopback() {
            return Err(ConfiguredTlsError::UnsupportedAddress);
        }
        let thumbprint = CertificateThumbprint::parse(&configuration.certificate.to_hex())
            .map_err(|_| ConfiguredTlsError::InvalidConfiguration)?;
        super::super::credentials::verify_configured_host(thumbprint, configuration.host.as_str())
            .map_err(|_| ConfiguredTlsError::Certificate)?;
        Ok(address)
    }
    pub(super) fn start_lan(&mut self, configuration: HttpsConfiguration) -> Result<(), String> {
        let address =
            configured::endpoint(&configuration).map_err(|e| format!("LAN endpoint: {e:?}"))?;
        self.lan_address = Some(address);
        self.lan_configuration = Some(configuration);
        // SAFETY: this one owner retains both listeners and their original
        // pinned operations; no second Winsock/IOCP/SSPI domain is created.
        let result = unsafe {
            (|| {
                self.resources.lan_listener = WSASocketW(
                    if address.is_ipv4() {
                        AF_INET.0
                    } else {
                        AF_INET6.0
                    } as i32,
                    SOCK_STREAM.0,
                    IPPROTO_TCP.0,
                    None,
                    0,
                    WSA_FLAG_OVERLAPPED,
                )
                .map_err(|e| format!("LAN listener: {e}"))?;
                check(
                    setsockopt(
                        self.resources.lan_listener,
                        SOL_SOCKET,
                        SO_EXCLUSIVEADDRUSE,
                        Some(&1i32.to_ne_bytes()),
                    ),
                    "LAN exclusive listener",
                )?;
                configured::bind_exact(self.resources.lan_listener, address)?;
                check(
                    listen(self.resources.lan_listener, (CONNECTION_SLOTS - 1) as i32),
                    "LAN listen",
                )?;
                CreateIoCompletionPort(
                    socket_handle(self.resources.lan_listener),
                    Some(self.resources.port),
                    Self::listener_key(1),
                    1,
                )
                .map_err(|e| format!("LAN listener IOCP: {e}"))?;
                let mut accept: LPFN_ACCEPTEX = None;
                let mut bytes = 0;
                check(
                    WSAIoctl(
                        self.resources.lan_listener,
                        SIO_GET_EXTENSION_FUNCTION_POINTER,
                        Some(ptr::from_ref(&WSAID_ACCEPTEX).cast()),
                        size_of::<windows::core::GUID>() as u32,
                        Some(ptr::from_mut(&mut accept).cast()),
                        size_of::<LPFN_ACCEPTEX>() as u32,
                        &mut bytes,
                        None,
                        None,
                    ),
                    "LAN AcceptEx lookup",
                )?;
                let accept = accept.ok_or("LAN AcceptEx unavailable")?;
                self.resources.lan_accept = Some(accept::AcceptSlot::new(Some(accept)));
                self.resources.lan_accept.as_mut().unwrap().serial = 1 << 63;
                self.post_bare_accept_for(1)
            })()
        };
        if result.is_err() {
            // Keep debt on failed post/close; replacement is forbidden until
            // its actual completion and checked socket return are observed.
            let cancel = self.cancel_lan_accept();
            let close = close(&mut self.resources.lan_listener, true);
            if cancel.is_err() || close.is_err() {
                OWNER_FAULTED.store(true, Ordering::Release);
                return Err("LAN startup cleanup retained original owner".into());
            }
            if self
                .resources
                .lan_accept
                .as_ref()
                .is_none_or(accept::AcceptSlot::released)
            {
                self.resources.lan_accept = None;
                self.lan_configuration = None;
            }
        }
        result
    }
    pub(super) fn cancel_lan_accept(&mut self) -> Result<(), String> {
        self.cancel_bare_accept_for(1, false)
    }
    pub(super) fn configure_lan(
        &mut self,
        configuration: Option<&HttpsConfiguration>,
    ) -> Result<(), ConfiguredTlsError> {
        if !self.control_mode || self.stopping {
            return Err(ConfiguredTlsError::InvalidConfiguration);
        }
        if self
            .pending_lan
            .as_ref()
            .is_some_and(|desired| desired.as_ref() == configuration)
            || (self.pending_lan.is_none()
                && self.lan_error.is_none()
                && self.lan_configuration.as_ref() == configuration)
        {
            return Ok(());
        }
        if let Some(configuration) = configuration {
            self.validate_lan(configuration)?;
        }
        self.pending_lan = Some(configuration.cloned());
        self.lan_error = None;
        self.cancel_lan_accept()
            .map_err(|_| ConfiguredTlsError::Cleanup)?;
        close(&mut self.resources.lan_listener, true).map_err(|_| ConfiguredTlsError::Cleanup)?;
        for index in 0..CONNECTION_SLOTS {
            if self.resources.slots[index].listener_index == 1
                && self.resources.slots[index].phase != Phase::Ready
            {
                self.close_slot(index, true)
                    .map_err(|_| ConfiguredTlsError::Cleanup)?;
            }
        }
        self.apply_lan_change()
            .map_err(|_| ConfiguredTlsError::Native)
    }
    pub(super) fn apply_lan_change(&mut self) -> Result<(), String> {
        if self.pending_lan.is_none() || self.stopping {
            return Ok(());
        }
        if !self
            .resources
            .lan_accept
            .as_ref()
            .is_none_or(accept::AcceptSlot::released)
            || self
                .resources
                .slots
                .iter()
                .any(|s| s.listener_index == 1 && s.phase != Phase::Ready)
        {
            return Ok(());
        }
        self.resources.lan_accept = None;
        self.lan_configuration = None;
        let worker = self.resources.worker.as_ref().ok_or("missing TLS worker")?;
        if !self.lan_credential_pending {
            let target = self.pending_lan.as_ref().and_then(Option::as_ref).cloned();
            let configuration = target
                .as_ref()
                .map(|c| super::super::CredentialConfiguration {
                    thumbprint: CertificateThumbprint::parse(&c.certificate.to_hex())
                        .expect("validated certificate identity"),
                    host: c.host.as_str().to_owned(),
                });
            worker
                .configure_lan(configuration)
                .map_err(|e| format!("LAN credential mailbox: {e:?}"))?;
            self.lan_credential_target = target;
            self.lan_credential_pending = true;
            return Ok(());
        }
        let Some(result) = worker.poll_lan_configured() else {
            return Ok(());
        };
        self.lan_credential_pending = false;
        if let Err(error) = result {
            self.lan_credential_target = None;
            self.pending_lan = None;
            let owner_fault = matches!(
                error,
                super::super::TlsError::Cleanup(_)
                    | super::super::TlsError::ThreadPanic
                    | super::super::TlsError::Faulted
                    | super::super::TlsError::UnexpectedCompletion
            );
            self.lan_error = Some(if owner_fault {
                ConfiguredTlsError::Cleanup
            } else {
                ConfiguredTlsError::Certificate
            });
            if owner_fault {
                OWNER_FAULTED.store(true, Ordering::Release);
            }
            return Err(format!("LAN credential readiness: {error:?}"));
        }
        // A later negative/configuration request can supersede a slow provider
        // call. Never bind it with a credential prepared for the old target.
        if self.pending_lan.as_ref().and_then(Option::as_ref) != self.lan_credential_target.as_ref()
        {
            self.lan_credential_target = None;
            return Ok(());
        }
        self.pending_lan = None;
        if let Some(configuration) = self.lan_credential_target.take() {
            if let Err(error) = self.start_lan(configuration) {
                self.lan_error = Some(ConfiguredTlsError::Native);
                return Err(error);
            }
        }
        Ok(())
    }
}
