//! Original native Begin owner, separate from the legacy embedded Join route.
//! No public Setup entry or controlled-inner MSI guard is released here. The
//! fixed package seal is absent from existing helpers, which reject this route.
use super::*;
use windows::Win32::System::SystemInformation::GetTickCount64;

pub(crate) const PACKAGE_SEAL_MAGIC: &[u8; 16] = b"FC-SETUP-MSI\0v1\0";
const INNER_MSI: &str = "FutureClassroom.msi";
const HELLO: u8 = 1;
const READY: u8 = 2;
const BEGUN: u8 = 3;
const RUN: u8 = 4;
const INSTALLED: u8 = 5;
const DECIDE: u8 = 6;
const ENDED: u8 = 7;
const ABORT: u8 = 8;

pub(super) fn package_seal(bytes: [u8; 48]) -> Result<Option<[u8; 32]>> {
    if &bytes[..16] != PACKAGE_SEAL_MAGIC {
        return Ok(None);
    }
    let digest: [u8; 32] = bytes[16..]
        .try_into()
        .map_err(|_| LifecycleError::Invalid)?;
    if digest == [0; 32] {
        return Err(LifecycleError::Invalid);
    }
    Ok(Some(digest))
}

/// Closed trusted Setup UI choice; never a command-line MSI path or property.
#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Install,
    Repair,
    Uninstall,
}
/// Resource settlement and the requested installation's business result are
/// independent. A clean rollback can return this value with install1603.
pub(crate) struct SetupResult {
    pub(crate) install_return: u32,
    pub(crate) outcome: SettledOutcome,
}
impl SetupResult {
    pub(crate) fn installation_succeeded(&self) -> bool {
        self.install_return == 0 && self.outcome == SettledOutcome::Commit
    }
}
impl Operation {
    fn tag(self) -> u8 {
        match self {
            Self::Install => 1,
            Self::Repair => 2,
            Self::Uninstall => 3,
        }
    }
}

#[derive(Clone, Copy)]
struct Message {
    kind: u8,
    outcome: u8,
    thread: u32,
    transaction: u32,
    code: u32,
    ticks: u64,
}
impl Message {
    fn new(kind: u8) -> Self {
        Self {
            kind,
            outcome: 0,
            thread: 0,
            transaction: 0,
            code: 0,
            ticks: 0,
        }
    }
    fn bytes(self) -> [u8; FRAME] {
        let mut b = [0; FRAME];
        b[..4].copy_from_slice(b"FCO1");
        b[4] = self.kind;
        b[5] = self.outcome;
        b[8..12].copy_from_slice(&self.thread.to_le_bytes());
        b[12..16].copy_from_slice(&self.transaction.to_le_bytes());
        b[16..20].copy_from_slice(&self.code.to_le_bytes());
        b[24..32].copy_from_slice(&self.ticks.to_le_bytes());
        b
    }
    fn parse(b: [u8; FRAME]) -> Result<Self> {
        if &b[..4] != b"FCO1" || b[6..8] != [0, 0] || b[20..24] != [0; 4] {
            return Err(LifecycleError::Invalid);
        }
        let m = Self {
            kind: b[4],
            outcome: b[5],
            thread: u32::from_le_bytes(b[8..12].try_into().unwrap()),
            transaction: u32::from_le_bytes(b[12..16].try_into().unwrap()),
            code: u32::from_le_bytes(b[16..20].try_into().unwrap()),
            ticks: u64::from_le_bytes(b[24..32].try_into().unwrap()),
        };
        let valid = match m.kind {
            HELLO => {
                m.thread != 0 && m.transaction == 0 && m.outcome == 0 && m.code == 0 && m.ticks == 0
            }
            READY => {
                m.thread == 0 && m.transaction == 0 && m.outcome == 0 && m.code == 0 && m.ticks != 0
            }
            BEGUN => {
                m.thread == 0
                    && m.outcome == 0
                    && m.ticks == 0
                    && m.code != u32::MAX
                    && ((m.code == 0 && m.transaction != 0) || (m.code != 0 && m.transaction == 0))
            }
            RUN => {
                m.thread == 0
                    && m.transaction != 0
                    && (1..=2).contains(&m.outcome)
                    && m.code == 0
                    && m.ticks == 0
            }
            INSTALLED => {
                m.thread == 0
                    && m.transaction != 0
                    && m.outcome == 0
                    && m.code != u32::MAX
                    && m.ticks == 0
            }
            DECIDE => {
                m.thread == 0
                    && m.transaction != 0
                    && matches!(m.outcome, 1 | 2)
                    && m.code == 0
                    && m.ticks == 0
            }
            ENDED => m.thread == 0 && matches!(m.outcome, 0..=2) && m.ticks == 0,
            ABORT => m.thread == 0 && m.outcome == 0 && m.code == 0 && m.ticks == 0,
            _ => false,
        };
        if valid {
            Ok(m)
        } else {
            Err(LifecycleError::Invalid)
        }
    }
}
fn send(s: &Session, pipe: HANDLE, m: Message) -> Result<()> {
    current(s.deadline)?;
    let mut n = 0;
    unsafe {
        WriteFile(pipe, Some(&m.bytes()), Some(&mut n), None)?;
    }
    if n as usize != FRAME {
        return Err(LifecycleError::Unsettled);
    }
    current(s.deadline)
}
fn receive(s: &Session, pipe: HANDLE) -> Result<Message> {
    let mut b = [0; FRAME];
    let mut offset = 0;
    while offset < FRAME {
        current(s.deadline)?;
        let mut n = 0;
        match unsafe { ReadFile(pipe, Some(&mut b[offset..]), Some(&mut n), None) } {
            Ok(()) if n != 0 => offset += n as usize,
            Ok(()) => pause(s.deadline)?,
            Err(e) if e.code() == ERROR_NO_DATA.to_hresult() => pause(s.deadline)?,
            Err(e) => return Err(e.into()),
        }
    }
    Message::parse(b)
}
fn pipe_name(base: &HSTRING, parent: GuardianIdentity) -> HSTRING {
    HSTRING::from(format!(
        "{base}-OwnedBegin-{}-{}",
        parent.process_id, parent.creation_filetime
    ))
}
fn package(s: &mut Session) -> Result<PathBuf> {
    let expected = s.sealed_package_sha.ok_or(LifecycleError::Invalid)?;
    let path = s
        .image
        .parent()
        .ok_or(LifecycleError::Invalid)?
        .join(INNER_MSI);
    // Session has pinned every image ancestor. This fixed regular source pin
    // permits engine reads while refusing writes/delete/replacement before End.
    let file = s.resources.open_file(
        &path,
        GENERIC_READ.0,
        FILE_SHARE_READ,
        OPEN_EXISTING,
        false,
        false,
    )?;
    s.resources.add(file, Role::Control);
    verify_info(file, false)?;
    if s.resources.digest(file, 64 * 1024 * 1024, s.deadline)? != expected {
        return Err(LifecycleError::Invalid);
    }
    Ok(path)
}

