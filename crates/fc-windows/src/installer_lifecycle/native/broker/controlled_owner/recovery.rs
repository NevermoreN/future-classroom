//! Explicit owned-fixture removal, separately authorized from ordinary repair.
//! The preserved old negative record is never settled by a new transaction.
//! This default-off artifact consumes build-bound evidence and actual native
//! observations, never user-supplied paths/hashes/results or stale PID lookups.
use super::*;
mod scope;
use scope::{hex, CompileOwnedFixtureScope};

const AUDIT: &str = "explicit-removal-audit.v1";
const MAX_AUDIT: usize = 16 * 1024;

struct ExplicitRemovalAuthorization(CompileOwnedFixtureScope);
impl ExplicitRemovalAuthorization {
    fn compiled(session: &Session) -> Result<Self> {
        let scope = CompileOwnedFixtureScope::compiled()?;
        if scope.sid != session.resources.sid {
            return Err(LifecycleError::Invalid);
        }
        scope.matches_target(&session.manifest)?;
        session.sealed_package_sha.ok_or(LifecycleError::Invalid)?;
        Ok(Self(scope))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Captured,
    Watching(u8),
    Armed(u8),
    Completed(u8),
    FailedKnown(u8),
    RemovalAuthorized,
    Removed,
}
#[derive(Clone, Copy)]
struct PhaseTerminal {
    guardian: GuardianIdentity,
    thread: u32,
    transaction: u32,
    install: u32,
    end: u32,
    outcome: SettledOutcome,
}
// Only phase2 native Absent reconciliation followed by checked source-control
// return constructs this capability. It is not a caller-provided result flag.
struct RecoveryAbsentProof;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OldDisposition {
    UnknownRemovedUnderSeparateAuthority,
}
struct ExplicitRemovalResult {
    old: OldDisposition,
}
struct RecoveryRemovalOwner {
    resources: Resources,
    authorization: ExplicitRemovalAuthorization,
    deadline: Instant,
    stage: Stage,
    phases: [Option<PhaseTerminal>; 2],
    audit: HANDLE,
}
impl RecoveryRemovalOwner {
    fn open(
        identity: &InstallIdentity,
        authorization: ExplicitRemovalAuthorization,
        session: &Session,
    ) -> Result<Self> {
        current(session.deadline)?;
        let mut resources = Resources::open_gate(identity, true, true)?;
        let scope = &authorization.0;
        if resources.sid != scope.sid {
            return Err(LifecycleError::Invalid);
        }
        let root = resources
            .handles
            .iter()
            .rev()
            .find(|h| h.role == Role::Parent)
            .ok_or(LifecycleError::Invalid)?
            .handle;
        scope.root.matches(root, true)?;
        scope.gate.matches(resources.handle(Role::Gate)?, false)?;
        let pending = resources.open_file(
            &resources.root.join("pending.v1"),
            GENERIC_READ.0 | READ_CONTROL.0 | DELETE.0,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            false,
            false,
        )?;
        resources.add(pending, Role::Pending);
        verify_acl(pending, &scope.sid, true)?;
        scope.pending_id.matches(pending, false)?;
        resources.pending_identity = Some((scope.pending_id.volume, scope.pending_id.index));
        resources.pending = scope.pending.clone();
        resources.verify_pending()?;
        let audit_path = session
            .image
            .parent()
            .ok_or(LifecycleError::Invalid)?
            .join(AUDIT);
        let audit = resources.open_file(
            &audit_path,
            GENERIC_READ.0 | GENERIC_WRITE.0 | READ_CONTROL.0,
            FILE_SHARE_READ,
            CREATE_NEW,
            false,
            true,
        )?;
        resources.add(audit, Role::Control);
        verify_info(audit, false)?;
        verify_acl(audit, &scope.sid, true)?;
        let mut this = Self {
            resources,
            authorization,
            deadline: session.deadline,
            stage: Stage::Captured,
            phases: [None, None],
            audit,
        };
        this.write_audit()?;
        Ok(this)
    }
    fn current(&self) -> Result<()> {
        current(self.deadline)?;
        self.resources.check_live()?;
        self.resources.verify_pending()?;
        self.authorization
            .0
            .pending_id
            .matches(self.resources.handle(Role::Pending)?, false)
    }
    fn write_audit(&mut self) -> Result<()> {
        self.current()?;
        let scope = &self.authorization.0;
        let mut text=format!("FC-EXPLICIT-REMOVAL-AUDIT/v1\noldDisposition=UNKNOWN\nsourcePins=HELD_UNTIL_FINAL_AUTHORIZATION\nsameBoot=DIAGNOSTIC_NOT_ASSERTED\nruntimeReport={}\npackageReport={}\npendingSnapshot={}\noldImage={}\noldB={},{};wait=0;exit=1603;checkedClose=1\noldG={},{};wait=0;exit=1603;checkedClose=1\noldPending={}, {}, {}\noldPendingHex={}\nstage={:?}\n",hex(&scope.runtime_report),hex(&scope.package_report),hex(&scope.pending_report),hex(&scope.old_image),scope.old_b.pid,scope.old_b.creation,scope.old_g.pid,scope.old_g.creation,scope.pending_id.volume,scope.pending_id.index,scope.pending_id.creation,hex(&scope.pending),self.stage);
        for (index, phase) in self.phases.iter().enumerate() {
            if let Some(p) = phase {
                text.push_str(&format!("newPhase{}=pid:{};creation:{};thread:{};begin:0;transaction:{};install:{};end:{};outcome:{:?};guardReturned:0;phaseControls:0;originalThreadExit:0;originalProcessExit:0\n",index+1,p.guardian.process_id,p.guardian.creation_filetime,p.thread,p.transaction,p.install,p.end,p.outcome));
            }
        }
        if text.len() > MAX_AUDIT {
            return Err(LifecycleError::Invalid);
        }
        unsafe {
            SetFilePointerEx(self.audit, 0, None, FILE_BEGIN)?;
        }
        let mut written = 0;
        unsafe {
            WriteFile(self.audit, Some(text.as_bytes()), Some(&mut written), None)?;
        }
        if written as usize != text.len() {
            return Err(LifecycleError::Unsettled);
        }
        unsafe {
            SetEndOfFile(self.audit)?;
            FlushFileBuffers(self.audit)?;
        }
        let mut actual = vec![0; text.len()];
        self.resources.read_at(self.audit, 0, &mut actual)?;
        if actual != text.as_bytes() {
            return Err(LifecycleError::Unsettled);
        }
        self.current()
    }
    fn launch(&mut self, session: &mut Session, index: u8) -> Result<(Launched, HANDLE)> {
        self.current()?;
        let expected = if index == 0 {
            Stage::Captured
        } else {
            Stage::Completed(0)
        };
        if index > 1 || self.stage != expected {
            return Err(LifecycleError::Invalid);
        }
        self.stage = Stage::Watching(index); // sticky before native launch
        if session.resources.handles.len() + 2 > HANDLE_LIMIT {
            return Err(LifecycleError::Invalid);
        }
        let application = HSTRING::from(session.image.as_os_str());
        let mut command: Vec<u16> = format!("\"{}\" owned-guardian", session.image.display())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut child = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                &application,
                Some(windows::core::PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_NO_WINDOW,
                None,
                None,
                &startup,
                &mut child,
            )?;
        }
        session.resources.add(child.hProcess, Role::Control);
        session.resources.add(child.hThread, Role::Control);
        let watch_start = self.resources.handles.len();
        let guardian = self.resources.watch_guardian_handle(child.hProcess)?;
        // Only this successful watch's temporary TOKEN_QUERY capabilities are
        // returned here. A watch/close failure retains the same owner and gate.
        for index in (watch_start..self.resources.handles.len()).rev() {
            if self.resources.handles[index].role == Role::Token {
                self.resources.close(index)?;
            }
        }
        if unsafe { GetThreadId(child.hThread) } != child.dwThreadId {
            return Err(LifecycleError::Invalid);
        }
        self.resources.watch_guardian_thread(child.hThread)?;
        self.resources.guardian_running()?;
        session.resources.close_handle(child.hThread)?;
        let (_, image) = session.authenticate_image(child.hProcess)?;
        self.current()?;
        Ok((
            Launched {
                process: child.hProcess,
                guardian,
                thread_id: child.dwThreadId,
            },
            image,
        ))
    }
    fn begin(&mut self, index: u8, began: Message) -> Result<()> {
        self.current()?;
        if self.stage != Stage::Watching(index)
            || began.kind != BEGUN
            || began.code != 0
            || began.transaction == 0
        {
            return Err(LifecycleError::Unsettled);
        }
        self.resources.guardian_running()?;
        self.stage = Stage::Armed(index); // new Begin only, no new/old Pending write
        Ok(())
    }
    fn complete(&mut self, index: u8, terminal: PhaseTerminal) -> Result<()> {
        self.current()?;
        if self.stage != Stage::Armed(index)
            || index > 1
            || terminal.transaction == 0
            || terminal.end != 0
        {
            return Err(LifecycleError::Unsettled);
        }
        self.resources.guardian_returned()?;
        // All original G1 handles return before G2 can exist. Its identity is
        // retained only as immutable actual terminal metadata, never recaptured.
        let thread = self.resources.handle(Role::GuardianThread)?;
        self.resources.close_handle(thread)?;
        let process = self.resources.handle(Role::Guardian)?;
        self.resources.close_handle(process)?;
        self.phases[usize::from(index)] = Some(terminal);
        let succeeded = terminal.install == 0 && terminal.outcome == SettledOutcome::Commit;
        self.stage = if succeeded {
            Stage::Completed(index)
        } else {
            Stage::FailedKnown(index)
        };
        self.write_audit()?;
        if succeeded {
            Ok(())
        } else {
            Err(LifecycleError::Unsettled)
        }
    }
    fn finish_removed(&mut self, _proof: RecoveryAbsentProof) -> Result<ExplicitRemovalResult> {
        self.current()?;
        if self.stage != Stage::Completed(1) || self.phases.iter().any(Option::is_none) {
            return Err(LifecycleError::Unsettled);
        }
        // The proof also requires checked source-control return. This audit is
        // authorization to dispose, not a claim that deletion already happened.
        // A final result exists only after the native delete/close/gate return.
        self.stage = Stage::RemovalAuthorized;
        self.write_audit()?;
        for index in (0..self.resources.handles.len()).rev() {
            if !matches!(
                self.resources.handles[index].role,
                Role::Gate | Role::Pending
            ) {
                self.resources.close(index)?;
            }
        }
        self.current()?;
        let handle = self.resources.handle(Role::Pending)?;
        let deletion = FILE_DISPOSITION_INFO { DeleteFile: true };
        unsafe {
            SetFileInformationByHandle(
                handle,
                FileDispositionInfo,
                (&raw const deletion).cast(),
                std::mem::size_of_val(&deletion) as u32,
            )?;
        }
        self.resources.close_handle(handle)?;
        self.resources.pending_identity = None;
        self.resources.absent_pending()?;
        self.resources.release()?;
        self.stage = Stage::Removed;
        Ok(ExplicitRemovalResult {
            old: OldDisposition::UnknownRemovedUnderSeparateAuthority,
        })
    }
}
impl Drop for RecoveryRemovalOwner {
    fn drop(&mut self) {
        if self.stage != Stage::Removed {
            self.resources.retain_until_exit();
        }
    }
}

