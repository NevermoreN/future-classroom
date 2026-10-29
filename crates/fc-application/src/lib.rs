#![forbid(unsafe_code)]
pub mod identity;
use fc_domain::{
    auth::ActorContext,
    content::{ContentCommand, ContentRecord},
};
use fc_ports::{Clock, ContentRepository, RepositoryError};

/// Admission validates quickly; the transaction adapter repeats every mutable check.
pub fn execute_content<R: ContentRepository>(
    repository: &mut R,
    clock: &dyn Clock,
    actor: &ActorContext,
    command: ContentCommand,
) -> Result<ContentRecord, RepositoryError> {
    let authority = repository.authority(actor)?;
    command
        .validate(&authority, actor, clock.now())
        .map_err(RepositoryError::Domain)?;
    repository.execute(command, actor, clock)
}

#[cfg(test)]
mod tests;