/// Owns only this process's actual native transaction/event. There is no Join
/// and no extra MsiCloseHandle of the transaction identifier after End0.
struct ApiOwner {
    resources: Resources,
    transaction: MSIHANDLE,
    changed: Option<HANDLE>,
    begin: u32,
    ended: bool,
    thread: u32,
}
impl ApiOwner {
    fn begin() -> Result<Self> {
        let resources = Resources::empty()?;
        let mut this = Self {
            resources,
            transaction: MSIHANDLE(0),
            changed: None,
            begin: u32::MAX,
            ended: false,
            thread: unsafe { GetCurrentThreadId() },
        };
        // Configure this API caller before Begin; do not reopen logging/UI while
        // an outer transaction is active. No machine/service/UI settings change.
        unsafe {
            MsiSetInternalUI(INSTALLUILEVEL_NONE, None);
        }
        let mut changed = HANDLE::default();
        this.begin = unsafe {
            MsiBeginTransactionW(
                windows::core::w!("FutureClassroom controlled maintenance"),
                0,
                &mut this.transaction,
                &mut changed,
            )
        };
        if this.begin == 0 {
            if !changed.is_invalid() {
                this.resources.add(changed, Role::Control);
                this.changed = Some(changed);
            }
            if this.transaction.0 == 0 || this.changed.is_none() {
                return Err(LifecycleError::Unsettled);
            }
        } else if this.transaction.0 != 0 || !changed.is_invalid() {
            // Failed Begin outputs are not invented owned capabilities.
            this.resources.retain_until_exit();
            return Err(LifecycleError::Unsettled);
        }
        Ok(this)
    }
    fn same_thread(&self) -> Result<()> {
        if unsafe { GetCurrentThreadId() } != self.thread {
            Err(LifecycleError::Invalid)
        } else {
            Ok(())
        }
    }
    fn install(&self, path: &Path, operation: u8) -> Result<u32> {
        self.same_thread()?;
        if self.begin != 0 || self.ended {
            return Err(LifecycleError::Invalid);
        }
        let quiet =
            windows::core::w!("REBOOT=ReallySuppress MSIRESTARTMANAGERCONTROL=DisableShutdown");
        Ok(unsafe {
            match operation {
            1=>MsiInstallProductW(&HSTRING::from(path.as_os_str()),quiet),
            2=>MsiInstallProductW(&HSTRING::from(path.as_os_str()),windows::core::w!("REINSTALL=ALL REINSTALLMODE=amus REBOOT=ReallySuppress MSIRESTARTMANAGERCONTROL=DisableShutdown")),
            _=>return Err(LifecycleError::Invalid),
        }
        })
    }
    fn end(&mut self, commit: bool) -> Result<u32> {
        self.same_thread()?;
        if self.begin != 0 || self.ended {
            return Err(LifecycleError::Invalid);
        }
        let code = unsafe {
            MsiEndTransaction(if commit {
                MSITRANSACTIONSTATE_COMMIT
            } else {
                MSITRANSACTIONSTATE_ROLLBACK
            })
        };
        self.ended = true; // one actual End attempt; an error never authorizes retry
        if let Some(event) = self.changed {
            self.resources.close_handle(event)?;
            self.changed = None;
        }
        self.resources.release()?;
        if code != 0 {
            self.resources.retain_until_exit();
        }
        Ok(code)
    }
}
impl Drop for ApiOwner {
    fn drop(&mut self) {
        if self.begin == 0 && !self.ended {
            // Negative cleanup only, including a broken parent pipe or expired
            // wait. This cannot become a successful receipt or a replacement G.
            if self.same_thread().is_err() {
                self.resources.retain_until_exit();
                return;
            }
            self.ended = true;
            let rc = unsafe { MsiEndTransaction(MSITRANSACTIONSTATE_ROLLBACK) };
            if rc != 0 {
                self.resources.retain_until_exit();
            }
        }
    }
}

