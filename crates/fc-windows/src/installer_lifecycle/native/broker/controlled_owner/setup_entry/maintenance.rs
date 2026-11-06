//! User-selected maintenance. MSI settlement remains inside original sealed Setup.
use super::standalone::Package;
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    process::Command,
};

const ARP: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\FutureClassroom-99378807";
fn io<T>(v: std::io::Result<T>) -> Result<T> {
    v.map_err(|e| LifecycleError::Native(HRESULT::from_win32(e.raw_os_error().unwrap_or(1) as u32)))
}

pub(super) fn installed() -> Result<Option<String>> {
    let mut product = [0u16; 39];
    let family = HSTRING::from(format!("{{{INSTALL_FAMILY}}}"));
    let rc = unsafe {
        MsiEnumRelatedProductsW(&family, None, 0, windows::core::PWSTR(product.as_mut_ptr()))
    };
    if rc == ERROR_NO_MORE_ITEMS.0 {
        return Ok(None);
    }
    if rc != 0 {
        return Err(LifecycleError::Invalid);
    }
    let mut other = [0u16; 39];
    if unsafe {
        MsiEnumRelatedProductsW(&family, None, 1, windows::core::PWSTR(other.as_mut_ptr()))
    } != ERROR_NO_MORE_ITEMS.0
    {
        return Err(LifecycleError::Unsettled);
    }
    let code =
        HSTRING::from(String::from_utf16(&product[..38]).map_err(|_| LifecycleError::Invalid)?);
    let mut text = [0u16; 64];
    let mut len = 64;
    let rc = unsafe {
        MsiGetProductInfoExW(
            &code,
            None,
            MSIINSTALLCONTEXT_USERUNMANAGED,
            windows::core::w!("VersionString"),
            Some(windows::core::PWSTR(text.as_mut_ptr())),
            Some(&mut len),
        )
    };
    if rc != 0 || len >= 64 {
        return Err(LifecycleError::Invalid);
    }
    Ok(Some(
        String::from_utf16(&text[..len as usize]).map_err(|_| LifecycleError::Invalid)?,
    ))
}
pub(super) fn exists() -> Result<bool> {
    Ok(installed()?.is_some()
        || crate::system::DataPaths::installed_bootstrap_for_current_user()?
            .parent()
            .ok_or(LifecycleError::Invalid)?
            .exists())
}

fn stop_product(force: bool) -> Result<()> {
    let paths = crate::system::DataPaths::program_data()?;
    let title = HSTRING::from(paths.root().join("runtime/instance.lock").as_os_str());
    let Ok(hwnd) =
        (unsafe { FindWindowW(windows::core::w!("FutureClassroom.HiddenHost.v1"), &title) })
    else {
        return Ok(());
    };
    paths.verify_root_for_current_user()?;
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    let mut resources = Resources::empty()?;
    resources.sid = resources.token_sid(unsafe { GetCurrentProcess() })?;
    let rights = PROCESS_QUERY_LIMITED_INFORMATION
        | PROCESS_SYNCHRONIZE
        | if force {
            PROCESS_TERMINATE
        } else {
            PROCESS_ACCESS_RIGHTS(0)
        };
    let process = unsafe { OpenProcess(rights, false, pid)? };
    resources.add(process, Role::Control);
    let target = crate::system::DataPaths::installed_bootstrap_for_current_user()?;
    if resources.token_sid(process)? != resources.sid
        || !image_path(process)?
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .eq_ignore_ascii_case(target.to_string_lossy().trim_start_matches(r"\\?\"))
    {
        return Err(LifecycleError::Invalid);
    }
    unsafe {
        PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))?;
    }
    let deadline = Instant::now() + Duration::from_secs(if force { 5 } else { 30 });
    while unsafe { WaitForSingleObject(process, 0) } == WAIT_TIMEOUT && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(40));
    }
    if unsafe { WaitForSingleObject(process, 0) } == WAIT_TIMEOUT && force {
        unsafe {
            TerminateProcess(process, 1602)?;
        }
    }
    // Termination is a request; only the original process handle's signal
    // allows the real lifecycle gate and native MSI checks to run afterwards.
    if unsafe { WaitForSingleObject(process, 5000) } != WAIT_OBJECT_0 {
        return Err(LifecycleError::Busy);
    }
    resources.release()
}

