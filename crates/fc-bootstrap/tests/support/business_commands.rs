//! Test-only composition. Real Writer issuance and the real repository retain
//! every authorization/CAS/publication check. Labels are never production IDs.
use fc_domain::{
    auth::*, content::*, desktop::*, identity::*, ids::*, lesson_context::LessonContextSnapshot,
    schedule_document::*,
};
use fc_ports::{command_window::*, *};
use fc_storage::{
    business::{BusinessRepository, MaterialSceneGuard, SessionGuard},
    command_window::IssueCommand,
    StorageError, StorageHandle,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

static NEXT: AtomicU64 = AtomicU64::new(20_000_000);
#[derive(Default)]
pub struct Commands(Mutex<BTreeMap<(&'static str, String), IssuedCommand>>);
impl Commands {
    fn issue<S: Send + 'static>(
        &self,
        handle: &StorageHandle<S>,
        family: CommandFamily,
        label: u64,
        actor: Option<&ActorContext>,
    ) -> IssuedCommand {
        self.issue_named(handle, family, &label.to_string(), actor)
            .unwrap()
    }
    fn issue_named<S: Send + 'static>(
        &self,
        handle: &StorageHandle<S>,
        family: CommandFamily,
        label: &str,
        actor: Option<&ActorContext>,
    ) -> Result<IssuedCommand, StorageError> {
        let mut map = self.0.lock().unwrap();
        if let Some(original) = map.get(&(family.name(), label.to_owned())) {
            return Ok(*original);
        }
        if let Ok(raw) = label.parse::<u64>() {
            if let Some(original) = map
                .values()
                .find(|p| p.family == family && p.command_id.get() == raw)
            {
                return Ok(*original);
            }
        }
        let raw = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .unwrap();
        let mut entropy = [83; 24];
        entropy[16..].copy_from_slice(&raw.to_be_bytes());
        let issued = handle
            .try_issue_command(IssueCommand {
                family,
                owner: actor.map_or(CommandOwner::PublicDesktop, CommandOwner::authenticated),
                entropy,
            })?
            .wait()?;
        map.insert((family.name(), label.to_owned()), issued);
        Ok(issued)
    }
    fn original(&self, family: CommandFamily, id: CommandId) -> IssuedCommand {
        let map = self.0.lock().unwrap();
        *map.values()
            .find(|p| p.family == family && p.command_id == id)
            .expect("opaque fixture proof must be bound before preparation")
    }
}
type Factory<S> = Arc<dyn Fn(&IdentityState, Revision) -> Result<S, StorageError> + Send + Sync>;
pub struct Repository<S> {
    inner: BusinessRepository<S>,
    handle: StorageHandle<S>,
    commands: Arc<Commands>,
}
impl<S> Clone for Repository<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            handle: self.handle.clone(),
            commands: self.commands.clone(),
        }
    }
}
impl<S> std::ops::Deref for Repository<S> {
    type Target = BusinessRepository<S>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl<S> std::ops::DerefMut for Repository<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
impl<S: Send + 'static> Repository<S> {
    pub fn new(
        handle: StorageHandle<S>,
        clock: Arc<dyn Clock + Send + Sync>,
        factory: Factory<S>,
        commands: Arc<Commands>,
    ) -> Self {
        Self {
            inner: BusinessRepository::new(handle.clone(), clock, factory),
            handle,
            commands,
        }
    }
    pub fn with_session_guard(mut self, guard: SessionGuard) -> Self {
        self.inner = self.inner.with_session_guard(guard);
        self
    }
    pub fn with_material_scene_guard(mut self, guard: MaterialSceneGuard) -> Self {
        self.inner = self.inner.with_material_scene_guard(guard);
        self
    }
    pub fn issue(
        &self,
        family: CommandFamily,
        label: u64,
        actor: Option<&ActorContext>,
    ) -> IssuedCommand {
        self.commands.issue(&self.handle, family, label, actor)
    }
    fn original(&self, family: CommandFamily, id: CommandId) -> BusinessRepository<S> {
        self.inner
            .with_command_window(self.commands.original(family, id))
    }
    pub fn generic_commit(
        &self,
        label: &str,
        revision: Revision,
        mutations: Vec<fc_storage::Mutation>,
        snapshot: S,
    ) -> Result<fc_storage::Ticket<fc_storage::CommitReceipt>, StorageError> {
        let issued =
            self.commands
                .issue_named(&self.handle, CommandFamily::Generic, label, None)?;
        self.handle.with_command_window(issued).try_commit(
            fc_storage::CommitCommand::new(
                issued.command_id.get().to_string(),
                revision,
                mutations,
            )?,
            snapshot,
        )
    }
    /// Resolve only existing fixture history; never issues during inspection.
    pub fn stored_key(&self, key: &str) -> String {
        let prefixes = [
            ("business/receipt/content/", CommandFamily::Content),
            ("business/receipt/schedule/", CommandFamily::Schedule),
            ("business/result/schedule/", CommandFamily::Schedule),
            ("business/receipt/display/", CommandFamily::Display),
            ("business/result/display/", CommandFamily::Display),
            ("business/material/v1/receipts/", CommandFamily::Materials),
        ];
        for (prefix, family) in prefixes {
            if let Some(label) = key.strip_prefix(prefix) {
                if label.starts_with("v2/") {
                    return key.to_owned();
                }
                let raw = label.parse::<u64>().expect("fixture receipt label");
                let map = self.commands.0.lock().unwrap();
                let issued = map
                    .get(&(family.name(), raw.to_string()))
                    .expect("original fixture pair before inspection");
                let epoch: String = issued
                    .ticket
                    .epoch()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                return format!("{prefix}v2/{epoch}/{}", issued.ticket.serial());
            }
        }
        key.to_owned()
    }
    pub fn identity_request(
        &self,
        mut request: IdentityRequest,
        actor: Option<&ActorContext>,
    ) -> IdentityRequest {
        request.command_id = self
            .issue(CommandFamily::Identity, request.command_id.get(), actor)
            .command_id;
        request
    }
    pub fn execute_schedule(
        &mut self,
        mut request: ScheduleRequest,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<ScheduleDocument, ScheduleRepositoryError> {
        let issued = self.issue(
            CommandFamily::Schedule,
            request.command_id.get(),
            Some(actor),
        );
        request.command_id = issued.command_id;
        self.inner
            .with_command_window(issued)
            .execute_schedule(request, actor, clock)
    }
    pub fn execute_display(
        &mut self,
        mut request: DisplayRequest,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<DisplaySettings, DisplayRepositoryError> {
        let issued = self.issue(
            CommandFamily::Display,
            request.command_id.get(),
            Some(actor),
        );
        request.command_id = issued.command_id;
        self.inner
            .with_command_window(issued)
            .execute_display(request, actor, clock)
    }
    pub fn execute_lesson(
        &mut self,
        mut request: LessonRequest,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<LessonContextSnapshot, LessonRepositoryError> {
        let issued = self.issue(CommandFamily::Lesson, request.command_id.get(), Some(actor));
        request.command_id = issued.command_id;
        self.inner
            .with_command_window(issued)
            .execute_lesson(request, actor, clock)
    }
    pub fn append_material(
        &mut self,
        prepared: PreparedMaterialAppend,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<fc_domain::materials::MaterialRecord, MaterialRepositoryError> {
        self.original(CommandFamily::Materials, prepared.request().command_id)
            .append_material(prepared, actor, clock)
    }
    pub fn delete_material(
        &mut self,
        mut request: MaterialDeleteRequest,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<fc_domain::materials::MaterialRecord, MaterialRepositoryError> {
        let issued = self.issue(
            CommandFamily::Materials,
            request.command_id.get(),
            Some(actor),
        );
        request.command_id = issued.command_id;
        self.inner
            .with_command_window(issued)
            .delete_material(request, actor, clock)
    }
}
impl<S: Send + 'static> IdentityRepository for Repository<S> {
    fn state(&self) -> Result<IdentityState, IdentityRepositoryError> {
        self.inner.state()
    }
    fn execute_identity(
        &mut self,
        request: IdentityRequest,
        actor: Option<&ActorContext>,
        clock: &dyn Clock,
    ) -> Result<IdentityTransition, IdentityRepositoryError> {
        let request = self.identity_request(request, actor);
        self.original(CommandFamily::Identity, request.command_id)
            .execute_identity(request, actor, clock)
    }
}
impl<S: Send + 'static> CredentialIdentityRepository<Vec<u8>, Vec<u8>> for Repository<S> {
    fn execute_credentials(
        &mut self,
        change: PreparedCredentialChange<Vec<u8>, Vec<u8>>,
        actor: Option<&ActorContext>,
        clock: &dyn Clock,
    ) -> Result<IdentityTransition, IdentityRepositoryError> {
        self.original(CommandFamily::Identity, change.request().command_id)
            .execute_credentials(change, actor, clock)
    }
}
impl<S: Send + 'static> ContentRepository for Repository<S> {
    fn authority(&self, actor: &ActorContext) -> Result<AuthorityContext, RepositoryError> {
        self.inner.authority(actor)
    }
    fn execute(
        &mut self,
        mut request: ContentCommand,
        actor: &ActorContext,
        clock: &dyn Clock,
    ) -> Result<ContentRecord, RepositoryError> {
        let issued = self.issue(
            CommandFamily::Content,
            request.command_id.get(),
            Some(actor),
        );
        request.command_id = issued.command_id;
        self.inner
            .with_command_window(issued)
            .execute(request, actor, clock)
    }
}
