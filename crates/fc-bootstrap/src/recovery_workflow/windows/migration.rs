//! The existing physical slot also owns the source throughout mapping. UI
//! handles contain catalog facts only and request retirement on Drop.
use super::*;
use fc_domain::{
    identity::IdentityState,
    recovery::{MigrationCatalog, MigrationMapping},
};

#[derive(Clone)]
pub struct MigrationDetails {
    pub source_catalog: MigrationCatalog,
    pub source_installation: [u8; 32],
    pub source_revision: Revision,
    pub target_identity: IdentityState,
    pub target_installation: [u8; 32],
    pub target_global_revision: Revision,
}
impl MigrationDetails {
    fn allocation_bytes(&self) -> Option<usize> {
        fn vector<T>(v: &Vec<T>) -> Option<usize> {
            v.capacity().checked_mul(std::mem::size_of::<T>())
        }
        let source = &self.source_catalog;
        let target = &self.target_identity;
        let mut n = std::mem::size_of::<Self>();
        for size in [
            source.school_name.capacity(),
            vector(&source.classes)?,
            vector(&source.subjects)?,
            vector(&source.principals)?,
            vector(&source.default_teachers)?,
            vector(&target.accounts)?,
            vector(&target.classes)?,
            vector(&target.subjects)?,
            vector(&target.assignments)?,
            vector(&target.default_teachers)?,
        ] {
            n = n.checked_add(size)?;
        }
        if let Some(i) = &target.installation {
            n = n.checked_add(i.school_name.capacity())?;
        }
        for s in source
            .classes
            .iter()
            .map(|v| &v.name)
            .chain(source.subjects.iter().map(|v| &v.name))
            .chain(source.principals.iter().map(|v| &v.display_name))
            .chain(
                target
                    .accounts
                    .iter()
                    .flat_map(|v| [&v.login_name, &v.display_name]),
            )
            .chain(target.classes.iter().map(|v| &v.name))
            .chain(target.subjects.iter().map(|v| &v.name))
        {
            n = n.checked_add(s.capacity())?;
        }
        Some(n)
    }
}
pub struct MigrationBundle {
    workflow: RecoveryWorkflow,
    generation: u64,
    details: Box<MigrationDetails>,
    armed: bool,
}
impl MigrationBundle {
    pub fn details(&self) -> &MigrationDetails {
        &self.details
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>().checked_add(self.details.allocation_bytes()?)
    }
}
impl Drop for MigrationBundle {
    fn drop(&mut self) {
        if self.armed {
            self.workflow.slot.retire(self.generation);
        }
    }
}
fn summary(preview: &RestorePreview) -> Result<ReviewSummary> {
    let mut summary = ReviewSummary {
        school_id: preview.manifest().school_id,
        source_revision: preview.manifest().revision,
        current_revision: preview.current_revision(),
        document_count: preview.document_count(),
        material_count: 0,
        profile_media_count: 0,
        payload_bytes: 0,
        database_bytes: 0,
        material_bytes: 0,
        profile_media_bytes: 0,
    };
    for entry in &preview.manifest().entries {
        summary.payload_bytes = summary
            .payload_bytes
            .checked_add(entry.bytes())
            .ok_or(WorkflowError::Files(RecoveryFileError::InvalidPackage))?;
        match entry {
            BackupEntry::Database { bytes, .. } => summary.database_bytes = *bytes,
            BackupEntry::Material { bytes, .. } => {
                summary.material_count += 1;
                summary.material_bytes += bytes;
            }
            BackupEntry::ProfileMedia { bytes, .. } => {
                summary.profile_media_count += 1;
                summary.profile_media_bytes += bytes;
            }
        }
    }
    Ok(summary)
}
impl RecoveryWorkflow {
    /// Catalog inspection only; no mapped preview or destructive permission is
    /// issued until all explicit choices and production preflight succeed.
    pub fn inspect_migration(
        &self,
        selected: SelectedFile,
        access: OrdinaryBackupAccess,
        execution: ExecutionFence,
    ) -> Result<MigrationBundle> {
        let generation = self.slot.reserve().ok_or(WorkflowError::Busy)?;
        let mut reservation = Reservation {
            workflow: self.clone(),
            generation,
            active: true,
        };
        let slot = self.slot.clone();
        let fence: ExecutionFence = Arc::new(move || {
            slot.permits(generation)
                && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execution()))
                    .unwrap_or(false)
        });
        let files = RecoveryFiles::new(&self.paths)?;
        let prepared = files.preflight_package(selected, &*fence)?;
        let inspection = access.inspect_migration_source(
            prepared.ordinary_database().to_owned(),
            prepared.manifest().clone(),
            prepared.manifest_sha256(),
            fence.clone(),
        )?;
        if !fence() {
            return Err(WorkflowError::Cancelled);
        }
        let details = Box::new(MigrationDetails {
            source_catalog: inspection.source_catalog().clone(),
            source_installation: inspection.source_manifest().installation_id,
            source_revision: inspection.source_manifest().revision,
            target_identity: inspection.target_identity().clone(),
            target_installation: inspection.target_installation(),
            target_global_revision: inspection.source_receipt().current_revision(),
        });
        if !self.slot.publish(
            generation,
            Payload::Mapping {
                prepared,
                inspection: Box::new(inspection),
            },
        ) {
            return Err(WorkflowError::Cancelled);
        }
        reservation.active = false;
        Ok(MigrationBundle {
            workflow: self.clone(),
            generation,
            details,
            armed: true,
        })
    }
    /// Consumes the source notification. All large owners remain on this shared
    /// worker or in its slot; failure retires both workspaces before reuse.
    pub fn preflight_mapping(
        &self,
        mut bundle: MigrationBundle,
        mapping: MigrationMapping,
        access: OrdinaryBackupAccess,
        execution: ExecutionFence,
    ) -> Result<ReviewBundle> {
        if !self.slot.same(&bundle.workflow.slot) {
            return Err(WorkflowError::Busy);
        }
        let generation = bundle.generation;
        let payload = self
            .slot
            .take_ready(generation)
            .ok_or(WorkflowError::Cancelled)?;
        bundle.armed = false;
        let mut reservation = Reservation {
            workflow: self.clone(),
            generation,
            active: true,
        };
        let Payload::Mapping {
            prepared,
            inspection,
        } = payload
        else {
            return Err(WorkflowError::Busy);
        };
        let slot = self.slot.clone();
        let fence: ExecutionFence = Arc::new(move || {
            slot.permits(generation)
                && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execution()))
                    .unwrap_or(false)
        });
        let stage = prepared.prepare_mapping(
            inspection.source_receipt().clone(),
            mapping.clone(),
            &*fence,
        )?;
        let receipt = access.map_migration_database(
            &inspection,
            mapping,
            stage.mapped_database_destination(),
            fence.clone(),
        )?;
        let mapped = stage.finish(&receipt, &*fence)?;
        let preview = access.preflight_restore(
            mapped.ordinary_database().to_owned(),
            mapped.ordinary_database().with_file_name("preview.sqlite3"),
            mapped.manifest().clone(),
            mapped.manifest_sha256(),
            fence.clone(),
        )?;
        let summary = summary(&preview)?;
        if !fence() {
            return Err(WorkflowError::Cancelled);
        }
        if !self.slot.publish(
            generation,
            Payload::Preview {
                prepared: RestorePayload::Mapped(Box::new(mapped)),
                preview,
            },
        ) {
            return Err(WorkflowError::Cancelled);
        }
        reservation.active = false;
        Ok(ReviewBundle {
            workflow: self.clone(),
            generation,
            summary,
            armed: true,
        })
    }
}
