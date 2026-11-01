//! MSI tracks this process until every original child/broker capability returns.
//! A child's Join/End receipt cannot stand in for its original process exit.
use super::*;

pub(super) fn run(session: &mut Session, transaction: u32) -> Result<()> {
    session.engine(unsafe { GetCurrentProcess() })?;
    let pipe = session.connect()?;
    let mut hello = Packet::new(SUPERVISOR);
    hello.transaction = transaction;
    session.send(pipe, hello)?;
    let broker = session.peer(pipe, false)?;
    // Obtain our own original process object and continuously held read-only
    // file capability before completing hello or allowing any child launch.
    let broker_original = duplicate_process(&mut session.resources, broker, Role::Control)?;
    let image = retirement::Image::capture(session, broker_original)?;
    if session.receive(pipe)?.kind != ACK {
        return Err(LifecycleError::Unsettled);
    }
    session.finish_request(pipe, broker)?;

    // The child cannot reach Join until the original CreateProcess capability
    // has been checked. No suspended-thread leak or timing assumption is used.
    let own = process_identity(unsafe { GetCurrentProcess() })?;
    let child_name = child_pipe_name(&session.pipe_name, own);
    let original_name = std::mem::replace(&mut session.pipe_name, child_name);
    let child_pipe = session.server()?;
    let child = launch_guardian(session, transaction)?;
    let child_identity = process_identity(child)?;
    session.accept(child_pipe)?;
    let packet = session.receive(child_pipe)?;
    let connected = session.peer(child_pipe, true)?;
    if packet.kind != GUARDIAN
        || packet.transaction != transaction
        || process_identity(connected)? != child_identity
    {
        return Err(LifecycleError::Invalid);
    }
    live(child)?;
    session.send(child_pipe, Packet::new(ACK))?;
    session.release_connection(child_pipe, connected)?;
    session.resources.close_handle(child_pipe)?;
    session.pipe_name = original_name;
    returned_zero(child, session.deadline)?;
    session.resources.close_handle(child)?;
    // The broker independently sees this same child's original exit, reconciles
    // effects, closes its session, and releases Pending/gate before exit0.
    returned_zero(broker_original, session.deadline)?;
    session.resources.close_handle(broker_original)?;
    // Business settlement is already complete here. A retirement/close failure
    // is a supervisor cleanup failure, NOT a retained/recreated old Pending and
    // NOT authority to invent a rollback of the committed product/effects.
    image.retire(session)
}

pub(super) fn child_pipe_name(broker_name: &HSTRING, parent: GuardianIdentity) -> HSTRING {
    HSTRING::from(format!(
        "{broker_name}-Supervisor-{}-{}",
        parent.process_id, parent.creation_filetime
    ))
}

pub(super) fn await_parent(session: &mut Session, parent: HANDLE, transaction: u32) -> Result<()> {
    let expected = process_identity(parent)?;
    let name = child_pipe_name(&session.pipe_name, expected);
    let original_name = std::mem::replace(&mut session.pipe_name, name);
    let pipe = session.connect()?;
    let mut hello = Packet::new(GUARDIAN);
    hello.transaction = transaction;
    session.send(pipe, hello)?;
    let actual = session.peer(pipe, false)?;
    if process_identity(actual)? != expected || session.receive(pipe)?.kind != ACK {
        return Err(LifecycleError::Invalid);
    }
    session.finish_request(pipe, actual)?;
    session.pipe_name = original_name;
    Ok(())
}

fn launch_guardian(session: &mut Session, transaction: u32) -> Result<HANDLE> {
    current(session.deadline)?;
    if session.resources.handles.len() + 2 > HANDLE_LIMIT {
        return Err(LifecycleError::Invalid);
    }
    let application = HSTRING::from(session.image.as_os_str());
    let mut command: Vec<u16> = format!("\"{}\" {transaction} guardian", session.image.display())
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
            PROCESS_CREATION_FLAGS(0),
            None,
            None,
            &startup,
            &mut child,
        )?;
    }
    session.resources.add(child.hProcess, Role::Control);
    session.resources.add(child.hThread, Role::Control);
    session.resources.close_handle(child.hThread)?;
    // Verify the returned original child object before treating it as our
    // guardian. The broker separately verifies its original parent capability.
    let (identity, image) = session.authenticate_image(child.hProcess)?;
    let own = process_identity(unsafe { GetCurrentProcess() })?;
    if identity.process_id == own.process_id || identity.creation_filetime < own.creation_filetime {
        return Err(LifecycleError::Invalid);
    }
    if !same_image_path(&image_path(child.hProcess)?, &session.image) {
        return Err(LifecycleError::Invalid);
    }
    // Our original CreateProcess HANDLE remains owned; this transient image
    // need not pin the MSI chainer image across broker settlement.
    session.resources.close_handle(image)?;
    Ok(child.hProcess)
}
fn same_image_path(actual: &Path, expected: &Path) -> bool {
    actual
        .as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.as_os_str().to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_image_comparison_is_exact_path_not_leaf_or_prefix() {
        let expected = Path::new(r"C:\Windows\Installer\MSI123.tmp");
        assert!(same_image_path(
            Path::new(r"c:\WINDOWS\Installer\msi123.TMP"),
            expected
        ));
        for value in [
            r"C:\Other\MSI123.tmp",
            r"C:\Windows\Installer\MSI123.tmp.exe",
            r"C:\Windows\Installer\MSI124.tmp",
        ] {
            assert!(!same_image_path(Path::new(value), expected));
        }
    }
}
