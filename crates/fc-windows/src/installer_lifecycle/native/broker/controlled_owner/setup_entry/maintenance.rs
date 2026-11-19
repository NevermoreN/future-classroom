//! User-selected maintenance. MSI settlement remains inside original sealed Setup.
use super::standalone::Package;
use super::*;
use crate::installer_lifecycle::{broker::SETUP_SHELL_INCOMPLETE, LifecycleGate};
use fc_domain::updates::ReleaseVersion;
use fc_protocol::{installer_variants, update_release::ReleaseClaims};
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

#[derive(PartialEq, Eq)]
pub(super) struct RegisteredProduct {
    product: String,
    version: String,
    package: String,
}

fn registered() -> Result<Option<RegisteredProduct>> {
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
    if product[38] != 0 {
        return Err(LifecycleError::Invalid);
    }
    let product = String::from_utf16(&product[..38]).map_err(|_| LifecycleError::Invalid)?;
    let version = registered_property(&product, "VersionString")?;
    let package = registered_property(&product, "PackageCode")?;
    Ok(Some(RegisteredProduct {
        product,
        version,
        package,
    }))
}

pub(crate) fn installed() -> Result<Option<String>> {
    Ok(registered()?.map(|value| value.version))
}
pub(super) fn exists() -> Result<bool> {
    Ok(installed()?.is_some()
        || crate::system::DataPaths::installed_bootstrap_for_current_user()?
            .parent()
            .ok_or(LifecycleError::Invalid)?
            .exists())
}

fn registered_property(product: &str, name: &str) -> Result<String> {
    let mut text = [0u16; 512];
    let mut length = 512;
    let rc = unsafe {
        MsiGetProductInfoExW(
            &HSTRING::from(product),
            None,
            MSIINSTALLCONTEXT_USERUNMANAGED,
            &HSTRING::from(name),
            Some(windows::core::PWSTR(text.as_mut_ptr())),
            Some(&mut length),
        )
    };
    if rc != 0 || length >= 512 || text[length as usize] != 0 {
        return Err(LifecycleError::Invalid);
    }
    String::from_utf16(&text[..length as usize]).map_err(|_| LifecycleError::Invalid)
}

fn matching_code(registered: &str, signed: &[u8; 16]) -> bool {
    let hex: String = signed.iter().map(|byte| format!("{byte:02x}")).collect();
    registered
        .trim_matches(['{', '}'])
        .replace('-', "")
        .eq_ignore_ascii_case(&hex)
}

pub(crate) fn materialize_source(
    directory: &std::path::Path,
    release: &ReleaseClaims,
) -> Result<()> {
    materialize_for(directory, release, registered()?.as_ref())
}

/// Registration identifies an exact signed predecessor. Native Setup still
/// checks the actual cached MSI, complete installation and resource gates.
pub(super) fn materialize_for(
    directory: &std::path::Path,
    release: &ReleaseClaims,
    installed: Option<&RegisteredProduct>,
) -> Result<()> {
    let setup = io(fs::read(directory.join("Setup.exe")))?;
    let msi = io(fs::read(directory.join("FutureClassroom.msi")))?;
    let raw = io(fs::read(directory.join("installer-variants.bin")))?;
    let package = installer_variants::Package::parse(&raw).map_err(|_| LifecycleError::Invalid)?;
    package
        .validate_base(&setup, &msi)
        .map_err(|_| LifecycleError::Invalid)?;
    let selected = if let Some(installed) = installed {
        let version =
            ReleaseVersion::parse(&installed.version).map_err(|_| LifecycleError::Invalid)?;
        if version > release.version {
            return Err(LifecycleError::UpgradeProtocolUnsupported);
        }
        let code = installed.package.trim_matches(['{', '}']);
        if version == release.version {
            if code.eq_ignore_ascii_case(&package.catalog.base_package_code) {
                None
            } else {
                Some(
                    package
                        .catalog
                        .variants
                        .iter()
                        .position(|v| v.package_code.eq_ignore_ascii_case(code))
                        .ok_or(LifecycleError::UpgradeProtocolUnsupported)?,
                )
            }
        } else {
            let mut matches = release.allowed_predecessors.iter().filter(|claim| {
                matching_code(&installed.product, &claim.product_code)
                    && matching_code(code, &claim.package_code)
            });
            let predecessor = matches
                .next()
                .ok_or(LifecycleError::UpgradeProtocolUnsupported)?;
            if matches.next().is_some() {
                return Err(LifecycleError::Invalid);
            }
            package
                .predecessor_variant(
                    &installed.version,
                    installed.product.trim_matches(['{', '}']),
                    predecessor.cached_msi_sha256,
                )
                .map_err(|_| LifecycleError::UpgradeProtocolUnsupported)?
        }
    } else {
        None
    };
    if let Some(index) = selected {
        let (setup, msi) = package
            .materialize(index, &setup, &msi)
            .map_err(|_| LifecycleError::Invalid)?;
        io(fs::write(directory.join("Setup.exe"), setup))?;
        io(fs::write(directory.join("FutureClassroom.msi"), msi))?;
    }
    Ok(())
}