pub(super) fn guardian(s: &mut Session) -> Result<()> {
    s.sealed_package_sha.ok_or(LifecycleError::Invalid)?;
    let parent = s.resources.parent(unsafe { GetCurrentProcess() })?;
    s.authenticate_image(parent)?;
    let original = process_identity(parent)?;
    s.pipe_name = pipe_name(&s.pipe_name, original);
    let pipe = s.connect()?;
    let peer = s.peer(pipe, false)?;
    if process_identity(peer)? != original {
        return Err(LifecycleError::Invalid);
    }
    let mut hello = Message::new(HELLO);
    hello.thread = unsafe { GetCurrentThreadId() };
    send(s, pipe, hello)?;
    let ready = receive(s, pipe)?;
    let now = unsafe { GetTickCount64() };
    if ready.kind != READY || ready.ticks <= now {
        return Err(LifecycleError::Expired);
    }
    s.deadline = s
        .deadline
        .min(Instant::now() + Duration::from_millis(ready.ticks - now));
    let path = package(s)?;
    let mut api = ApiOwner::begin()?;
    let mut began = Message::new(BEGUN);
    began.code = api.begin;
    began.transaction = api.transaction.0;
    send(s, pipe, began)?;
    let run = receive(s, pipe)?;
    let mut outcome = 0;
    let end = if run.kind == ABORT && run.transaction == api.transaction.0 {
        if api.begin == 0 {
            api.end(false)?
        } else {
            u32::MAX
        }
    } else {
        if run.kind != RUN || run.transaction != api.transaction.0 || api.begin != 0 {
            return Err(LifecycleError::Invalid);
        }
        current(s.deadline)?;
        let rc = api.install(&path, run.outcome)?;
        let mut installed = Message::new(INSTALLED);
        installed.transaction = api.transaction.0;
        installed.code = rc;
        send(s, pipe, installed)?;
        let decision = receive(s, pipe)?;
        if decision.kind != DECIDE
            || decision.transaction != api.transaction.0
            || (decision.outcome == 1 && rc != 0)
        {
            return Err(LifecycleError::Invalid);
        }
        outcome = decision.outcome;
        api.end(outcome == 1)?
    };
    // Actual event/control return precedes this message. Close errors leave no
    // positive receipt. Failed Begin has no fabricated End return.
    let mut ended = Message::new(ENDED);
    ended.transaction = api.transaction.0;
    ended.outcome = outcome;
    ended.code = end;
    send(s, pipe, ended)?;
    s.finish_request(pipe, peer)?;
    if api.begin == 0 && end != 0 {
        return Err(LifecycleError::Unsettled);
    }
    Ok(())
}

