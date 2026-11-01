//! Sole-writer installation-local HTTPS configuration. No certificate/native
//! query or positive Web admission effect is performed by this repository.
use super::*;
use fc_domain::https_settings::*;
use fc_ports::{
    HttpsSettingsRepository, HttpsSettingsRepositoryError, HttpsSettingsRequest, HttpsSettingsState,
};

pub(crate) const DOCUMENT: &str = "business/security/https/v1";
fn error(error: StorageError) -> HttpsSettingsRepositoryError {
    match error {
        StorageError::HttpsSettings(e) => HttpsSettingsRepositoryError::Domain(e),
        StorageError::CommandIdReused => HttpsSettingsRepositoryError::IdempotencyConflict,
        StorageError::DocumentTooLarge => HttpsSettingsRepositoryError::DocumentTooLarge,
        StorageError::PublicationFailed { .. }
        | StorageError::Indeterminate
        | StorageError::WorkerPanicked => HttpsSettingsRepositoryError::Faulted,
        _ => HttpsSettingsRepositoryError::Unavailable,
    }
}
fn authorize(
    identity: &IdentityState,
    actor: &ActorContext,
    clock: &dyn Clock,
    session: &SessionGuard,
) -> Result<(), StorageError> {
    session(identity, actor)?;
    if actor.origin != TrustedOrigin::Local {
        return Err(StorageError::HttpsSettings(HttpsSettingsError::Identity(
            IdentityError::Permission(PermissionError::LocalOnly),
        )));
    }
    identity
        .authorize_management(actor, clock.now())
        .map_err(|e| StorageError::HttpsSettings(HttpsSettingsError::Identity(e)))
}
fn configuration(value: &Option<HttpsConfiguration>) -> Value {
    value.as_ref().map_or(Value::Null, |c| {
        json!({"host":c.host.as_str(),"port":c.port,
        "certificate_store":"CURRENT_USER_MY","thumbprint_sha1":c.certificate.to_hex(),
        "source_networks":c.source_networks.as_ref().map(|p| p.networks().iter().map(|n| n.as_str()).collect::<Vec<_>>())})
    })
}
fn read_configuration(
    value: &Value,
    format: u64,
) -> Result<Option<HttpsConfiguration>, StorageError> {
    if value.is_null() {
        return Ok(None);
    }
    if format == 1 {
        codec::object_fields(
            value,
            &["host", "port", "certificate_store", "thumbprint_sha1"],
        )?;
    } else {
        codec::object_fields(
            value,
            &[
                "host",
                "port",
                "certificate_store",
                "thumbprint_sha1",
                "source_networks",
            ],
        )?;
    }
    if codec::text(value, "certificate_store")? != "CURRENT_USER_MY" {
        return Err(StorageError::Database);
    }
    let name = codec::text(value, "host")?;
    let host = HttpsHost::parse(&name).map_err(|_| StorageError::Database)?;
    let thumbprint = codec::text(value, "thumbprint_sha1")?;
    let certificate =
        HttpsCertificateRef::parse(&thumbprint).map_err(|_| StorageError::Database)?;
    if host.as_str() != name || certificate.to_hex() != thumbprint {
        return Err(StorageError::Database);
    }
    let mut result = HttpsConfiguration::new(
        host,
        u16::try_from(codec::number(value, "port")?).map_err(|_| StorageError::Database)?,
        certificate,
    )
    .map_err(|_| StorageError::Database)?;
    if format == 2 && !value["source_networks"].is_null() {
        let rows = value["source_networks"]
            .as_array()
            .ok_or(StorageError::Database)?;
        if rows.is_empty() || rows.len() > 16 {
            return Err(StorageError::Database);
        }
        let mut networks = Vec::with_capacity(rows.len());
        for row in rows {
            let text = row.as_str().ok_or(StorageError::Database)?;
            let network = HttpsSourceNetwork::parse(text).map_err(|_| StorageError::Database)?;
            if network.as_str() != text {
                return Err(StorageError::Database);
            }
            networks.push(network);
        }
        result.source_networks =
            Some(HttpsSourceNetworks::new(networks).map_err(|_| StorageError::Database)?);
    }
    Ok(Some(result))
}
fn settings(value: &HttpsSettings) -> Value {
    json!({"format":2,"revision":value.revision.get(),"configuration":configuration(&value.configuration)})
}
fn read_settings(value: &Value) -> Result<HttpsSettings, StorageError> {
    codec::object_fields(value, &["format", "revision", "configuration"])?;
    let format = codec::number(value, "format")?;
    if !matches!(format, 1 | 2) {
        return Err(StorageError::Database);
    }
    let settings = HttpsSettings {
        revision: Revision::new(codec::number(value, "revision")?),
        configuration: read_configuration(&value["configuration"], format)?,
    };
    settings.validate().map_err(|_| StorageError::Database)?;
    Ok(settings)
}
pub(super) fn load(
    d: &dyn TransactionDocuments,
    identity: &IdentityState,
) -> Result<HttpsSettings, StorageError> {
    let Some(row) = d.get(DOCUMENT)? else {
        return Ok(HttpsSettings::default());
    };
    let s = read_settings(&codec::value(&row.value)?)?;
    if identity.installation.is_none() || s.revision.get() == 0 || s.revision > d.revision()? {
        return Err(StorageError::Database);
    }
    Ok(s)
}
fn state(s: &HttpsSettingsState) -> Value {
    json!({"format":1,"identity_revision":s.identity_revision.get(),"settings":settings(&s.settings)})
}
fn read_state(value: &Value) -> Result<HttpsSettingsState, StorageError> {
    codec::object_fields(value, &["format", "identity_revision", "settings"])?;
    if codec::number(value, "format")? != 1 {
        return Err(StorageError::Database);
    }
    Ok(HttpsSettingsState {
        identity_revision: Revision::new(codec::number(value, "identity_revision")?),
        settings: read_settings(&value["settings"])?,
    })
}
fn check_permit(guard: &HttpsConfigurationGuard) -> Result<(), StorageError> {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| guard())).unwrap_or(false) {
        Ok(())
    } else {
        Err(StorageError::HttpsSettings(
            HttpsSettingsError::ConfigurationUnavailable,
        ))
    }
}
fn fingerprint(
    request: &HttpsSettingsRequest,
    actor: &ActorContext,
) -> Result<Vec<u8>, StorageError> {
    if let Some(configuration) = &request.configuration {
        configuration
            .validate()
            .map_err(StorageError::HttpsSettings)?;
    }
    let mut hash = Sha256::new();
    hash.update(b"future-classroom/https-settings/v1\0");
    // Lease expiry is freshly authorized on every attempt, not command
    // payload. Stable actor scope permits an authorized new lease to query
    // the same committed command without inventing a second command id.
    hash.update(codec::bytes(&json!({"principal":actor.principal_id.get(),
            "school":actor.school_id.get(),"auth_epoch":actor.auth_epoch.get(),
            "origin":match actor.origin {TrustedOrigin::Local=>"LOCAL",TrustedOrigin::Web=>"WEB"}}))?);
    hash.update(request.expected_revision.get().to_be_bytes());
    hash.update(request.expected_identity_revision.get().to_be_bytes());
    let mut canonical = configuration(&request.configuration);
    // Preserve exact old command fingerprints when no policy was supplied.
    // A configured policy is new command payload and must match in full.
    if request
        .configuration
        .as_ref()
        .is_some_and(|c| c.source_networks.is_none())
    {
        canonical
            .as_object_mut()
            .ok_or(StorageError::Database)?
            .remove("source_networks");
    }
    hash.update(codec::bytes(&canonical)?);
    Ok(hash.finalize().to_vec())
}
fn original_outcome(
    d: &dyn TransactionDocuments,
    request: &HttpsSettingsRequest,
    actor: &ActorContext,
) -> Result<Option<Vec<u8>>, StorageError> {
    let fingerprint = fingerprint(request, actor)?;
    let receipt_key = format!(
        "business/receipt/https-settings/{}",
        request.command_id.get()
    );
    let result_key = format!(
        "business/result/https-settings/{}",
        request.command_id.get()
    );
    if let Some(row) = d.get(&receipt_key)? {
        let receipt = codec::value(&row.value)?;
        codec::object_fields(&receipt, &["fingerprint", "result_digest"])?;
        let old: [u8; 32] = serde_json::from_value(receipt["fingerprint"].clone())
            .map_err(|_| StorageError::Database)?;
        if old.as_slice() != fingerprint {
            return Err(StorageError::CommandIdReused);
        }
        let result = d.get(&result_key)?.ok_or(StorageError::Database)?;
        let digest: [u8; 32] = serde_json::from_value(receipt["result_digest"].clone())
            .map_err(|_| StorageError::Database)?;
        if digest.as_slice() != Sha256::digest(&result.value).as_slice() {
            return Err(StorageError::Database);
        }
        let restored = read_state(&codec::value(&result.value)?)?;
        if restored.identity_revision != request.expected_identity_revision
            || restored.settings.revision
                != request
                    .expected_revision
                    .next()
                    .map_err(|_| StorageError::Database)?
        {
            return Err(StorageError::Database);
        }
        return Ok(Some(result.value));
    }
    Ok(None)
}
impl<S: Send + 'static> HttpsSettingsRepository for BusinessRepository<S> {
    fn https_settings(
        &self,
        actor: &ActorContext,
    ) -> Result<HttpsSettingsState, HttpsSettingsRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(ReadSettings {
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        read_state(&codec::value(&bytes).map_err(error)?).map_err(error)
    }
    fn query_https_settings_outcome(
        &self,
        request: HttpsSettingsRequest,
        actor: &ActorContext,
    ) -> Result<Option<HttpsSettingsState>, HttpsSettingsRepositoryError> {
        let bytes = self
            .handle
            .try_transaction(QueryOutcome {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        let value = codec::value(&bytes).map_err(error)?;
        if value.is_null() {
            Ok(None)
        } else {
            read_state(&value).map(Some).map_err(error)
        }
    }
    fn execute_https_settings(
        &mut self,
        request: HttpsSettingsRequest,
        actor: &ActorContext,
    ) -> Result<HttpsSettingsState, HttpsSettingsRepositoryError> {
        let guard =
            self.https_configuration_guard
                .clone()
                .ok_or(HttpsSettingsRepositoryError::Domain(
                    HttpsSettingsError::ConfigurationUnavailable,
                ))?;
        check_permit(&guard).map_err(error)?;
        let bytes = self
            .handle
            .try_transaction(SaveSettings {
                request,
                actor: actor.clone(),
                clock: self.clock.clone(),
                session: self.session.clone(),
                factory: self.factory.clone(),
                guard,
            })
            .map_err(error)?
            .wait()
            .map_err(error)?;
        read_state(&codec::value(&bytes).map_err(error)?).map_err(error)
    }
}
struct QueryOutcome {
    request: HttpsSettingsRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S: Send> TransactionTask<S> for QueryOutcome {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .request
                .configuration
                .as_ref()
                .map_or(0, HttpsConfiguration::allocation_bytes)
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        let outcome =
            original_outcome(d, &self.request, &self.actor)?.unwrap_or_else(|| b"null".to_vec());
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: Some(Box::new(move || {
                authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)
            })),
        })
    }
}
struct ReadSettings {
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
}
impl<S: Send> TransactionTask<S> for ReadSettings {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        let identity = load_identity(d)?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        let value = HttpsSettingsState {
            identity_revision: identity.revision,
            settings: load(d, &identity)?,
        };
        let outcome = codec::bytes(&state(&value))?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        Ok(TransactionDecision {
            outcome,
            commit: None,
            precommit: Some(Box::new(move || {
                authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)
            })),
        })
    }
}
struct SaveSettings<S> {
    request: HttpsSettingsRequest,
    actor: ActorContext,
    clock: Arc<dyn Clock + Send + Sync>,
    session: SessionGuard,
    factory: Factory<S>,
    guard: HttpsConfigurationGuard,
}
impl<S: Send + 'static> TransactionTask<S> for SaveSettings<S> {
    fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .request
                .configuration
                .as_ref()
                .map_or(0, HttpsConfiguration::allocation_bytes)
    }
    fn evaluate(
        self: Box<Self>,
        d: &dyn TransactionDocuments,
    ) -> Result<TransactionDecision<S>, StorageError> {
        check_permit(&self.guard)?;
        let identity = load_identity(d)?;
        authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
        if let Some(outcome) = original_outcome(d, &self.request, &self.actor)? {
            authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
            check_permit(&self.guard)?;
            return Ok(TransactionDecision {
                outcome,
                commit: None,
                precommit: Some(Box::new(move || {
                    check_permit(&self.guard)?;
                    authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)
                })),
            });
        }
        let fingerprint = fingerprint(&self.request, &self.actor)?;
        let receipt_key = format!(
            "business/receipt/https-settings/{}",
            self.request.command_id.get()
        );
        let result_key = format!(
            "business/result/https-settings/{}",
            self.request.command_id.get()
        );
        if identity.revision != self.request.expected_identity_revision {
            return Err(StorageError::HttpsSettings(
                HttpsSettingsError::IdentityRevisionConflict {
                    current: identity.revision,
                },
            ));
        }
        let current = load(d, &identity)?;
        let next = current
            .transition(
                self.request.configuration.clone(),
                self.request.expected_revision,
            )
            .map_err(StorageError::HttpsSettings)?;
        let outcome = codec::bytes(&state(&HttpsSettingsState {
            identity_revision: identity.revision,
            settings: next.clone(),
        }))?;
        let receipt = codec::bytes(
            &json!({"fingerprint":fingerprint,"result_digest":Sha256::digest(&outcome).to_vec()}),
        )?;
        let snapshot = (self.factory)(
            &identity,
            d.revision()?
                .next()
                .map_err(|_| StorageError::InvalidCommand)?,
        )?;
        Ok(TransactionDecision {
            outcome: outcome.clone(),
            commit: Some((
                vec![
                    Mutation::Put {
                        key: DOCUMENT.into(),
                        value: codec::bytes(&settings(&next))?,
                    },
                    Mutation::Put {
                        key: receipt_key,
                        value: receipt,
                    },
                    Mutation::Put {
                        key: result_key,
                        value: outcome,
                    },
                ],
                snapshot,
            )),
            precommit: Some(Box::new(move || {
                check_permit(&self.guard)?;
                authorize(&identity, &self.actor, self.clock.as_ref(), &self.session)?;
                current
                    .transition(self.request.configuration, self.request.expected_revision)
                    .map(|_| ())
                    .map_err(StorageError::HttpsSettings)
            })),
        })
    }
}
#[cfg(test)]
mod tests;
