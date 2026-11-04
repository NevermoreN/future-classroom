//! Current-user Shell links are reconciled only after a successful, settled
//! Setup operation. They are a separate Shell integration step, not MSI effects.
use super::{broker::SetupCommand, InstallIdentity};
use std::{
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
};
use windows::{
    core::{Error, Interface, Result, HSTRING},
    Win32::{
        Foundation::{E_ACCESSDENIED, E_FAIL},
        Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED, STGM_READ,
        },
        UI::{Shell::*, WindowsAndMessaging::SW_SHOWNORMAL},
    },
};

const DESCRIPTION: &str = "未来教室：启动桌面信息层（由安装程序管理）";
const NAMES: [&str; 2] = ["未来教室.lnk", "未来教室（应用）.lnk"];

struct Apartment;
impl Apartment {
    fn new() -> Result<Self> {
        // SAFETY: synchronous use on the calling Setup thread; S_FALSE also
        // owns one matching CoUninitialize, and changed apartment is rejected.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: every COM object below is dropped before this guard.
        unsafe { CoUninitialize() }
    }
}
fn folder(id: &windows::core::GUID) -> Result<PathBuf> {
    crate::com_task::with_task_memory(|memory| unsafe {
        let value = SHGetKnownFolderPath(id, KF_FLAG_CREATE, None)?;
        let text = value.to_string();
        memory.free(value.0.cast());
        Ok(PathBuf::from(text?))
    })
}
fn link() -> Result<IShellLinkW> {
    // SAFETY: current thread owns an initialized apartment and returned object.
    unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
}
fn text(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|v| *v == 0).unwrap_or(value.len())])
}
fn owned(path: &Path, target: &Path) -> Result<Option<bool>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || metadata.len() > 64 * 1024
    {
        return Ok(Some(false));
    }
    let shell = link()?;
    let file: IPersistFile = shell.cast()?;
    // Malformed or unrelated links are user files, never replacement targets.
    if unsafe { file.Load(&HSTRING::from(path.as_os_str()), STGM_READ) }.is_err() {
        return Ok(Some(false));
    }
    let mut destination = [0u16; 1024];
    let mut arguments = [0u16; 1024];
    let mut description = [0u16; 256];
    // SAFETY: fixed, live buffers; GetPath reads stored metadata only. Resolve
    // is deliberately never invoked and no external target is launched.
    let metadata = unsafe {
        shell
            .GetPath(
                &mut destination,
                std::ptr::null_mut(),
                SLGP_RAWPATH.0 as u32,
            )
            .and_then(|()| shell.GetArguments(&mut arguments))
            .and_then(|()| shell.GetDescription(&mut description))
    };
    if metadata.is_err() {
        return Ok(Some(false));
    }
    Ok(Some(
        text(&destination).eq_ignore_ascii_case(&target.to_string_lossy())
            && text(&arguments).is_empty()
            && text(&description) == DESCRIPTION,
    ))
}
fn save(path: &Path, target: &Path) -> Result<()> {
    let shell = link()?;
    let executable = HSTRING::from(target.as_os_str());
    let parent = target
        .parent()
        .ok_or_else(|| Error::from_hresult(E_ACCESSDENIED))?;
    // SAFETY: the fixed current-user installation target has been committed;
    // all strings live through each synchronous COM call and Save.
    unsafe {
        shell.SetPath(&executable)?;
        shell.SetArguments(&HSTRING::new())?;
        shell.SetWorkingDirectory(&HSTRING::from(parent.as_os_str()))?;
        shell.SetDescription(&HSTRING::from(DESCRIPTION))?;
        shell.SetIconLocation(&executable, 0)?;
        shell.SetShowCmd(SW_SHOWNORMAL)?;
        let file: IPersistFile = shell.cast()?;
        file.Save(&HSTRING::from(path.as_os_str()), true)?;
    }
    if owned(path, target)? != Some(true) {
        return Err(Error::from_hresult(E_ACCESSDENIED));
    }
    Ok(())
}
fn notify(path: &Path, event: SHCNE_ID) {
    let path = HSTRING::from(path.as_os_str());
    // SAFETY: Shell copies the terminated path during this synchronous call.
    unsafe { SHChangeNotify(event, SHCNF_PATHW, Some(path.as_ptr().cast()), None) }
}
fn reconcile_directory(directory: &Path, target: &Path, command: SetupCommand) -> Result<()> {
    let paths = NAMES.map(|name| directory.join(name));
    let states = [owned(&paths[0], target)?, owned(&paths[1], target)?];
    if command == SetupCommand::Uninstall {
        for (path, state) in paths.iter().zip(states) {
            if state == Some(true) {
                std::fs::remove_file(path)?;
                notify(path, SHCNE_DELETE);
            }
        }
    } else {
        // Prefer our existing link before an empty slot, so an upgrade never
        // creates a duplicate when a previously occupied name becomes free.
        let slot = states
            .iter()
            .position(|v| *v == Some(true))
            .or_else(|| states.iter().position(Option::is_none))
            .ok_or_else(|| Error::from_hresult(E_ACCESSDENIED))?;
        save(&paths[slot], target)?;
        notify(
            &paths[slot],
            if states[slot].is_none() {
                SHCNE_CREATE
            } else {
                SHCNE_UPDATEITEM
            },
        );
    }
    Ok(())
}
fn on_shell_thread(action: impl FnOnce() -> Result<()> + Send + 'static) -> Result<()> {
    // MSI discovery may initialize the caller's COM apartment as MTA. Shell
    // integration owns one fresh STA, and joins it before returning; no COM
    // object or apartment is transferred back to the transaction caller.
    std::thread::Builder::new()
        .name("fc-setup-shell".into())
        .spawn(move || {
            let _apartment = Apartment::new()?;
            action()
        })?
        .join()
        .map_err(|_| Error::from_hresult(E_FAIL))?
}
pub(super) fn reconcile(command: SetupCommand) -> Result<()> {
    on_shell_thread(move || {
        let identity =
            InstallIdentity::for_current_user().map_err(|_| Error::from_hresult(E_ACCESSDENIED))?;
        let target = identity
            .local
            .join("Programs")
            .join("FutureClassroom")
            .join("fc-bootstrap.exe");
        for directory in [folder(&FOLDERID_Desktop)?, folder(&FOLDERID_Programs)?] {
            reconcile_directory(&directory, &target, command)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::COINIT_MULTITHREADED;
    #[test]
    fn repair_and_uninstall_preserve_unrelated_links_and_do_not_duplicate_owned_links() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let _apartment = Apartment::new().unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("FC-Shortcut-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(directory.clone());
        let unrelated = directory.join(NAMES[0]);
        let ours = directory.join(NAMES[1]);
        let target = std::env::current_exe().unwrap();
        std::fs::write(&unrelated, b"unrelated user file").unwrap();
        reconcile_directory(&directory, &target, SetupCommand::Install).unwrap();
        assert_eq!(owned(&ours, &target).unwrap(), Some(true));
        reconcile_directory(&directory, &target, SetupCommand::Repair).unwrap();
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        reconcile_directory(&directory, &target, SetupCommand::Uninstall).unwrap();
        assert!(!ours.exists());
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"unrelated user file");
    }

    #[test]
    fn mta_caller_removes_owned_link_after_target_directory_is_deleted() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // Match a Setup thread whose earlier MSI discovery chose MTA. The old
        // inline STA initialization rejects this exact caller with 0x80010106.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        }
        let _caller_apartment = Apartment;
        let error = Apartment::new().err().expect("MTA cannot become STA");
        assert_eq!(error.code(), windows::core::HRESULT(0x80010106u32 as i32));
        eprintln!("INLINE_STA_HRESULT=0x{:08X}", error.code().0 as u32);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "FC-Shortcut-Missing-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(directory.clone());
        let installed = directory.join("installed");
        std::fs::create_dir(&installed).unwrap();
        let target = installed.join("fc-bootstrap.exe");
        std::fs::write(&target, b"isolated shortcut target").unwrap();
        let work_directory = directory.clone();
        on_shell_thread(move || {
            reconcile_directory(&work_directory, &target, SetupCommand::Install)?;
            std::fs::remove_file(&target)?;
            std::fs::remove_dir(&installed)?;
            assert_eq!(owned(&work_directory.join(NAMES[0]), &target)?, Some(true));
            reconcile_directory(&work_directory, &target, SetupCommand::Uninstall)
        })
        .unwrap();
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
    }
}