struct Launched {
    process: HANDLE,
    guardian: GuardianIdentity,
    thread_id: u32,
}
fn launch(
    s: &mut Session,
    draft: &mut super::super::super::controlled::MaintenanceDraft,
) -> Result<Launched> {
    if s.resources.handles.len() + 2 > HANDLE_LIMIT {
        return Err(LifecycleError::Invalid);
    }
    let application = HSTRING::from(s.image.as_os_str());
    let mut command: Vec<u16> = format!("\"{}\" owned-guardian", s.image.display())
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
    s.resources.add(child.hProcess, Role::Control);
    s.resources.add(child.hThread, Role::Control);
    // No authentication or pipe wait may leave a launched G unaccounted for.
    let guardian = draft.watch_native_guardian(child.hProcess, child.hThread, child.dwThreadId)?;
    s.resources.close_handle(child.hThread)?;
    s.authenticate_image(child.hProcess)?;
    Ok(Launched {
        process: child.hProcess,
        guardian,
        thread_id: child.dwThreadId,
    })
}
#[allow(clippy::too_many_arguments)] // One original pipe/child/Draft cleanup boundary.
fn abort(
    s: &mut Session,
    pipe: HANDLE,
    peer: HANDLE,
    child: HANDLE,
    draft: &mut super::super::super::controlled::MaintenanceDraft,
    original: GuardianIdentity,
    began: Message,
) -> Result<()> {
    let mut request = Message::new(ABORT);
    request.transaction = began.transaction;
    send(s, pipe, request)?;
    let ended = receive(s, pipe)?;
    if ended.kind != ENDED
        || ended.transaction != began.transaction
        || ended.outcome != 0
        || (began.code == 0 && ended.code != 0)
        || (began.code != 0 && ended.code != u32::MAX)
    {
        return Err(LifecycleError::Unsettled);
    }
    s.release_connection(pipe, peer)?;
    s.resources.close_handle(pipe)?;
    returned_zero(child, s.deadline)?;
    // Return B's original source/image/process controls before Draft's gate.
    // A close failure must leave the independently watched Draft unreleased.
    s.resources.release()?;
    draft.try_abort(Some(DraftAbortObservation {
        guardian: original,
        begin: Some((began.code, began.transaction)),
        empty_rollback_return: if began.code == 0 {
            Some(ended.code)
        } else {
            None
        },
        control_return: 0,
    }))
}