pub(super) fn run(mut package: Package, mode: &str, force: bool, delete_data: bool) -> Result<u32> {
    let mut source = package.extract()?;
    stop_product(force)?;
    let old = installed()?;
    let previous = old.as_deref() == Some(super::previous_source::VERSION);
    let invoke = |directory: &std::path::Path, operation: &str| -> Result<u32> {
        let mut child = io(Command::new(directory.join("Setup.exe"))
            .arg(operation)
            .current_dir(directory)
            .spawn())?;
        Ok(io(child.wait())?.code().ok_or(LifecycleError::Unsettled)? as u32)
    };
    let mut code;
    if previous {
        if !source.previous {
            return Err(LifecycleError::UpgradeProtocolUnsupported);
        }
        let prior = source.root.join("previous");
        code = invoke(
            &prior,
            if mode == "maintain-uninstall" {
                "uninstall"
            } else {
                "repair"
            },
        )?;
        if code == 0 && mode != "maintain-uninstall" {
            code = invoke(&source.root, "install")?;
        }
    } else {
        let operation = if mode == "maintain-uninstall" {
            "uninstall"
        } else if old.is_some() {
            "repair"
        } else {
            "install"
        };
        if old.is_none() {
            super::super::super::maintenance::update::remove_orphaned(&source.root)?;
        }
        code = if mode == "maintain-uninstall" && old.is_none() {
            0
        } else {
            invoke(&source.root, operation)?
        };
    }
    if code != 0 {
        return Ok(code);
    }
    if mode == "maintain-uninstall" {
        // Never delete teaching data after a failed/unknown MSI transaction.
        if installed()?.is_some() {
            return Err(LifecycleError::Unsettled);
        }
        if delete_data && purge_data().is_err() {
            return Ok(5101);
        }
        if remove_entry().is_err() {
            return Ok(5102);
        }
    } else {
        let target = crate::system::DataPaths::installed_bootstrap_for_current_user()?;
        io(Command::new(&target)
            .current_dir(target.parent().ok_or(LifecycleError::Invalid)?)
            .spawn())?;
    }
    source.cleanup()?;
    if mode == "maintain-uninstall" && schedule_cache_cleanup().is_err() {
        return Ok(5103);
    }
    Ok(0)
}

fn remove_tree(root: &std::path::Path) -> Result<()> {
    use crate::materials::files::ControlledFiles;
    if !root.exists() {
        return Ok(());
    }
    let parent = root.parent().ok_or(LifecycleError::Invalid)?;
    let files = ControlledFiles::new(parent)?;
    fn visit(files: &ControlledFiles, path: &std::path::Path) -> Result<()> {
        let m = io(fs::symlink_metadata(path))?;
        if std::os::windows::fs::MetadataExt::file_attributes(&m) & FILE_ATTRIBUTE_REPARSE_POINT.0
            != 0
        {
            return Err(LifecycleError::Invalid);
        }
        if m.is_dir() {
            for child in files.entries(path, 32768)? {
                visit(files, &child)?;
            }
            files.remove_empty_directory(path)?;
        } else {
            files.remove(path)?;
        }
        Ok(())
    }
    visit(&files, root)
}
fn purge_data() -> Result<()> {
    let identity = InstallIdentity::for_current_user()?;
    let mut gate = Resources::open(&identity, true)?;
    let paths = crate::system::DataPaths::program_data()?;
    if paths.root().exists() {
        paths.verify_root_for_current_user()?;
        remove_tree(paths.root())?;
    }
    if let Some(legacy) = crate::system::DataPaths::local_migration_source()? {
        if legacy.local_migration_retired()? {
            legacy.set_local_migration_retired(false)?;
        }
        remove_tree(legacy.root())?;
    }
    crate::tls_frontdoor::certificate_provision::remove_all_owned_certificates()
        .map_err(|_| LifecycleError::Unsettled)?;
    // Update identity, downloaded caches, school-group secrets and checkpoints
    // are product data too; SSH keys/services are outside every owned root.
    let updates = identity.local.join("FutureClassroom-Updates");
    if updates.exists() {
        let h = gate.open_file(
            &updates,
            READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            true,
            false,
        )?;
        verify_info(h, true)?;
        verify_acl(h, &identity.sid, true)?;
        gate.add(h, Role::Parent);
        gate.close_handle(h)?;
        remove_tree(&updates)?;
    }
    gate.release()
}

