//! Independent update listener, sharing the original IOCP and SSPI budgets.
use super::*;
use fc_domain::https_settings::HttpsConfiguration;
use std::net::SocketAddr;
impl Owner {
    pub(super) fn validate_update(
        &self,
        configuration: &HttpsConfiguration,
    ) -> Result<SocketAddr, ConfiguredTlsError> {
        if !self.control_mode || self.stopping {
            return Err(ConfiguredTlsError::InvalidConfiguration);
        }
        let address = configured::endpoint(configuration)?;
        if !matches!(address.ip(), std::net::IpAddr::V4(ip) if ip.is_private()) {
            return Err(ConfiguredTlsError::UnsupportedAddress);
        }
        let thumbprint = CertificateThumbprint::parse(&configuration.certificate.to_hex())
            .map_err(|_| ConfiguredTlsError::InvalidConfiguration)?;
        super::super::credentials::verify_configured_host(thumbprint, configuration.host.as_str())
            .map_err(|_| ConfiguredTlsError::Certificate)?;
        Ok(address)
    }
    pub(super) fn start_update(&mut self, configuration: HttpsConfiguration) -> Result<(), String> {
        let address =
            configured::endpoint(&configuration).map_err(|e| format!("update endpoint: {e:?}"))?;
        self.update_address = Some(address);
        self.update_configuration = Some(configuration);
        // SAFETY: this one owner retains all listeners and their original
        // pinned operations; no second Winsock/IOCP/SSPI domain is created.
        let result = unsafe {
            (|| {
                self.resources.update_listener = WSASocketW(
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
                .map_err(|e| format!("update listener: {e}"))?;
                check(
                    setsockopt(
                        self.resources.update_listener,
                        SOL_SOCKET,
                        SO_EXCLUSIVEADDRUSE,
                        Some(&1i32.to_ne_bytes()),
                    ),
                    "update exclusive listener",
                )?;
                configured::bind_exact(self.resources.update_listener, address)?;
                check(
                    listen(
                        self.resources.update_listener,
                        (CONNECTION_SLOTS - 1) as i32,
                    ),
                    "update listen",
                )?;
                CreateIoCompletionPort(
                    socket_handle(self.resources.update_listener),
                    Some(self.resources.port),
                    Self::listener_key(2),
                    1,
                )
                .map_err(|e| format!("update listener IOCP: {e}"))?;
                let mut accept: LPFN_ACCEPTEX = None;
                let mut bytes = 0;
                check(
                    WSAIoctl(
                        self.resources.update_listener,
                        SIO_GET_EXTENSION_FUNCTION_POINTER,
                        Some(ptr::from_ref(&WSAID_ACCEPTEX).cast()),
                        size_of::<windows::core::GUID>() as u32,
                        Some(ptr::from_mut(&mut accept).cast()),
                        size_of::<LPFN_ACCEPTEX>() as u32,
                        &mut bytes,
                        None,
                        None,
                    ),
                    "update AcceptEx lookup",
                )?;
                let accept = accept.ok_or("update AcceptEx unavailable")?;
                self.resources.update_accept = Some(accept::AcceptSlot::new(Some(accept)));
                self.resources.update_accept.as_mut().unwrap().serial = 1 << 62;
                self.post_bare_accept_for(2)
            })()
        };
        if result.is_err() {
            // Keep debt on failed post/close; replacement is forbidden until
            // its actual completion and checked socket return are observed.
            let cancel = self.cancel_update_accept();
            let close = close(&mut self.resources.update_listener, true);
            if cancel.is_err() || close.is_err() {
                OWNER_FAULTED.store(true, Ordering::Release);
                return Err("update startup cleanup retained original owner".into());
            }
            if self
                .resources
                .update_accept
                .as_ref()
                .is_none_or(accept::AcceptSlot::released)
            {
                self.resources.update_accept = None;
                self.update_configuration = None;
            }
        }
        result
    }
    pub(super) fn cancel_update_accept(&mut self) -> Result<(), String> {
        self.cancel_bare_accept_for(2, false)
    }
    pub(super) fn configure_update(
        &mut self,
        configuration: Option<&HttpsConfiguration>,
    ) -> Result<(), ConfiguredTlsError> {
        if !self.control_mode || self.stopping {
            return Err(ConfiguredTlsError::InvalidConfiguration);
        }
        if self
            .pending_update
            .as_ref()
            .is_some_and(|desired| desired.as_ref() == configuration)
            || (self.pending_update.is_none()
                && self.update_error.is_none()
                && self.update_configuration.as_ref() == configuration)
        {
            return Ok(());
        }
        if let Some(configuration) = configuration {
            self.validate_update(configuration)?;
        }
        self.pending_update = Some(configuration.cloned());
        self.update_error = None;
        self.cancel_update_accept()
            .map_err(|_| ConfiguredTlsError::Cleanup)?;
        close(&mut self.resources.update_listener, true)
            .map_err(|_| ConfiguredTlsError::Cleanup)?;
        for index in 0..CONNECTION_SLOTS {
            if self.resources.slots[index].listener_index == 2
                && self.resources.slots[index].phase != Phase::Ready
            {
                self.close_slot(index, true)
                    .map_err(|_| ConfiguredTlsError::Cleanup)?;
            }
        }
        self.apply_update_change()
            .map_err(|_| ConfiguredTlsError::Native)
    }
    pub(super) fn apply_update_change(&mut self) -> Result<(), String> {
        if self.pending_update.is_none() || self.stopping {
            return Ok(());
        }
        if !self
            .resources
            .update_accept
            .as_ref()
            .is_none_or(accept::AcceptSlot::released)
            || self
                .resources
                .slots
                .iter()
                .any(|s| s.listener_index == 2 && s.phase != Phase::Ready)
        {
            return Ok(());
        }
        self.resources.update_accept = None;
        self.update_configuration = None;
        let worker = self.resources.worker.as_ref().ok_or("missing TLS worker")?;
        if !self.update_credential_pending {
            let target = self
                .pending_update
                .as_ref()
                .and_then(Option::as_ref)
                .cloned();
            let configuration = target
                .as_ref()
                .map(|c| super::super::CredentialConfiguration {
                    thumbprint: CertificateThumbprint::parse(&c.certificate.to_hex())
                        .expect("validated certificate identity"),
                    host: c.host.as_str().to_owned(),
                });
            worker
                .configure_update(configuration)
                .map_err(|e| format!("update credential mailbox: {e:?}"))?;
            self.update_credential_target = target;
            self.update_credential_pending = true;
            return Ok(());
        }
        let Some(result) = worker.poll_update_configured() else {
            return Ok(());
        };
        self.update_credential_pending = false;
        if let Err(error) = result {
            self.update_credential_target = None;
            self.pending_update = None;
            let owner_fault = matches!(
                error,
                super::super::TlsError::Cleanup(_)
                    | super::super::TlsError::ThreadPanic
                    | super::super::TlsError::Faulted
                    | super::super::TlsError::UnexpectedCompletion
            );
            self.update_error = Some(if owner_fault {
                ConfiguredTlsError::Cleanup
            } else {
                ConfiguredTlsError::Certificate
            });
            if owner_fault {
                OWNER_FAULTED.store(true, Ordering::Release);
            }
            return Err(format!("update credential readiness: {error:?}"));
        }
        // A later negative/configuration request can supersede a slow provider
        // call. Never bind it with a credential prepared for the old target.
        if self.pending_update.as_ref().and_then(Option::as_ref)
            != self.update_credential_target.as_ref()
        {
            self.update_credential_target = None;
            return Ok(());
        }
        self.pending_update = None;
        if let Some(configuration) = self.update_credential_target.take() {
            if let Err(error) = self.start_update(configuration) {
                self.update_error = Some(ConfiguredTlsError::Native);
                return Err(error);
            }
        }
        Ok(())
    }
}
