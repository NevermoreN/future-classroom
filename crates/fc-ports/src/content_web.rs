//! Durable content command outcomes, distinct from the currently visible slot.
use crate::RepositoryError;
use fc_domain::{auth::ActorContext, content::ContentRecord, ids::CommandId};

/// A COMMITTED receipt proves the original command committed. A newer command
/// may already have replaced or cleared that slot; this result is not a view.
///
/// Only authenticated Web teacher/global channels may query. Implementations
/// first persist a new observed day through the trusted writer (metadata only).
/// They must read the receipt, current identity, current managed
/// scope and persistent date floor in the same writer transaction. Bind a fresh
/// exact session and check its authority again before returning. Only the
/// original `last_actor_id` may receive the result, including when the querying
/// account is a global administrator. Unknown and foreign command IDs both
/// return `None`; an original author's expired-day query returns `WrongDate`
/// and must not release its old text. Empty same-day results keep their
/// committed revision. Clocks belong to the trusted
/// repository adapter, not a caller-supplied HTTP date.
pub trait ContentOutcomeRepository {
    fn content_outcome(
        &self,
        command_id: CommandId,
        actor: &ActorContext,
    ) -> Result<Option<ContentRecord>, RepositoryError>;
}
