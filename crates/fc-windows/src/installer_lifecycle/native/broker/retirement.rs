//! Retirement is restricted to the authenticated live broker's original MSI
//! temporary image. An uninterrupted original read capability bridges exit and
//! the second DELETE open; no receipt/path/PID disappearance grants deletion.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    volume: u32,
    index: u64,
    size: u64,
    creation: u64,
    write: u64,
    attributes: u32,
    links: u32,
}
fn filetime(value: FILETIME) -> u64 {
    u64::from(value.dwLowDateTime) | (u64::from(value.dwHighDateTime) << 32)
}
fn identity(handle: HANDLE) -> Result<Identity> {
    verify_info(handle, false)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        GetFileInformationByHandle(handle, &mut info)?;
    }
    Ok(Identity {
        volume: info.dwVolumeSerialNumber,
        index: u64::from(info.nFileIndexLow) | (u64::from(info.nFileIndexHigh) << 32),
        size: u64::from(info.nFileSizeLow) | (u64::from(info.nFileSizeHigh) << 32),
        creation: filetime(info.ftCreationTime),
        write: filetime(info.ftLastWriteTime),
        attributes: info.dwFileAttributes,
        links: info.nNumberOfLinks,
    })
}
fn installer_image(path: &Path, windows: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    parent
        .as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&windows.join("Installer").as_os_str().to_string_lossy())
        && lower.starts_with("msi")
        && lower.ends_with(".tmp")
        && lower.len() > 7
        && lower[3..lower.len() - 4]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric())
}