fn stop_product() -> Result<()> {
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
    let rights = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE;
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
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { WaitForSingleObject(process, 0) } == WAIT_TIMEOUT && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(40));
    }
    if unsafe { WaitForSingleObject(process, 0) } == WAIT_TIMEOUT {
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

pub(super) fn run(
    mut package: Package,
    release: Option<&ReleaseClaims>,
    mode: &str,
    delete_data: bool,
) -> Result<u32> {
    let registration = registered()?;
    let old = registration.as_ref().map(|value| value.version.as_str());
    let target = env!("CARGO_PKG_VERSION");
    let unified = package.unified();
    if unified && release.map(|claim| claim.version) != ReleaseVersion::parse(target).ok() {
        return Err(LifecycleError::Invalid);
    }
    let upgrade = old.is_some_and(|version| version != target);
    if mode == "maintain-uninstall" && upgrade {
        return Err(LifecycleError::UpgradeProtocolUnsupported);
    }
    let mut source = package.extract(release, registration.as_ref())?;
    // An intact predecessor can be checked while the current program is
    // running. Any unsupported/corrupt source must fail before asking it to stop.
    let predecessor = if upgrade {
        let release = release.ok_or(LifecycleError::Invalid)?;
        let facts = crate::installer_lifecycle::inspect_update_sources(&source.root)?;
        if ReleaseVersion::parse(&facts.version).ok() != Some(release.version)
            || !release.allowed_predecessors.contains(&facts.predecessor)
        {
            return Err(LifecycleError::Invalid);
        }
        crate::installer_lifecycle::qualify_update_predecessor(
            &facts.predecessor_wire,
            &facts.predecessor,
        )?;
        Some(facts)
    } else {
        None
    };
    if registered()? != registration {
        return Err(LifecycleError::Unsettled);
    }
    stop_product()?;
    let operation = if mode == "maintain-uninstall" {
        "uninstall"
    } else if old == Some(target) {
        "repair"
    } else if unified {
        crate::installer_lifecycle::broker::OFFLINE_INSTALL_COMMAND
    } else {
        "install"
    };
    if old.is_none() {
        super::super::super::maintenance::update::remove_orphaned(&source.root)?;
    }
    let code = if mode == "maintain-uninstall" && old.is_none() {
        Ok(0)
    } else {
        let child = io(Command::new(source.root.join("Setup.exe"))
            .arg(operation)
            .current_dir(&source.root)
            .spawn());
        match child {
            Ok(mut child) => {
                // Recover a transient wait error only while the original
                // handle can still prove that this Setup has exited.
                let status = match child.wait() {
                    Ok(status) => status,
                    Err(_) => io(child.try_wait())?.ok_or(LifecycleError::Unsettled)?,
                };
                Ok(status.code().ok_or(LifecycleError::Unsettled)? as u32)
            }
            Err(error) => Err(error),
        }
    };
    if !code
        .as_ref()
        .is_ok_and(|&code| code == 0 || code == SETUP_SHELL_INCOMPLETE)
    {
        if let Some(facts) = predecessor {
            let identity = InstallIdentity::for_current_user()?;
            if LifecycleGate::probe_quiescent(&identity).is_ok()
                && crate::installer_lifecycle::qualify_update_predecessor(
                    &facts.predecessor_wire,
                    &facts.predecessor,
                )
                .is_ok()
            {
                let _ = start_product();
            }
        }
        return code;
    }
    let mut result = code?;
    if mode == "maintain-uninstall" {
        // Never delete teaching data after a failed/unknown MSI transaction.
        if installed()?.is_some() {
            return Err(LifecycleError::Unsettled);
        }
        if delete_data && purge_data().is_err() {
            result = 5101;
        }
        if remove_entry().is_err() && result != 5101 {
            result = 5102;
        }
    } else {
        if installed()?.as_deref() != Some(target) {
            return Err(LifecycleError::Unsettled);
        }
        if start_product().is_err() {
            result = 5105;
        }
    }
    let extracted_cleaned = source.cleanup().is_ok();
    if mode == "maintain-uninstall" {
        // Keep the cached maintenance entry available when a requested data
        // purge needs a retry. A shortcut failure does not block cache cleanup.
        if result != 5101 && result != 5102 {
            let scheduled = schedule_cache_cleanup().is_ok();
            if result == 0 && (!extracted_cleaned || !scheduled) {
                result = 5103;
            }
        }
    } else if result == 0 && !extracted_cleaned {
        result = SETUP_SHELL_INCOMPLETE;
    }
    Ok(result)
}

fn start_product() -> Result<()> {
    let target = crate::system::DataPaths::installed_bootstrap_for_current_user()?;
    io(Command::new(&target)
        .current_dir(target.parent().ok_or(LifecycleError::Invalid)?)
        .spawn())?;
    Ok(())
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
    if registered()?.is_some() {
        return Err(LifecycleError::Unsettled);
    }
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
pub(super) fn local_legacy_cache(path: &std::path::Path) -> Result<bool> {
    if installed()?.as_deref() != Some(env!("CARGO_PKG_VERSION"))
        || path != cache_file(&cache_root()?)
    {
        return Ok(false);
    }
    let identity = InstallIdentity::for_current_user()?;
    let mut resources = Resources::empty()?;
    let root = cache_root()?;
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
    let attributes =
        std::os::windows::fs::MetadataExt::file_attributes(&io(fs::symlink_metadata(path))?);
    resources.release()?;
    Ok(attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0)
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

pub(crate) fn distribution_source() -> Result<Option<PathBuf>> {
    if installed()?.as_deref() != Some(env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let root = cache_root()?;
    if !root.exists() {
        return Ok(None);
    }
    let identity = InstallIdentity::for_current_user()?;
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
    let target = cache_file(&root);
    if !target.exists() {
        resources.release()?;
        return Ok(None);
    }
    let mut package = Package::open(&target)?.ok_or(LifecycleError::Invalid)?;
    if !package.unified() {
        resources.release()?;
        return Ok(None);
    }
    package.members()?;
    resources.release()?;
    Ok(Some(target))
}
pub(crate) fn cache_distribution(path: &std::path::Path) -> Result<()> {
    let mut package = Package::open(path)?.ok_or(LifecycleError::Invalid)?;
    if !package.unified() {
        return Err(LifecycleError::Invalid);
    }
    let installed = registered()?.ok_or(LifecycleError::Invalid)?;
    if installed.version != env!("CARGO_PKG_VERSION") {
        return Err(LifecycleError::Invalid);
    }
    let members = package.members()?;
    let raw = &members
        .iter()
        .find(|(n, _)| *n == "installer-variants.bin")
        .ok_or(LifecycleError::Invalid)?
        .1;
    let catalog = fc_protocol::installer_variants::Package::parse(raw)
        .map_err(|_| LifecycleError::Invalid)?
        .catalog;
    let code = installed.package.trim_matches(['{', '}']);
    if !catalog.base_package_code.eq_ignore_ascii_case(code)
        && !catalog
            .variants
            .iter()
            .any(|v| v.package_code.eq_ignore_ascii_case(code))
    {
        return Err(LifecycleError::Invalid);
    }
    store_distribution(&package.original()?).map(|_| ())
}
fn store_distribution(output: &[u8]) -> Result<PathBuf> {
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
    let write = match fs::read(&target) {
        Ok(existing) if existing.as_slice() == output => false,
        Ok(_) => {
            let mut old = Package::open(&target)?.ok_or(LifecycleError::Invalid)?;
            if old.unified()
                || output.len() < 64
                || &output[output.len() - 64..output.len() - 48] != b"FC-SFX-PACK\0v4\0\0"
            {
                return Err(LifecycleError::Invalid);
            }
            old.members()?;
            drop(old);
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => {
            return Err(LifecycleError::Native(HRESULT::from_win32(
                e.raw_os_error().unwrap_or(1) as u32,
            )))
        }
    };
    use fc_ports::RandomSource;
    let mut nonce = [0; 16];
    crate::SystemRandom.fill(&mut nonce)?;
    let generation: String = nonce.iter().map(|v| format!("{v:02x}")).collect();
    if write {
        let temporary = root.join(format!("source-{generation}.tmp"));
        let result: Result<()> = (|| {
            let mut file = io(fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(0)
                .open(&temporary))?;
            io(file.write_all(output))?;
            io(file.sync_all())?;
            drop(file);
            unsafe {
                windows::Win32::Storage::FileSystem::MoveFileExW(
                    &native_path(&temporary)?,
                    &native_path(&target)?,
                    windows::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                        | windows::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
                )?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
    }
    io(fs::write(root.join("generation"), generation))?;
    drop(lock);
    resources.release()?;
    Ok(target)
}

/// Invoked only after settled successful native install/repair. Preserve the
/// complete original signed distribution for maintenance and LAN sharing.
pub(crate) fn register_source() -> Result<()> {
    let image = io(std::env::current_exe())?;
    let directory = image.parent().ok_or(LifecycleError::Invalid)?;
    let original = directory.join("OriginalInstaller.exe");
    let setup = io(fs::read(directory.join("Setup.exe")))?;
    let msi = io(fs::read(directory.join("FutureClassroom.msi")))?;
    let installed = crate::system::DataPaths::installed_bootstrap_for_current_user()?;
    let app = installed.parent().ok_or(LifecycleError::Invalid)?;
    let output = if original.exists() {
        let mut package = Package::open(&original)?.ok_or(LifecycleError::Invalid)?;
        let members = package.members()?;
        let variants_raw = members
            .iter()
            .find(|(n, _)| *n == "installer-variants.bin")
            .ok_or(LifecycleError::Invalid)?
            .1
            .as_slice();
        let variants = fc_protocol::installer_variants::Package::parse(variants_raw)
            .map_err(|_| LifecycleError::Invalid)?;
        let matches = |s: &fc_protocol::installer_variants::Source, bytes: &[u8]| {
            s.bytes == bytes.len() && s.sha256 == format!("{:x}", Sha256::digest(bytes))
        };
        if !(matches(&variants.catalog.base_setup, &setup)
            && matches(&variants.catalog.base_msi, &msi))
            && !variants
                .catalog
                .variants
                .iter()
                .any(|v| matches(&v.setup, &setup) && matches(&v.msi, &msi))
        {
            return Err(LifecycleError::Invalid);
        }
        package.original()?
    } else {
        // Compatibility only for the previous coordinator's four-member stage.
        // Native Setup already settled its exact target source. This local-only
        // fallback is never advertised as a signed universal LAN distribution.
        if self::installed()?.as_deref() != Some(env!("CARGO_PKG_VERSION")) {
            return Err(LifecycleError::Invalid);
        }
        let carrier = crate::installer_lifecycle::broker::carrier::parse(&setup)?;
        if Sha256::digest(&msi).as_slice() != carrier.digest_format_value() {
            return Err(LifecycleError::Invalid);
        }
        let license = io(fs::read(app.join("LICENSE")))?;
        let notices = io(fs::read(app.join("THIRD-PARTY-NOTICES.txt")))?;
        let members = [
            setup.as_slice(),
            msi.as_slice(),
            license.as_slice(),
            notices.as_slice(),
        ];
        let mut header = super::standalone::LEGACY_MAGIC.to_vec();
        for size in
            std::iter::once(carrier.normal_helper().len()).chain(members.iter().map(|b| b.len()))
        {
            header.extend_from_slice(&(size as u64).to_le_bytes());
        }
        let mut digest = Sha256::new();
        digest.update(&header);
        let mut bytes = carrier.normal_helper().to_vec();
        for member in members {
            digest.update(member);
            bytes.extend_from_slice(member);
        }
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&digest.finalize());
        bytes
    };
    let target = store_distribution(&output)?;
    let mut resources = Resources::empty()?;
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
        r#"$ErrorActionPreference='Stop';$r=[IO.Path]::Combine([Environment]::GetFolderPath('LocalApplicationData'),'FutureClassroom-Maintenance');$g='{generation}';if([IO.File]::GetAttributes($r) -band [IO.FileAttributes]::ReparsePoint){{exit}};Get-Process | Where-Object {{try {{$_.Path -and ([IO.Path]::GetDirectoryName($_.Path) -eq $r)}}catch {{$false}}}} | ForEach-Object {{try {{$_.WaitForExit()}}catch {{}}}};$l=[IO.File]::Open([IO.Path]::Combine($r,'maintenance.lock'),'OpenOrCreate','ReadWrite','None');try {{if([IO.File]::ReadAllText([IO.Path]::Combine($r,'generation')) -ne $g){{exit}};Get-ChildItem -LiteralPath $r | Where-Object {{$_.Name -match '^FutureClassroom-[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+\.exe$' -and -not ($_.Attributes -band [IO.FileAttributes]::ReparsePoint)}} | ForEach-Object {{[IO.File]::Delete($_.FullName)}};[IO.File]::Delete([IO.Path]::Combine($r,'generation'))}}finally {{$l.Dispose()}};[IO.File]::Delete([IO.Path]::Combine($r,'maintenance.lock'));[IO.Directory]::Delete($r)"#
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