/// Private actual Setup adapter, intentionally not in run_helper's user grammar.
/// A future separate Setup artifact must append the reviewed seal and use a new
/// verified inner guard. Existing packages/normal app startup cannot enter it.
pub(crate) fn run_setup(operation: Operation) -> Result<SetupResult> {
    // ConfigureProductEx selects registered LocalPackage, whose complete bytes
    // and CA identity are not proven by the separate sealed source MSI digest.
    // Keep this new route closed for uninstall until that scope is verified.
    if matches!(operation, Operation::Uninstall) {
        return Err(LifecycleError::Invalid);
    }
    let deadline = Instant::now() + super::super::super::broker::BROKER_BUDGET;
    let mut s = Session::new(deadline)?;
    s.sealed_package_sha.ok_or(LifecycleError::Invalid)?;
    let identity = InstallIdentity::for_current_user()?;
    let mut draft = LifecycleGate::try_draft_maintenance(&identity, deadline)?;
    package(&mut s)?;
    let own = process_identity(unsafe { GetCurrentProcess() })?;
    s.pipe_name = pipe_name(&s.pipe_name, own);
    let pipe = s.server()?;
    let child = launch(&mut s, &mut draft)?;
    s.accept(pipe)?;
    let peer = s.peer(pipe, true)?;
    if process_identity(peer)? != process_identity(child.process)? {
        return Err(LifecycleError::Invalid);
    }
    let hello = receive(&s, pipe)?;
    if hello.kind != HELLO || hello.thread != child.thread_id {
        return Err(LifecycleError::Invalid);
    }
    let original = child.guardian;
    let left = deadline
        .saturating_duration_since(Instant::now())
        .as_millis();
    if left == 0 || left > 120_000 {
        return Err(LifecycleError::Expired);
    }
    let mut ready = Message::new(READY);
    ready.ticks = unsafe { GetTickCount64() }
        .checked_add(left as u64)
        .ok_or(LifecycleError::Invalid)?;
    send(&s, pipe, ready)?;
    let began = receive(&s, pipe)?;
    if began.kind != BEGUN {
        return Err(LifecycleError::Invalid);
    }
    let observed = draft.observe_begin(BeginObservation {
        guardian: original,
        return_code: began.code,
        transaction_id: began.transaction,
    });
    if began.code != 0 {
        abort(
            &mut s,
            pipe,
            peer,
            child.process,
            &mut draft,
            original,
            began,
        )?;
        return Err(LifecycleError::Unsettled);
    }
    observed?;
    let admission = match effects::admit(&mut s).and_then(|admission| {
        effects::revalidate(&mut s, &admission)?;
        Ok(admission)
    }) {
        Ok(admission) => admission,
        Err(e) => {
            abort(
                &mut s,
                pipe,
                peer,
                child.process,
                &mut draft,
                original,
                began,
            )?;
            return Err(e);
        }
    };
    if !matches!(
        (operation, admission.operation()),
        (Operation::Install, MaintenanceOperation::Install)
            | (Operation::Repair, MaintenanceOperation::SameManifest)
    ) {
        abort(
            &mut s,
            pipe,
            peer,
            child.process,
            &mut draft,
            original,
            began,
        )?;
        return Err(LifecycleError::Invalid);
    }
    let mut owner = draft.arm(&identity)?;
    let result = (|| -> Result<SetupResult> {
        let mut run = Message::new(RUN);
        run.transaction = began.transaction;
        run.outcome = operation.tag();
        send(&s, pipe, run)?;
        let installed = receive(&s, pipe)?;
        if installed.kind != INSTALLED || installed.transaction != began.transaction {
            return Err(LifecycleError::Invalid);
        }
        let decision = if installed.code == 0 {
            effects::staged(&mut s, &admission)?
        } else {
            effects::failed_install(&admission)
        };
        let mut reply = Message::new(DECIDE);
        reply.transaction = began.transaction;
        reply.outcome = if decision.outcome() == SettledOutcome::Commit {
            1
        } else {
            2
        };
        send(&s, pipe, reply)?;
        let ended = receive(&s, pipe)?;
        if ended.kind != ENDED
            || ended.transaction != began.transaction
            || ended.outcome != reply.outcome
            || ended.code != 0
        {
            return Err(LifecycleError::Unsettled);
        }
        s.release_connection(pipe, peer)?;
        s.resources.close_handle(pipe)?;
        returned_zero(child.process, deadline)?;
        loop {
            match owner.resources.guardian_returned() {
                Ok(()) => break,
                Err(LifecycleError::GuardianRunning) => pause(deadline)?,
                Err(e) => return Err(e),
            }
        }
        let mut proof = effects::reconcile(&mut s, &admission, decision)?;
        proof.check()?;
        owner.record_controlled_settlement(TransactionObservation {
            operation_id: owner.operation_id(),
            guardian: original,
            ownership: TransactionOwnership::OwnedByBegin {
                begin_return: began.code,
                transaction_id: began.transaction,
            },
            outcome: decision.outcome(),
            install_return: installed.code,
            end_return: ended.code,
            control_return: 0,
            effects_reconciled: true,
        })?;
        proof.check()?;
        proof.release()?;
        s.resources.release()?;
        owner.try_release()?;
        Ok(SetupResult {
            install_return: installed.code,
            outcome: decision.outcome(),
        })
    })();
    if result.is_err() {
        owner.resources.retain_until_exit();
        s.resources.retain_until_exit();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup_seal_is_optional_for_legacy_but_never_zero_digest_authority() {
        assert_eq!(package_seal([0; 48]).unwrap(), None);
        let mut b = [0; 48];
        b[..16].copy_from_slice(PACKAGE_SEAL_MAGIC);
        assert!(package_seal(b).is_err());
        b[16..].fill(7);
        assert_eq!(package_seal(b).unwrap(), Some([7; 32]));
    }
    #[test]
    fn owned_messages_do_not_accept_legacy_join_or_unknown_fields() {
        assert!(Message::parse(Packet::new(BEGIN).encode()).is_err());
        let mut hello = Message::new(HELLO);
        hello.thread = 7;
        assert!(Message::parse(hello.bytes()).is_ok());
        for index in [6, 7, 20, 21, 22, 23] {
            let mut b = hello.bytes();
            b[index] = 1;
            assert!(Message::parse(b).is_err());
        }
        let mut failed = Message::new(BEGUN);
        failed.code = 1618;
        assert!(Message::parse(failed.bytes()).is_ok());
        failed.transaction = 7;
        assert!(Message::parse(failed.bytes()).is_err());
    }
}