pub(super) struct Image {
    read: HANDLE,
    path: PathBuf,
    original: Identity,
    sha: [u8; 32],
}
impl Image {
    pub(super) fn capture(session: &mut Session, process: HANDLE) -> Result<Self> {
        live(process)?;
        let path = image_path(process)?;
        let mut windows = [0; PATH_LIMIT];
        let length = unsafe { GetWindowsDirectoryW(Some(&mut windows)) } as usize;
        if length == 0
            || length >= windows.len()
            || !installer_image(
                &path,
                &PathBuf::from(OsString::from_wide(&windows[..length])),
            )
        {
            return Err(LifecycleError::Invalid);
        }
        let image = Self::capture_path(session, path)?;
        live(process)?;
        Ok(image)
    }
    fn capture_path(session: &mut Session, path: PathBuf) -> Result<Self> {
        // Desired READ remains compatible with the broker's existing READ-only
        // sharing. This handle permits delete but grants no DELETE itself; all
        // original broker pins remain unchanged and prevent deletion while live.
        let read = session.resources.open_file(
            &path,
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            OPEN_EXISTING,
            false,
            false,
        )?;
        session.resources.add(read, Role::Control);
        let original = identity(read)?;
        let sha = session
            .resources
            .digest(read, 64 * 1024 * 1024, session.deadline)?;
        if sha != session.image_sha || identity(read)? != original {
            return Err(LifecycleError::Invalid);
        }
        Ok(Self {
            read,
            path,
            original,
            sha,
        })
    }
    pub(super) fn retire(self, session: &mut Session) -> Result<()> {
        current(session.deadline)?;
        // The original read object is STILL held. A second open can never
        // authorize a substituted namespace entry, even with identical bytes.
        let delete = session.resources.open_file(
            &self.path,
            GENERIC_READ.0 | DELETE.0,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            OPEN_EXISTING,
            false,
            false,
        )?;
        session.resources.add(delete, Role::Control);
        if identity(self.read)? != self.original
            || identity(delete)? != self.original
            || session
                .resources
                .digest(delete, 64 * 1024 * 1024, session.deadline)?
                != self.sha
            || identity(delete)? != self.original
            || identity(self.read)? != self.original
        {
            return Err(LifecycleError::Invalid);
        }
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        unsafe {
            SetFileInformationByHandle(
                delete,
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of_val(&disposition) as u32,
            )?;
        }
        session.resources.close_handle(delete)?;
        session.resources.close_handle(self.read)?;
        // A successful disposition/close is insufficient without external
        // namespace absence. Any access-denied/unknown result remains failure.
        match session.resources.open_file(
            &self.path,
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            OPEN_EXISTING,
            false,
            false,
        ) {
            Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
            Err(error) => Err(error.into()),
            Ok(handle) => {
                session.resources.add(handle, Role::Control);
                Err(LifecycleError::Unsettled)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        session: Session,
    }
    impl Fixture {
        fn new() -> Self {
            use fc_ports::RandomSource;
            let mut nonce = [0; 16];
            crate::SystemRandom.fill(&mut nonce).unwrap();
            let suffix: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
            let root =
                std::env::temp_dir().join(format!("FutureClassroom-Test-Retirement-{suffix}"));
            std::fs::create_dir(&root).unwrap();
            std::fs::write(
                root.join(".owner"),
                b"future-classroom-owned-retirement-test/v1\n",
            )
            .unwrap();
            let path = root.join("image.tmp");
            std::fs::write(&path, b"owned original image fixture bytes").unwrap();
            let mut resources = Resources::empty().unwrap();
            let handle = resources
                .open_file(
                    &path,
                    GENERIC_READ.0,
                    FILE_SHARE_READ | FILE_SHARE_DELETE,
                    OPEN_EXISTING,
                    false,
                    false,
                )
                .unwrap();
            resources.add(handle, Role::Control);
            let deadline = Instant::now() + Duration::from_secs(10);
            let image_sha = resources.digest(handle, 1024, deadline).unwrap();
            resources.close_handle(handle).unwrap();
            let session = Session {
                resources,
                image: path,
                manifest: Manifest {
                    product: String::new(),
                    version: String::new(),
                    entries: Vec::new(),
                },
                image_sha,
                pipe_name: HSTRING::new(),
                deadline,
                peer_images: Vec::new(),
                sealed_package_sha: None,
                captured_source: None,
            };
            Self { root, session }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // A test cleanup never removes arbitrary data: this create-new
            // random root has only the explicit marker and synthetic files.
            self.session.resources.release().unwrap();
            assert_eq!(
                std::fs::read(self.root.join(".owner")).unwrap(),
                b"future-classroom-owned-retirement-test/v1\n"
            );
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }
    #[test]
    fn original_read_cap_survives_strict_pin_then_retires_same_file() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut fixture = Fixture::new();
        let session = &mut fixture.session;
        let strict = session
            .resources
            .open_file(
                &session.image,
                GENERIC_READ.0,
                FILE_SHARE_READ,
                OPEN_EXISTING,
                false,
                false,
            )
            .unwrap();
        session.resources.add(strict, Role::Control);
        let pinned = Image::capture_path(session, session.image.clone()).unwrap();
        let original = identity(pinned.read).unwrap();
        let denied = session
            .resources
            .open_file(
                &session.image,
                GENERIC_READ.0 | DELETE.0,
                FILE_SHARE_READ | FILE_SHARE_DELETE,
                OPEN_EXISTING,
                false,
                false,
            )
            .unwrap_err();
        assert_eq!(denied.code(), ERROR_SHARING_VIOLATION.to_hresult());
        assert!(std::fs::write(&session.image, b"replacement").is_err());
        session.resources.close_handle(strict).unwrap();
        assert_eq!(
            identity(pinned.read).unwrap(),
            original,
            "same uninterrupted read object after strict pin return"
        );
        pinned.retire(session).unwrap();
        assert!(
            !session.image.exists(),
            "same-object disposition and external absence"
        );
        assert!(session.resources.handles.is_empty());
    }
    #[test]
    fn namespace_replacement_is_rejected_while_original_read_cap_stays_held() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut fixture = Fixture::new();
        let moved = fixture.root.join("original-renamed.tmp");
        let session = &mut fixture.session;
        let bytes = std::fs::read(&session.image).unwrap();
        let pinned = Image::capture_path(session, session.image.clone()).unwrap();
        let original = identity(pinned.read).unwrap();
        // The retained READ/shareDELETE capability permits a namespace rename,
        // but the original object is never closed or replaced by that rename.
        std::fs::rename(&session.image, &moved).unwrap();
        std::fs::write(&session.image, &bytes).unwrap();
        assert_eq!(identity(pinned.read).unwrap(), original);
        assert_eq!(pinned.retire(session), Err(LifecycleError::Invalid));
        session.resources.release().unwrap();
        assert_eq!(std::fs::read(&session.image).unwrap(), bytes);
        assert_eq!(
            std::fs::read(moved).unwrap(),
            bytes,
            "neither substituted nor original object was deleted"
        );
    }
    #[test]
    fn only_exact_os_installer_temp_namespace_is_eligible() {
        let windows = Path::new(r"C:\Windows");
        assert!(installer_image(
            Path::new(r"c:\windows\Installer\MSI123.tmp"),
            windows
        ));
        for path in [
            r"C:\Windows\Installer2\MSI123.tmp",
            r"C:\Elsewhere\MSI123.tmp",
            r"C:\Windows\Installer\MSI.tmp",
            r"C:\Windows\Installer\MSI123.tmp.exe",
            r"C:\Windows\Installer\MSI12-3.tmp",
            r"C:\Windows\Installer\sub\MSI123.tmp",
        ] {
            assert!(!installer_image(Path::new(path), windows), "{path}");
        }
    }
    #[test]
    fn replacement_even_with_same_size_or_hash_cannot_match_original_object() {
        let original = Identity {
            volume: 1,
            index: 42,
            size: 100,
            creation: 11,
            write: 12,
            attributes: 32,
            links: 1,
        };
        for value in [
            Identity {
                volume: 2,
                ..original
            },
            Identity {
                index: 43,
                ..original
            },
            Identity {
                size: 101,
                ..original
            },
            Identity {
                creation: 13,
                ..original
            },
            Identity {
                write: 14,
                ..original
            },
            Identity {
                links: 2,
                ..original
            },
            Identity {
                attributes: FILE_ATTRIBUTE_REPARSE_POINT.0,
                ..original
            },
        ] {
            assert_ne!(original, value);
        }
    }
}
