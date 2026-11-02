//! Bounded assembly of already prepared material proofs. Assembly performs no
//! I/O and creates no authorization; the sole Writer still rechecks the original
//! command pair, actor, scope, quotas and every item in one transaction.
use crate::{Clock, MaterialRepositoryError, PreparedMaterialAppend};
use fc_domain::{
    auth::ActorContext,
    ids::CommandId,
    materials::{
        MaterialAppend, MaterialError, MaterialRecord, MaterialScope, MAX_MATERIAL_BATCH_FILES,
    },
};
use std::collections::HashSet;

/// Includes vector and nested allocation capacities, not just logical lengths.
/// All twenty maximum-size names and class lists fit inside this metadata bound.
pub const MAX_MATERIAL_BATCH_ALLOCATION_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialBatchAppendRequest {
    pub command_id: CommandId,
    pub scope: MaterialScope,
    pub appends: Vec<MaterialAppend>,
}
impl MaterialBatchAppendRequest {
    pub fn allocation_bytes(&self) -> Option<usize> {
        let base = std::mem::size_of::<Self>().checked_add(
            self.appends
                .capacity()
                .checked_mul(std::mem::size_of::<MaterialAppend>())?,
        )?;
        self.appends.iter().try_fold(base, |bytes, append| {
            bytes.checked_add(append.allocation_bytes()?)
        })
    }
}

/// No Clone, Deserialize, public constructor or mutable accessor. Each member
/// can enter only through the original trusted single-file preparation port.
pub struct PreparedMaterialBatchAppend {
    request: MaterialBatchAppendRequest,
}
impl PreparedMaterialBatchAppend {
    pub fn request(&self) -> &MaterialBatchAppendRequest {
        &self.request
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.request.allocation_bytes()
    }
    pub fn into_request(self) -> MaterialBatchAppendRequest {
        self.request
    }
}