fn run_phase(
    session: &mut Session,
    owner: &mut RecoveryRemovalOwner,
    index: u8,
    operation: Operation,
) -> Result<()> {
    owner.current()?;
    if !matches!(
        (index, operation),
        (0, Operation::Repair) | (1, Operation::Uninstall)
    ) {
        return Err(LifecycleError::Invalid);
    }
    // This recovery candidate accepts the original complete healthy14, not an
    // unknown product/preimage. Present/registration/component proof is native.
    let mut present = effects::present(session)?;
    present.check()?;
    present.release()?;
    let admission = effects::admit(session)?;
    if admission.operation() != MaintenanceOperation::SameManifest {
        return Err(LifecycleError::Invalid);
    }
    effects::revalidate(session, &admission)?;
    let pipe = session.server()?;
    let (child, image) = owner.launch(session, index)?;
    session.accept(pipe)?;
    let peer = session.peer(pipe, true)?;
    if process_identity(peer)? != child.guardian {
        return Err(LifecycleError::Invalid);
    }
    let hello = receive(session, pipe)?;
    if hello.kind != HELLO || hello.thread != child.thread_id {
        return Err(LifecycleError::Invalid);
    }
    let left = session
        .deadline
        .saturating_duration_since(Instant::now())
        .as_millis();
    if left == 0 || left > 120_000 {
        return Err(LifecycleError::Expired);
    }
    let mut ready = Message::new(READY);
    ready.ticks = unsafe { GetTickCount64() }
        .checked_add(left as u64)
        .ok_or(LifecycleError::Invalid)?;
    send(session, pipe, ready)?;
    let began = receive(session, pipe)?;
    owner.begin(index, began)?;
    // Recheck old negative and actual preimage after Begin, before granting Run.
    effects::revalidate(session, &admission)?;
    owner.current()?;
    let mut run = Message::new(RUN);
    run.transaction = began.transaction;
    run.outcome = operation.tag();
    send(session, pipe, run)?;
    let mut guard = GuardAssociation::new(began.transaction);
    let installed = loop {
        let frame = receive_frame(session.deadline, pipe)?;
        if &frame[..4] == b"FCGB" {
            owner.current()?;
            live(child.process)?;
            if let Some(ack) = guard.observe(Bridge::parse(&frame)?)? {
                send_frame(session.deadline, pipe, &ack.encode())?;
            }
        } else {
            guard.require_returned()?;
            break Message::parse(frame)?;
        }
    };
    if installed.kind != INSTALLED || installed.transaction != began.transaction {
        return Err(LifecycleError::Invalid);
    }
    let decision = if installed.code != 0 {
        effects::failed_install(&admission)
    } else {
        match operation {
            Operation::Repair => effects::staged_present(session, &admission)?,
            Operation::Uninstall => effects::staged_absent(session, &admission)?,
            Operation::Install => return Err(LifecycleError::Invalid),
        }
    };
    let mut reply = Message::new(DECIDE);
    reply.transaction = began.transaction;
    reply.outcome = if decision.outcome() == SettledOutcome::Commit {
        1
    } else {
        2
    };
    send(session, pipe, reply)?;
    let ended = receive(session, pipe)?;
    if ended.kind != ENDED
        || ended.transaction != began.transaction
        || ended.outcome != reply.outcome
        || ended.code != 0
    {
        return Err(LifecycleError::Unsettled);
    }
    session.release_connection(pipe, peer)?;
    session.resources.close_handle(pipe)?;
    returned_zero(child.process, session.deadline)?;
    loop {
        match owner.resources.guardian_returned() {
            Ok(()) => break,
            Err(LifecycleError::GuardianRunning) => pause(session.deadline)?,
            Err(e) => return Err(e),
        }
    }
    // Return the original launch image/process controls too, not just duplicates.
    session.resources.close_handle(image)?;
    session.resources.close_handle(child.process)?;
    let mut proof = effects::reconcile(session, &admission, decision)?;
    proof.check()?;
    proof.release()?;
    owner.complete(
        index,
        PhaseTerminal {
            guardian: child.guardian,
            thread: child.thread_id,
            transaction: began.transaction,
            install: installed.code,
            end: ended.code,
            outcome: decision.outcome(),
        },
    )
}

