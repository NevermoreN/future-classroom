//! Durable material commands, separate from currently published folder contents.
use crate::MaterialRepositoryError;
use fc_domain::{
    auth::ActorContext,
    ids::CommandId,
    materials::{MaterialRecord, MaterialScope},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialCommandKind {
    Append,
    Delete,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialCommandOutcome {
    pub command_id: CommandId,
    pub kind: MaterialCommandKind,
    pub scope: MaterialScope,
    pub record: MaterialRecord,
}
/// Only an exact live Web channel of the original submitting principal can read
/// its receipt, with current folder authority checked again before return.
/// Global administrators cannot read another submitter's result. Unknown,
/// foreign and legacy commands without authenticated Web attribution return None.
/// A result is historical: deletion or a subsequent command does not rewrite it.
/// Unavailable/faulted reads prove neither commit nor rollback; callers retain
/// the original command ID and must not invent a fresh mutation to resolve it.
pub trait MaterialOutcomeRepository {
    fn material_outcome(
        &self,
        command_id: CommandId,
        actor: &ActorContext,
    ) -> Result<Option<MaterialCommandOutcome>, MaterialRepositoryError>;
}
