//! Test-harness-only observer. No normal entrypoint, Session, lifecycle gate,
//! Pending, MSI mutation, registry mutation, process launcher or UI mutation.
use super::*;
use std::path::Component;

const NAME: &str =
    "installer_lifecycle::native::broker::context_probe::engine_parent_context_observer";
const TOKEN: &str = "fc-owned-parent-observer-v1:";
const MARKER: &[u8] = b"future-classroom-owned-r75-parent-observer/v1\n";

struct Request {
    nonce: String,
    root: PathBuf,
}
fn request(args: &[String]) -> Result<Option<Request>> {
    let armed = args.iter().any(|a| a.starts_with(TOKEN));
    if !armed {
        return Ok(None);
    }
    if args.len() != 6
        || args[0] != "--exact"
        || args[1] != NAME
        || args[2] != "--test-threads=1"
        || args[3] != "--nocapture"
        || args[4] != "--skip"
    {
        return Err(LifecycleError::Invalid);
    }
    let (nonce, root) = args[5]
        .strip_prefix(TOKEN)
        .and_then(|s| s.split_once(':'))
        .ok_or(LifecycleError::Invalid)?;
    let raw = root.as_bytes();
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || raw.len() < 3
        || !raw[0].is_ascii_alphabetic()
        || &raw[1..3] != b":\\"
        || root[2..].contains([':', '\0', '/', '"', '%', '[', ']'])
        || root.encode_utf16().count() > 240
        || root[3..]
            .split('\\')
            .any(|s| s.is_empty() || matches!(s, "." | "..") || s.ends_with([' ', '.']))
    {
        return Err(LifecycleError::Invalid);
    }
    let root = PathBuf::from(root);
    if root.components().count() > 16
        || root.file_name().and_then(|s| s.to_str())
            != Some(format!("FutureClassroom-r75-parent-observer-{nonce}").as_str())
    {
        return Err(LifecycleError::Invalid);
    }
    Ok(Some(Request {
        nonce: nonce.to_owned(),
        root,
    }))
}
fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn failure(error: LifecycleError) -> String {
    match error {
        LifecycleError::Native(hr) => {
            let win32 = if hr.0 as u32 & 0xffff0000 == 0x80070000 {
                (hr.0 as u32 & 0xffff).to_string()
            } else {
                String::from("null")
            };
            format!(
                "{{\"status\":\"ERROR\",\"kind\":\"Native\",\"hresult\":{},\"win32Code\":{win32}}}",
                hr.0
            )
        }
        error => format!(
            "{{\"status\":\"ERROR\",\"kind\":{}}}",
            quote(&format!("{error:?}"))
        ),
    }
}
fn sid_fact(result: Result<Vec<u8>>) -> String {
    match result {
        Ok(sid) => {
            let hex: String = sid.iter().map(|b| format!("{b:02x}")).collect();
            format!("{{\"status\":\"OK\",\"tokenUserSidHex\":{}}}", quote(&hex))
        }
        Err(error) => failure(error),
    }
}
fn identity_fact(process: HANDLE) -> String {
    match process_identity(process) {
        Ok(id) => format!(
            "{{\"status\":\"OK\",\"processId\":{},\"creationFiletime\":{}}}",
            id.process_id, id.creation_filetime
        ),
        Err(error) => failure(error),
    }
}
fn image_fact(process: HANDLE) -> String {
    match image_path(process) {
        Ok(path) => format!(
            "{{\"status\":\"OK\",\"image\":{}}}",
            quote(&path.to_string_lossy())
        ),
        Err(error) => failure(error),
    }
}
fn parent_id(owner: &mut Resources, deadline: Instant) -> Result<u32> {
    let id = unsafe { GetCurrentProcessId() };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? };
    owner.add(snapshot, Role::Control);
    let mut row = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut next = unsafe { Process32FirstW(snapshot, &mut row) };
    let mut result = Err(LifecycleError::Invalid);
    for _ in 0..8192 {
        current(deadline)?;
        match next {
            Ok(()) if row.th32ProcessID == id => {
                result = Ok(row.th32ParentProcessID);
                break;
            }
            Ok(()) => next = unsafe { Process32NextW(snapshot, &mut row) },
            Err(error) => {
                result = Err(error.into());
                break;
            }
        }
    }
    owner.close_handle(snapshot)?;
    result
}
fn thread_sid(owner: &mut Resources) -> String {
    let mut token = HANDLE::default();
    match unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token) } {
        Ok(()) => {
            owner.add(token, Role::Token);
            sid_fact(Resources::sid_from_token(token).map_err(Into::into))
        }
        Err(error) if error.code() == ERROR_NO_TOKEN.to_hresult() => {
            format!("{{\"status\":\"ABSENT\",\"hresult\":{}}}", error.code().0)
        }
        Err(error) => failure(error.into()),
    }
}
fn observe(request: Request) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut owner = Resources::empty()?;
    let own = unsafe { GetCurrentProcess() };
    let sid = owner.token_sid(own)?;
    // Pin every native path component before accepting the owned marker. No
    // reparse/parent alias can redirect the report outside this exact new root.
    let mut path = PathBuf::new();
    for component in request.root.components() {
        path.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let h = owner.open_file(
            &path,
            FILE_READ_ATTRIBUTES.0 | READ_CONTROL.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            true,
            false,
        )?;
        owner.add(h, Role::Parent);
        verify_info(h, true)?;
        if path == request.root {
            verify_acl(h, &sid, false)?;
        }
    }
    let marker = owner.open_file(
        &request.root.join(".owner"),
        GENERIC_READ.0,
        FILE_SHARE_READ,
        OPEN_EXISTING,
        false,
        false,
    )?;
    owner.add(marker, Role::Parent);
    verify_info(marker, false)?;
    let mut size = 0;
    unsafe {
        GetFileSizeEx(marker, &mut size)?;
    }
    if size != MARKER.len() as i64 {
        return Err(LifecycleError::Invalid);
    }
    let mut bytes = [0; MARKER.len()];
    owner.read_at(marker, 0, &mut bytes)?;
    if bytes != MARKER {
        return Err(LifecycleError::Invalid);
    }
    current(deadline)?;
    let own_identity = identity_fact(own);
    let own_image = image_fact(own);
    let own_sid = sid_fact(Ok(sid));
    let thread_sid = thread_sid(&mut owner);
    let parent = match parent_id(&mut owner, deadline) {
        Ok(pid) => match owner.process(pid, Role::Engine) {
            Ok(process) => format!("{{\"status\":\"OPENED_LIMITED_QUERY_SYNC\",\"snapshotParentId\":{pid},\"identity\":{},\"image\":{},\"processToken\":{}}}",
                identity_fact(process), image_fact(process), sid_fact(owner.token_sid(process).map_err(Into::into))),
            Err(error) => format!("{{\"snapshotParentId\":{pid},\"open\":{}}}", failure(error)),
        },
        Err(error) => failure(error),
    };
    let console = unsafe { windows::Win32::System::Console::GetConsoleWindow() };
    let json = format!("{{\"schemaVersion\":1,\"scope\":\"READ_ONLY_TEST_FACTS_NOT_INSTALLER_ACCEPTANCE\",\"nonce\":{},\"ownIdentity\":{own_identity},\"ownImage\":{own_image},\"ownProcessToken\":{own_sid},\"optionalThreadToken\":{thread_sid},\"parent\":{parent},\"consoleWindowPresent\":{},\"releaseProof\":\"REQUIRES_ACTUAL_TEST_PROCESS_EXIT0\"}}\n",
        quote(&request.nonce), !console.0.is_null());
    if json.len() > 8192 {
        return Err(LifecycleError::Invalid);
    }
    current(deadline)?;
    let output = owner.open_file(
        &request.root.join("context.json"),
        GENERIC_WRITE.0 | READ_CONTROL.0,
        FILE_SHARE_READ,
        CREATE_NEW,
        false,
        false,
    )?;
    owner.add(output, Role::Control);
    verify_info(output, false)?;
    let mut n = 0;
    unsafe {
        WriteFile(output, Some(json.as_bytes()), Some(&mut n), None)?;
        FlushFileBuffers(output)?;
    }
    if n as usize != json.len() {
        return Err(LifecycleError::Unsettled);
    }
    owner.release()?;
    current(deadline)?;
    // No identity/SID/path dump is sent to inherited stdout. Root verifies the
    // exact bounded create-new JSON plus actual same-SHA harness exit status.
    Ok(())
}