fn cache_root() -> Result<PathBuf> {
    Ok(InstallIdentity::for_current_user()?
        .local
        .join("FutureClassroom-Maintenance"))
}
fn cache_file(root: &std::path::Path) -> PathBuf {
    root.join(format!(
        "FutureClassroom-{}.exe",
        fc_domain::PRODUCT_DISPLAY_VERSION
    ))
}
fn set_string(key: HKEY, name: &str, value: &str) -> Result<()> {
    let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let bytes: Vec<u8> = wide.iter().flat_map(|v| v.to_le_bytes()).collect();
    unsafe {
        RegSetValueExW(key, &HSTRING::from(name), Some(0), REG_SZ, Some(&bytes)).ok()?;
    }
    Ok(())
}
pub(crate) fn remove_entry() -> Result<()> {
    let rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(ARP)) };
    if rc != ERROR_FILE_NOT_FOUND {
        rc.ok()?;
    }
    Ok(())
}

/// Invoked only after settled successful native install/repair. The canonical
/// four-member source is also cached for background LAN updates, not only SFX.
pub(crate) fn register_source() -> Result<()> {
    use crate::installer_lifecycle::broker::carrier;
    let image = io(std::env::current_exe())?;
    let directory = image.parent().ok_or(LifecycleError::Invalid)?;
    let sealed = io(fs::read(&image))?;
    let carrier = carrier::parse(&sealed)?;
    let msi = io(fs::read(directory.join("FutureClassroom.msi")))?;
    if <[u8; 32]>::from(Sha256::digest(&msi)) != carrier.digest_format_value() {
        return Err(LifecycleError::Invalid);
    }
    let installed = crate::system::DataPaths::installed_bootstrap_for_current_user()?;
    let app = installed.parent().ok_or(LifecycleError::Invalid)?;
    let members = [
        sealed.as_slice(),
        msi.as_slice(),
        &io(fs::read(app.join("LICENSE")))?,
        &io(fs::read(app.join("THIRD-PARTY-NOTICES.txt")))?,
    ];
    let mut header = super::standalone::MAGIC.to_vec();
    for n in std::iter::once(carrier.normal_helper().len()).chain(members.iter().map(|b| b.len())) {
        header.extend_from_slice(&(n as u64).to_le_bytes());
    }
    let mut digest = Sha256::new();
    digest.update(&header);
    let mut output = carrier.normal_helper().to_vec();
    for bytes in members {
        output.extend_from_slice(bytes);
        digest.update(bytes);
    }
    output.extend_from_slice(&header);
    output.extend_from_slice(&digest.finalize());
    let identity = InstallIdentity::for_current_user()?;
    let root = cache_root()?;
    let created = with_security(&identity.sid, true, |sa| unsafe {
        CreateDirectoryW(&native_path(&root)?, Some(sa))
    });
    if let Err(e) = created {
        if e.code() != ERROR_ALREADY_EXISTS.to_hresult() {
            return Err(e.into());
        }
    }
    let mut resources = Resources::empty()?;
    let pin = resources.open_file(
        &root,
        READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        OPEN_EXISTING,
        true,
        false,
    )?;
    resources.add(pin, Role::Parent);
    verify_info(pin, true)?;
    verify_acl(pin, &identity.sid, true)?;
    let lock = io(fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(root.join("maintenance.lock")))?;
    let target = cache_file(&root);
    match fs::read(&target) {
        Ok(existing) if existing == output => (),
        Ok(_) => return Err(LifecycleError::Invalid),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut file = io(fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(0)
                .open(&target))?;
            io(file.write_all(&output))?;
            io(file.sync_all())?;
        }
        Err(e) => {
            return Err(LifecycleError::Native(HRESULT::from_win32(
                e.raw_os_error().unwrap_or(1) as u32,
            )))
        }
    }
    use fc_ports::RandomSource;
    let mut nonce = [0; 16];
    crate::SystemRandom.fill(&mut nonce)?;
    let generation: String = nonce.iter().map(|v| format!("{v:02x}")).collect();
    io(fs::write(root.join("generation"), generation))?;
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(ARP),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
    }
    resources.add(HANDLE(key.0), Role::Registry);
    for (name, value) in [
        ("DisplayName", "未来教室".to_owned()),
        (
            "DisplayVersion",
            fc_domain::PRODUCT_DISPLAY_VERSION.to_owned(),
        ),
        ("Publisher", "Eliora".to_owned()),
        ("DisplayIcon", format!("\"{}\",0", target.display())),
        (
            "UninstallString",
            format!("\"{}\" uninstall", target.display()),
        ),
        (
            "ModifyPath",
            format!("\"{}\" maintenance", target.display()),
        ),
        ("InstallLocation", app.display().to_string()),
    ] {
        set_string(key, name, &value)?;
    }
    drop(lock);
    resources.release()
}