/// Only the feature-gated, fixed literal entry calls this. No runtime scope is
/// admitted; without a valid build-bound owned-fixture scope this cannot run.
pub(crate) fn run_compiled_fixture() -> Result<()> {
    // Fail inert before image/gate discovery when no fixture was compiled in.
    CompileOwnedFixtureScope::compiled()?;
    let deadline = Instant::now() + super::super::super::super::broker::BROKER_BUDGET;
    let mut session = Session::new(deadline)?;
    let authorization = ExplicitRemovalAuthorization::compiled(&session)?;
    package(&mut session)?;
    let identity = InstallIdentity::for_current_user()?;
    let mut owner = RecoveryRemovalOwner::open(&identity, authorization, &session)?;
    let original = process_identity(unsafe { GetCurrentProcess() })?;
    session.pipe_name = pipe_name(&session.pipe_name, original);
    let result = (|| {
        run_phase(&mut session, &mut owner, 0, Operation::Repair)?;
        run_phase(&mut session, &mut owner, 1, Operation::Uninstall)?;
        owner.current()?;
        // No source/image/peer control can fail a late close after the old
        // record is gone. Only the held gate/Pending remain after the final checked-control sweep.
        session.resources.release()?;
        let result = owner.finish_removed(RecoveryAbsentProof)?;
        match result.old {
            OldDisposition::UnknownRemovedUnderSeparateAuthority => Ok(()),
        }
    })();
    if result.is_err() {
        owner.resources.retain_until_exit();
        session.resources.retain_until_exit();
    }
    result
}