pub fn prepare_material_batch_append(
    prepared: Vec<PreparedMaterialAppend>,
) -> Result<PreparedMaterialBatchAppend, MaterialRepositoryError> {
    if prepared.is_empty() || prepared.len() > MAX_MATERIAL_BATCH_FILES {
        return Err(MaterialRepositoryError::Domain(MaterialError::Capacity));
    }
    let first = prepared[0].request();
    let command_id = first.command_id;
    let scope = first.scope;
    let mut ids = HashSet::with_capacity(prepared.len());
    let mut versions = HashSet::with_capacity(prepared.len());
    let mut dynamic = 0usize;
    for item in &prepared {
        let request = item.request();
        if request.command_id != command_id {
            return Err(MaterialRepositoryError::IdempotencyConflict);
        }
        if request.scope != scope {
            return Err(MaterialRepositoryError::Domain(MaterialError::InvalidScope));
        }
        request
            .append
            .validate()
            .map_err(MaterialRepositoryError::Domain)?;
        if !ids.insert(request.append.material_id)
            || !versions.insert(request.append.resource.version_id)
        {
            return Err(MaterialRepositoryError::Domain(
                MaterialError::AlreadyExists,
            ));
        }
        dynamic = dynamic
            .checked_add(
                item.allocation_bytes()
                    .ok_or(MaterialRepositoryError::DocumentTooLarge)?,
            )
            .ok_or(MaterialRepositoryError::DocumentTooLarge)?;
    }
    // During assembly both vector headers can coexist, but nested allocations
    // move from the old proofs into the batch; their bytes are counted once.
    let live = prepared
        .capacity()
        .checked_mul(std::mem::size_of::<PreparedMaterialAppend>())
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<MaterialBatchAppendRequest>()))
        .and_then(|bytes| {
            bytes.checked_add(
                prepared
                    .len()
                    .checked_mul(std::mem::size_of::<MaterialAppend>())?,
            )
        })
        .and_then(|bytes| bytes.checked_add(dynamic))
        .ok_or(MaterialRepositoryError::DocumentTooLarge)?;
    if live > MAX_MATERIAL_BATCH_ALLOCATION_BYTES {
        return Err(MaterialRepositoryError::DocumentTooLarge);
    }
    let mut appends = Vec::with_capacity(prepared.len());
    for item in prepared {
        appends.push(item.into_request().append);
    }
    Ok(PreparedMaterialBatchAppend {
        request: MaterialBatchAppendRequest {
            command_id,
            scope,
            appends,
        },
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialBatchCommandOutcome {
    pub command_id: CommandId,
    pub scope: MaterialScope,
    pub records: Vec<MaterialRecord>,
}

/// Separate from the single-file trait so existing adapters/mocks remain valid.
pub trait MaterialBatchRepository {
    fn append_material_batch(
        &mut self,
        prepared: PreparedMaterialBatchAppend,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<Vec<MaterialRecord>, MaterialRepositoryError>;
}
/// Historical outcomes retain the original authenticated submitter and full
/// command binding. A missing/failed read never authorizes a fresh mutation.
pub trait MaterialBatchOutcomeRepository {
    fn material_batch_outcome(
        &self,
        command_id: CommandId,
        actor: &ActorContext,
    ) -> Result<Option<MaterialBatchCommandOutcome>, MaterialRepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{prepare_material_append, MaterialAppendMetadata, MaterialPreparation};
    use fc_domain::{ids::*, materials::*};
    use std::{cell::Cell, rc::Rc};

    struct Preparation {
        calls: Rc<Cell<usize>>,
        version: u64,
    }
    impl MaterialPreparation for Preparation {
        type Input = ();
        fn prepare(&mut self, _: ()) -> Result<MaterialResource, MaterialRepositoryError> {
            self.calls.set(self.calls.get() + 1);
            Ok(MaterialResource {
                version_id: ResourceVersionId::new(self.version).unwrap(),
                sha256: [self.version as u8; 32],
                kind: MaterialKind::Txt,
                bytes: 100,
            })
        }
    }
    fn proof(
        command: u64,
        teacher: u64,
        material: u64,
        version: u64,
        calls: &Rc<Cell<usize>>,
    ) -> PreparedMaterialAppend {
        prepare_material_append(
            &mut Preparation {
                calls: calls.clone(),
                version,
            },
            (),
            MaterialAppendMetadata {
                command_id: CommandId::new(command).unwrap(),
                scope: MaterialScope {
                    teacher_id: PrincipalId::new(teacher).unwrap(),
                    subject_id: SubjectId::new(1).unwrap(),
                },
                material_id: MaterialId::new(material).unwrap(),
                display_name: "same-name.txt".into(),
                allowed_class_ids: vec![ClassId::new(1).unwrap()],
            },
        )
        .unwrap()
    }
    #[test]
    fn opaque_batch_moves_verified_proofs_in_order_without_repeating_preparation() {
        let calls = Rc::new(Cell::new(0));
        let originals = vec![proof(8, 2, 3, 9, &calls), proof(8, 2, 1, 4, &calls)];
        assert_eq!(calls.get(), 2);
        let batch = prepare_material_batch_append(originals).unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(batch.request().command_id, CommandId::new(8).unwrap());
        assert_eq!(
            batch
                .request()
                .appends
                .iter()
                .map(|r| r.material_id.get())
                .collect::<Vec<_>>(),
            vec![3, 1]
        );
        assert!(batch.allocation_bytes().unwrap() <= MAX_MATERIAL_BATCH_ALLOCATION_BYTES);
        let request = batch.into_request();
        assert_eq!(request.appends[0].resource.version_id.get(), 9);
        assert_eq!(request.appends[1].resource.sha256, [4; 32]);
    }
    #[test]
    fn mixed_command_scope_or_duplicate_id_and_version_never_forms_a_batch() {
        let calls = Rc::new(Cell::new(0));
        assert!(matches!(
            prepare_material_batch_append(vec![
                proof(8, 2, 1, 1, &calls),
                proof(9, 2, 2, 2, &calls)
            ]),
            Err(MaterialRepositoryError::IdempotencyConflict)
        ));
        assert!(matches!(
            prepare_material_batch_append(vec![
                proof(8, 2, 1, 1, &calls),
                proof(8, 3, 2, 2, &calls)
            ]),
            Err(MaterialRepositoryError::Domain(MaterialError::InvalidScope))
        ));
        for (id, version) in [(1, 2), (2, 1)] {
            assert!(matches!(
                prepare_material_batch_append(vec![
                    proof(8, 2, 1, 1, &calls),
                    proof(8, 2, id, version, &calls)
                ]),
                Err(MaterialRepositoryError::Domain(
                    MaterialError::AlreadyExists
                ))
            ));
        }
    }
    #[test]
    fn batch_count_and_actual_allocation_capacities_are_bounded() {
        let calls = Rc::new(Cell::new(0));
        assert!(matches!(
            prepare_material_batch_append(vec![]),
            Err(MaterialRepositoryError::Domain(MaterialError::Capacity))
        ));
        let max = (1..=20).map(|n| proof(8, 2, n, n, &calls)).collect();
        assert_eq!(
            prepare_material_batch_append(max)
                .unwrap()
                .request()
                .appends
                .len(),
            20
        );
        let largest = (1..=20)
            .map(|n| {
                prepare_material_append(
                    &mut Preparation {
                        calls: calls.clone(),
                        version: n,
                    },
                    (),
                    MaterialAppendMetadata {
                        command_id: CommandId::new(8).unwrap(),
                        scope: MaterialScope {
                            teacher_id: PrincipalId::new(2).unwrap(),
                            subject_id: SubjectId::new(1).unwrap(),
                        },
                        material_id: MaterialId::new(n).unwrap(),
                        display_name: "🧪".repeat(176) + ".txt",
                        allowed_class_ids: (1..=fc_domain::identity::MAX_CLASSES as u64)
                            .map(|id| ClassId::new(id).unwrap())
                            .collect(),
                    },
                )
                .unwrap()
            })
            .collect();
        assert_eq!(
            prepare_material_batch_append(largest)
                .unwrap()
                .request()
                .appends
                .len(),
            20
        );
        let excess = (1..=21).map(|n| proof(8, 2, n, n, &calls)).collect();
        assert!(matches!(
            prepare_material_batch_append(excess),
            Err(MaterialRepositoryError::Domain(MaterialError::Capacity))
        ));
        let mut oversized = String::with_capacity(MAX_MATERIAL_BATCH_ALLOCATION_BYTES + 1);
        oversized.push_str("x.txt");
        let prepared = prepare_material_append(
            &mut Preparation {
                calls: calls.clone(),
                version: 1,
            },
            (),
            MaterialAppendMetadata {
                command_id: CommandId::new(8).unwrap(),
                scope: MaterialScope {
                    teacher_id: PrincipalId::new(2).unwrap(),
                    subject_id: SubjectId::new(1).unwrap(),
                },
                material_id: MaterialId::new(1).unwrap(),
                display_name: oversized,
                allowed_class_ids: vec![ClassId::new(1).unwrap()],
            },
        )
        .unwrap();
        assert!(matches!(
            prepare_material_batch_append(vec![prepared]),
            Err(MaterialRepositoryError::DocumentTooLarge)
        ));
        let mut overallocated = Vec::with_capacity(MAX_MATERIAL_BATCH_ALLOCATION_BYTES);
        overallocated.push(proof(8, 2, 1, 1, &calls));
        assert!(matches!(
            prepare_material_batch_append(overallocated),
            Err(MaterialRepositoryError::DocumentTooLarge)
        ));
    }
}