fn schedule_cache_cleanup() -> Result<()> {
    // A cached installer may be this very running image and its UI parent.
    // A hidden system PowerShell waits for those processes; generation + lock
    // prevents this cleanup from removing a newly reinstalled maintenance source.
    let root = cache_root()?;
    let generation = match fs::read_to_string(root.join("generation")) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(LifecycleError::Native(HRESULT::from_win32(
                e.raw_os_error().unwrap_or(1) as u32,
            )))
        }
    };
    if generation.len() != 32 || !generation.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(LifecycleError::Invalid);
    }
    let script = format!(
        r#"$ErrorActionPreference='Stop';$r=[IO.Path]::Combine([Environment]::GetFolderPath('LocalApplicationData'),'FutureClassroom-Maintenance');$g='{generation}';if([IO.File]::GetAttributes($r) -band [IO.FileAttributes]::ReparsePoint){{exit}};Get-Process | Where-Object {{try {{$_.Path -and ([IO.Path]::GetDirectoryName($_.Path) -eq $r)}}catch {{$false}}}} | ForEach-Object {{try {{$_.WaitForExit()}}catch {{}}}};$l=[IO.File]::Open([IO.Path]::Combine($r,'maintenance.lock'),'OpenOrCreate','ReadWrite','None');try {{if([IO.File]::ReadAllText([IO.Path]::Combine($r,'generation')) -ne $g){{exit}};Get-ChildItem -LiteralPath $r | Where-Object {{$_.Name -match '^FutureClassroom-1\.0\.0\.[0-9]+\.exe$' -and -not ($_.Attributes -band [IO.FileAttributes]::ReparsePoint)}} | ForEach-Object {{[IO.File]::Delete($_.FullName)}};[IO.File]::Delete([IO.Path]::Combine($r,'generation'))}}finally {{$l.Dispose()}};[IO.File]::Delete([IO.Path]::Combine($r,'maintenance.lock'));[IO.Directory]::Delete($r)"#
    );
    // EncodedCommand is a literal script, never a shell-interpolated path.
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            encoded.push(if i > chunk.len() {
                '='
            } else {
                alphabet[((n >> shift) & 63) as usize] as char
            });
        }
    }
    let mut windows_dir = [0u16; 1024];
    let n = unsafe {
        windows::Win32::System::SystemInformation::GetWindowsDirectoryW(Some(&mut windows_dir))
    } as usize;
    if n == 0 || n >= windows_dir.len() {
        return Err(LifecycleError::Invalid);
    }
    let ps =
        PathBuf::from(String::from_utf16(&windows_dir[..n]).map_err(|_| LifecycleError::Invalid)?)
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    io(Command::new(ps)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encoded,
        ])
        .creation_flags(0x08000000)
        .spawn())?;
    Ok(())
}