#[test]
fn engine_parent_context_observer() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if let Some(request) = request(&args).expect("closed test-only observer args") {
        observe(request).expect("owned read-only engine parent context observation");
    }
}

#[test]
fn observer_carrier_has_no_path_alias_or_normal_role() {
    assert!(request(&["begin".into()]).unwrap().is_none());
    let nonce = "0123456789abcdef0123456789abcdef";
    let root = format!(r"C:\test\FutureClassroom-r75-parent-observer-{nonce}");
    let args = vec![
        "--exact".into(),
        NAME.into(),
        "--test-threads=1".into(),
        "--nocapture".into(),
        "--skip".into(),
        format!("{TOKEN}{nonce}:{root}"),
    ];
    assert!(request(&args).unwrap().is_some());
    for bad in [
        root.replace("C:\\", "\\\\server\\"),
        root.replace("\\test\\", "\\test\\..\\"),
        root.replace("\\test\\", "\\test/"),
        format!("{root}\\"),
        format!("{root}:ads"),
        format!("{root}."),
        root.replace(nonce, "other"),
    ] {
        let mut bad_args = args.clone();
        bad_args[5] = format!("{TOKEN}{nonce}:{bad}");
        assert!(request(&bad_args).is_err());
    }
    let mut repeated = args.clone();
    repeated.extend(["--skip".into(), args[5].clone()]);
    assert!(request(&repeated).is_err());
}
