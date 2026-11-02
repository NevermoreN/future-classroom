//! Current Windows-account deployment relocation, before any Backend/writer.
//! Caller retains the ProgramData InstanceGuard for the entire normal process.
//! The worker acquires the original local root's actual InstanceGuard as well.
//! No ordinary backup projection, ID remapping or credential re-encryption.
use fc_domain::recovery::{MAX_BACKUP_ENTRIES, MAX_DATABASE_BYTES, MAX_MEDIA_BYTES};
use fc_security::{CredentialRecordCodec, Sha256Stream};
use fc_windows::{
    materials::files::{ControlledFiles, LockedFile, OwnedObjectPin, PinnedDirectory},
    system::{current_user_sid, CurrentUserDpapi, DataPaths},
    InstanceGuard,
};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path},
};

const MARKER: &[u8] = b"FutureClassroom.Deployment.v1\n";
const JOURNAL: &str = "runtime/local-data-migration.v1.bin";
const READY: &str = "runtime/local-data-migration.ready.bin";
const DONE: &str = "runtime/local-data-migration.done";
const KEY: &str = "config/credential-key.dpapi";
const DEPLOYMENT: &str = "config/deployment.marker";
const TREES: &[&str] = &["teachers", "profile-content", "voice", "exports"];
const NAMESPACES: &[&str] = &[
    "state",
    "config",
    "runtime",
    "teachers",
    "open-copies",
    "quarantine",
    "cache",
    "exports",
    "profile-content",
    "voice",
    "staging",
    "launch-copies",
    "web-material-staging",
    "recovery",
    "reinitialization",
];

#[derive(Debug)]
pub enum Error {
    Unavailable,
    InvalidDeployment,
    Cancelled,
    ConcurrentRoots,
    Interrupted,
}
type Result<T> = std::result::Result<T, Error>;
fn checked<T, E>(r: std::result::Result<T, E>) -> Result<T> {
    r.map_err(|_| Error::Unavailable)
}
fn require(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::InvalidDeployment)
    }
}
fn active(check: &dyn Fn() -> bool) -> Result<()> {
    if check() {
        Ok(())
    } else {
        Err(Error::Cancelled)
    }
}

struct Journal {
    user_sid: String,
    source: (u32, u64),
    target: (u32, u64),
    school: u64,
    key_sha256: [u8; 32],
}
struct FileFact {
    relative: String,
    bytes: u64,
    sha256: [u8; 32],
}
struct Ready {
    database_bytes: u64,
    database_sha256: [u8; 32],
    files: Vec<FileFact>,
}

fn take<const N: usize>(input: &mut &[u8]) -> Result<[u8; N]> {
    if input.len() < N {
        return Err(Error::InvalidDeployment);
    }
    let (head, tail) = input.split_at(N);
    *input = tail;
    checked(head.try_into())
}
impl Journal {
    fn encode(&self) -> Vec<u8> {
        let mut b = b"FCLOCAL1".to_vec();
        b.extend((self.user_sid.len() as u16).to_le_bytes());
        b.extend(self.user_sid.as_bytes());
        for (volume, index) in [self.source, self.target] {
            b.extend(volume.to_le_bytes());
            b.extend(index.to_le_bytes());
        }
        b.extend(self.school.to_le_bytes());
        b.extend(self.key_sha256);
        b
    }
    fn decode(mut b: &[u8]) -> Result<Self> {
        require(take::<8>(&mut b)? == *b"FCLOCAL1")?;
        let len = u16::from_le_bytes(take(&mut b)?) as usize;
        require(len > 0 && len <= 256 && b.len() >= len)?;
        let user_sid = checked(std::str::from_utf8(&b[..len]))?.to_owned();
        b = &b[len..];
        let source = (
            u32::from_le_bytes(take(&mut b)?),
            u64::from_le_bytes(take(&mut b)?),
        );
        let target = (
            u32::from_le_bytes(take(&mut b)?),
            u64::from_le_bytes(take(&mut b)?),
        );
        let school = u64::from_le_bytes(take(&mut b)?);
        let key_sha256 = take(&mut b)?;
        require(b.is_empty())?;
        Ok(Self {
            user_sid,
            source,
            target,
            school,
            key_sha256,
        })
    }
}
impl Ready {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut b = b"FCREADY1".to_vec();
        b.extend(self.database_bytes.to_le_bytes());
        b.extend(self.database_sha256);
        b.extend((self.files.len() as u32).to_le_bytes());
        for f in &self.files {
            require(!f.relative.is_empty() && f.relative.len() <= 4096)?;
            b.extend((f.relative.len() as u16).to_le_bytes());
            b.extend(f.relative.as_bytes());
            b.extend(f.bytes.to_le_bytes());
            b.extend(f.sha256);
        }
        require(b.len() <= 8 * 1024 * 1024)?;
        Ok(b)
    }
    fn decode(mut b: &[u8]) -> Result<Self> {
        require(take::<8>(&mut b)? == *b"FCREADY1")?;
        let database_bytes = u64::from_le_bytes(take(&mut b)?);
        let database_sha256 = take(&mut b)?;
        let count = u32::from_le_bytes(take(&mut b)?) as usize;
        require(count <= MAX_BACKUP_ENTRIES)?;
        let mut files = Vec::with_capacity(count);
        for _ in 0..count {
            let len = u16::from_le_bytes(take(&mut b)?) as usize;
            require(len > 0 && len <= 4096 && b.len() >= len)?;
            let relative = checked(std::str::from_utf8(&b[..len]))?.to_owned();
            b = &b[len..];
            let bytes = u64::from_le_bytes(take(&mut b)?);
            let sha256 = take(&mut b)?;
            files.push(FileFact {
                relative,
                bytes,
                sha256,
            });
        }
        require(b.is_empty())?;
        Ok(Self {
            database_bytes,
            database_sha256,
            files,
        })
    }
}

fn read_small(path: &Path, max: u64) -> Result<Vec<u8>> {
    let mut pin = checked(LockedFile::open(path, max))?;
    let mut bytes = Vec::new();
    checked(pin.file_mut().read_to_end(&mut bytes))?;
    Ok(bytes)
}
fn exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Error::Unavailable),
    }
}
fn write_new(
    paths: &DataPaths,
    files: &ControlledFiles,
    relative: &str,
    bytes: &[u8],
) -> Result<()> {
    let path = paths.root().join(relative);
    let mut output = checked(files.create_new(&path))?;
    checked(output.file_mut().write_all(bytes))?;
    checked(output.finish())?;
    checked(paths.protect_migration_object(&path, false))
}
fn digest(pin: &mut LockedFile, check: &dyn Fn() -> bool) -> Result<(u64, [u8; 32])> {
    let bytes = checked(pin.len())?;
    checked(pin.file_mut().seek(SeekFrom::Start(0)))?;
    let mut hash = Sha256Stream::default();
    let mut block = [0; 32768];
    let mut total = 0u64;
    loop {
        active(check)?;
        let n = checked(pin.file_mut().read(&mut block))?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(Error::InvalidDeployment)?;
        require(total <= bytes)?;
        hash.update(&block[..n]);
    }
    require(total == bytes)?;
    Ok((total, hash.finish()))
}
fn mkdir(paths: &DataPaths, files: &ControlledFiles, path: &Path) -> Result<()> {
    if !exists(path)? {
        checked(files.create_directory_new(path))?;
        checked(paths.protect_migration_object(path, true))?;
    } else {
        checked(PinnedDirectory::open(path))?;
    }
    Ok(())
}
fn copy_tree(
    source: &ControlledFiles,
    target: &DataPaths,
    output: &ControlledFiles,
    relative: &Path,
    ledger: &mut Vec<FileFact>,
    total: &mut u64,
    check: &dyn Fn() -> bool,
) -> Result<()> {
    active(check)?;
    require(relative.components().count() <= 16)?;
    let original = source.root().join(relative);
    let _pin = checked(PinnedDirectory::open(&original))?;
    let destination = target.root().join(relative);
    mkdir(target, output, &destination)?;
    for child in checked(source.entries(&original, MAX_BACKUP_ENTRIES))? {
        active(check)?;
        let rel = checked(child.strip_prefix(source.root()))?;
        let metadata = checked(std::fs::symlink_metadata(&child))?;
        if metadata.is_dir() {
            copy_tree(source, target, output, rel, ledger, total, check)?;
            continue;
        }
        require(ledger.len() < MAX_BACKUP_ENTRIES)?;
        let mut file = checked(LockedFile::open(&child, MAX_DATABASE_BYTES))?;
        let bytes = checked(file.len())?;
        *total = total.checked_add(bytes).ok_or(Error::InvalidDeployment)?;
        require(*total <= MAX_MEDIA_BYTES)?;
        let destination = target.root().join(rel);
        let mut created = checked(output.create_new(&destination))?;
        let mut hash = Sha256Stream::default();
        let mut block = [0; 32768];
        let mut copied = 0u64;
        loop {
            active(check)?;
            let n = checked(file.file_mut().read(&mut block))?;
            if n == 0 {
                break;
            }
            copied += n as u64;
            require(copied <= bytes)?;
            checked(created.file_mut().write_all(&block[..n]))?;
            hash.update(&block[..n]);
        }
        require(copied == bytes)?;
        checked(created.finish())?;
        checked(target.protect_migration_object(&destination, false))?;
        ledger.push(FileFact {
            relative: rel
                .to_str()
                .ok_or(Error::InvalidDeployment)?
                .replace('\\', "/"),
            bytes,
            sha256: hash.finish(),
        });
    }
    Ok(())
}
fn validate_source_layout(source: &DataPaths) -> Result<()> {
    let files = checked(ControlledFiles::new(source.root()))?;
    for name in ["config", "state", "runtime"] {
        checked(PinnedDirectory::open(&source.root().join(name)))?;
    }
    // The caller already verified the original lock file and now retains its
    // share-mode-zero InstanceGuard. Reopening that file would reject our lock.
    let mut count = 0;
    for entry in checked(std::fs::read_dir(source.root()))? {
        count += 1;
        require(count <= NAMESPACES.len())?;
        let path = checked(entry)?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(Error::InvalidDeployment)?;
        require(NAMESPACES.contains(&name))?;
        checked(PinnedDirectory::open(&path))?;
        if matches!(name, "recovery" | "reinitialization") {
            let entries = checked(files.entries(&path, 1))?;
            let marker = path.join(".future-classroom-owner");
            if !entries.is_empty() {
                let owner: &[u8] = if name == "recovery" {
                    b"FUTURE_CLASSROOM_RECOVERY_FILES_V1\n"
                } else {
                    b"FUTURE_CLASSROOM_REINITIALIZATION_V1\n"
                };
                if entries != [marker.clone()] || read_small(&marker, 128)? != owner {
                    return Err(Error::Interrupted);
                }
            }
        }
    }
    for (folder, allowed) in [
        ("config", &["credential-key.dpapi", "deployment.marker"][..]),
        (
            "state",
            &[
                "classroom.sqlite3",
                "classroom.sqlite3-wal",
                "classroom.sqlite3-shm",
            ][..],
        ),
    ] {
        for path in checked(files.entries(&source.root().join(folder), allowed.len()))? {
            require(
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| allowed.contains(&n)),
            )?;
            checked(OwnedObjectPin::file_allow_empty(&path))?;
        }
    }
    Ok(())
}
fn fresh_target(paths: &DataPaths) -> Result<()> {
    let files = checked(ControlledFiles::new(paths.root()))?;
    for entry in checked(std::fs::read_dir(paths.root()))? {
        let path = checked(entry)?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(Error::InvalidDeployment)?;
        require(NAMESPACES.contains(&name))?;
        checked(PinnedDirectory::open(&path))?;
        let names = checked(files.entries(&path, 4))?;
        if name == "runtime" {
            require(
                names
                    .iter()
                    .all(|p| p.file_name().is_some_and(|n| n == "instance.lock")),
            )?;
        } else {
            require(names.is_empty())?;
        }
    }
    Ok(())
}
fn verify_ready(target: &DataPaths, ready: &Ready, check: &dyn Fn() -> bool) -> Result<()> {
    require(
        ready.database_bytes > 0
            && ready.database_bytes <= MAX_DATABASE_BYTES
            && ready.files.len() <= MAX_BACKUP_ENTRIES,
    )?;
    let mut db = checked(LockedFile::open(&target.database(), MAX_DATABASE_BYTES))?;
    require(digest(&mut db, check)? == (ready.database_bytes, ready.database_sha256))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0u64;
    for fact in &ready.files {
        let path = Path::new(&fact.relative);
        require(
            path.components().all(|p| matches!(p, Component::Normal(_)))
                && seen.insert(&fact.relative),
        )?;
        total = total
            .checked_add(fact.bytes)
            .ok_or(Error::InvalidDeployment)?;
        require(total <= MAX_MEDIA_BYTES)?;
        let mut file = checked(LockedFile::open(
            &target.root().join(path),
            MAX_DATABASE_BYTES,
        ))?;
        require(digest(&mut file, check)? == (fact.bytes, fact.sha256))?;
    }
    Ok(())
}
fn publish_marker(target: &DataPaths, files: &ControlledFiles) -> Result<()> {
    let temporary = target
        .root()
        .join("runtime/local-data-migration.marker.tmp");
    let mut file = checked(files.create_new(&temporary))?;
    checked(file.file_mut().write_all(MARKER))?;
    let file = checked(file.seal())?;
    drop(checked(
        file.publish(files, &target.root().join(DEPLOYMENT)),
    )?);
    checked(target.protect_migration_object(&target.root().join(DEPLOYMENT), false))
}

/// Called on Work::Open after the first frame, while the GUI's original target
/// InstanceGuard remains held, before fresh initialization/cold recovery/writer.
pub fn migrate_before_open(target: &DataPaths, check: &dyn Fn() -> bool) -> Result<()> {
    let canonical = checked(fc_windows::program_data())?;
    if target.root() != canonical && !target.requires_existing_deployment() {
        return Ok(());
    }
    let Some(source) = checked(DataPaths::local_migration_source())? else {
        return Ok(());
    };
    if target.root() != canonical || target.requires_existing_deployment() {
        return Err(Error::ConcurrentRoots);
    }
    migrate_known(&source, target, check)
}

fn migrate_known(source: &DataPaths, target: &DataPaths, check: &dyn Fn() -> bool) -> Result<()> {
    active(check)?;
    checked(target.verify_root_for_current_user())?;
    let was_retired = checked(source.local_migration_retired())?;
    // The exact original instance.lock is acquired before opening SQLite. Old
    // instances are refused, never killed, signalled or migrated underneath.
    let _source_runtime = checked(PinnedDirectory::open(&source.runtime()))?;
    checked(OwnedObjectPin::file_allow_empty(
        &source.runtime().join("instance.lock"),
    ))?;
    let _source_instance = checked(InstanceGuard::acquire(source.root()))?;
    let source_pin = checked(PinnedDirectory::open(source.root()))?;
    let target_pin = checked(PinnedDirectory::open(target.root()))?;
    let files = checked(ControlledFiles::new(target.root()))?;
    let journal_path = target.root().join(JOURNAL);
    let journal = if exists(&journal_path)? {
        let journal = Journal::decode(&read_small(&journal_path, 4096)?)?;
        require(
            journal.user_sid == checked(current_user_sid())?
                && journal.source == checked(source_pin.identity())?
                && journal.target == checked(target_pin.identity())?,
        )?;
        if !was_retired {
            return Err(Error::Interrupted);
        }
        if exists(&target.root().join(DONE))? {
            require(read_small(&target.root().join(DONE), 128)? == MARKER)?;
            // A completed relocation chooses ProgramData. Later password/key
            // rotation or an explicitly authorized reset is validated by normal
            // startup, not compared forever with this retired source backup.
            return Ok(());
        }
        Some(journal)
    } else {
        None
    };
    validate_source_layout(source)?;
    require(read_small(&source.root().join(DEPLOYMENT), 128)? == MARKER)?;
    let encrypted_key = read_small(&source.root().join(KEY), 4096)?;
    let secret = checked(CurrentUserDpapi::default().unprotect(&encrypted_key))?;
    let key: [u8; 32] = checked(secret.expose().try_into())?;
    let _codec = checked(CredentialRecordCodec::from_key(key))?;
    let source_files = checked(ControlledFiles::new(source.root()))?;
    if let Some(journal) = journal {
        require(journal.key_sha256 == fc_security::sha256(&encrypted_key))?;
        require(read_small(&target.root().join(KEY), 4096)? == encrypted_key)?;
        let facts = checked(fc_storage::inspect_local_deployment(&target.database()))?;
        require(facts.school_id.get() == journal.school)?;
        let ready = Ready::decode(&read_small(&target.root().join(READY), 8 * 1024 * 1024)?)?;
        verify_ready(target, &ready, check)?;
        if exists(&target.root().join(DEPLOYMENT))? {
            require(read_small(&target.root().join(DEPLOYMENT), 128)? == MARKER)?;
            checked(target.protect_migration_object(&target.root().join(DEPLOYMENT), false))?;
        } else {
            publish_marker(target, &files)?;
        }
        write_new(target, &files, DONE, MARKER)?;
        return Ok(());
    }
    if was_retired || exists(&target.root().join(DEPLOYMENT))? {
        return Err(Error::ConcurrentRoots);
    }
    fresh_target(target)?;
    let _source_db = checked(OwnedObjectPin::file_allow_empty(&source.database()))?;
    let facts = checked(fc_storage::inspect_local_deployment(&source.database()))?;
    let journal = Journal {
        user_sid: checked(current_user_sid())?,
        source: checked(source_pin.identity())?,
        target: checked(target_pin.identity())?,
        school: facts.school_id.get(),
        key_sha256: fc_security::sha256(&encrypted_key),
    };
    write_new(target, &files, JOURNAL, &journal.encode())?;
    // A failure from here leaves this exact journal and every source byte intact;
    // startup refuses initialization instead of guessing which school to open.
    checked(fc_storage::snapshot_local_deployment(
        &source.database(),
        &target.database(),
        check,
    ))?;
    checked(target.protect_migration_object(&target.database(), false))?;
    write_new(target, &files, KEY, &encrypted_key)?;
    let mut ledger = vec![FileFact {
        relative: KEY.into(),
        bytes: encrypted_key.len() as u64,
        sha256: fc_security::sha256(&encrypted_key),
    }];
    let mut total = encrypted_key.len() as u64;
    for name in TREES {
        if exists(&source.root().join(name))? {
            copy_tree(
                &source_files,
                target,
                &files,
                Path::new(name),
                &mut ledger,
                &mut total,
                check,
            )?;
        }
    }
    let moved = checked(fc_storage::inspect_local_deployment(&target.database()))?;
    require(moved == facts)?;
    // Reuse the authoritative repository's typed inventory and existing path
    // adapter. No directory guessing substitutes for each indexed hash/length.
    for media in &moved.media {
        let rel = crate::recovery_files::entry_relative_path(media);
        let mut file = checked(LockedFile::open(
            &target.root().join(rel),
            MAX_DATABASE_BYTES,
        ))?;
        require(digest(&mut file, check)? == (media.bytes(), media.sha256()))?;
    }
    checked(checked(target.prepare_for_current_user())?.read_back())?;
    let mut database = checked(LockedFile::open(&target.database(), MAX_DATABASE_BYTES))?;
    let (database_bytes, database_sha256) = digest(&mut database, check)?;
    drop(database);
    let ready = Ready {
        database_bytes,
        database_sha256,
        files: ledger,
    };
    verify_ready(target, &ready, check)?;
    write_new(target, &files, READY, &ready.encode()?)?;
    active(check)?;
    // Retirement changes only the original project's known root ACL, after the
    // full target is verified. Known old startup now rejects that exact root.
    checked(source.set_local_migration_retired(true))?;
    if let Err(error) = publish_marker(target, &files) {
        // Restore only the known original DACL when publication is definitively
        // absent. Unknown publication retains retirement + the durable journal.
        if matches!(exists(&target.root().join(DEPLOYMENT)), Ok(false)) {
            checked(source.set_local_migration_retired(false))?;
        }
        return Err(error);
    }
    write_new(target, &files, DONE, MARKER)
}

/// Durable relocation evidence must never turn missing target data into fresh
/// setup. An explicit verified reinitialization permit still takes precedence.
pub fn has_deployment_receipt(paths: &DataPaths) -> Result<bool> {
    exists(&paths.root().join(JOURNAL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery::test_support::{password, Fixture};
    use fc_application::identity::SessionPurpose;
    use fc_domain::{
        identity::IdentityCommand, ids::*, schedule::*, schedule_document::*, time::*,
    };
    use fc_ports::command_window::CommandFamily;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Roots {
        source: DataPaths,
        target: DataPaths,
    }
    impl Drop for Roots {
        fn drop(&mut self) {
            let restored = self
                .source
                .isolated_migration_source()
                .and_then(|source| source.set_local_migration_retired(false));
            let source = std::fs::remove_dir_all(self.source.root());
            let target = std::fs::remove_dir_all(self.target.root());
            if !std::thread::panicking() {
                restored.unwrap();
                source.unwrap();
                target.unwrap();
            }
        }
    }
    #[test]
    fn isolated_full_relocation_keeps_credentials_timetable_original_and_completed_reopen() {
        // Existing production-crypto/backend fixture; every path is a fresh
        // FutureClassroom-Test child of temp, never either real KnownFolder.
        let fixture = Fixture::new();
        let mut backend = fixture.backend.lock().unwrap();
        backend
            .test_native_command(
                CommandFamily::Identity,
                Some(fixture.lease),
                SessionPurpose::Manage,
                |b| {
                    b.identity(
                        fixture.lease,
                        IdentityCommand::CreateClass {
                            id: ClassId::new(1).unwrap(),
                            name: "合成迁移班级".into(),
                        },
                    )
                },
            )
            .unwrap();
        let mut document = backend.read_schedule(fixture.lease).unwrap();
        document.id_high_water = 13;
        document.semesters = vec![SemesterPlan {
            semester: Semester {
                id: SemesterId::new(10).unwrap(),
                class_id: ClassId::new(1).unwrap(),
                revision: Revision::new(1),
                enabled: true,
                start: LocalDate::new(2026, 10, 26).unwrap(),
                end: LocalDate::new(2026, 11, 30).unwrap(),
                first_week_monday: LocalDate::new(2026, 10, 26).unwrap(),
                tables: vec![WeekTable {
                    id: TableId::new(11).unwrap(),
                    role: TableRole::Single,
                    rows: vec![TimetableRow {
                        id: RowId::new(12).unwrap(),
                        logical_key: LogicalRowKey::new(13).unwrap(),
                        group: DayGroup::Morning,
                        kind: RowKind::Activity,
                        range: MinuteRange::new(
                            Minute::new(480).unwrap(),
                            Minute::new(540).unwrap(),
                        )
                        .unwrap(),
                        enabled_days: [true; 7],
                        courses: std::array::from_fn(|_| None),
                    }],
                }],
            },
            name: "合成学期".into(),
            timing_sync: false,
            dual_history: None,
            row_details: vec![RowDetails {
                row_id: RowId::new(12).unwrap(),
                name: "合成活动".into(),
            }],
            cell_details: vec![],
        }];
        let expected = backend
            .test_native_command(
                CommandFamily::Schedule,
                Some(fixture.lease),
                SessionPurpose::Manage,
                |b| b.save_schedule(fixture.lease, document.revision, document),
            )
            .unwrap();
        backend.close().unwrap();
        drop(backend);
        let source = DataPaths::isolated_test(fixture.root.with_file_name(format!(
            "{}-LocalSource",
            fixture.root.file_name().unwrap().to_str().unwrap()
        )))
        .unwrap();
        let target = DataPaths::isolated_test(fixture.root.with_file_name(format!(
            "{}-LocalTarget",
            fixture.root.file_name().unwrap().to_str().unwrap()
        )))
        .unwrap();
        source
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        target
            .prepare_for_current_user()
            .unwrap()
            .read_back()
            .unwrap();
        let roots = Roots { source, target };
        drop(InstanceGuard::acquire(roots.source.root()).unwrap());
        let target_instance = InstanceGuard::acquire(roots.target.root()).unwrap();
        // Fixture preparation copies an already closed synthetic DB. The real
        // relocation below itself uses the production SQLite Backup loop.
        std::fs::copy(
            fixture.root.join("state/classroom.sqlite3"),
            roots.source.database(),
        )
        .unwrap();
        let source_files = ControlledFiles::new(roots.source.root()).unwrap();
        let secret = fc_windows::system::SecretBytes::new(vec![119; 32]).unwrap();
        let encrypted = CurrentUserDpapi::default().protect(&secret).unwrap();
        write_new(&roots.source, &source_files, KEY, &encrypted).unwrap();
        write_new(&roots.source, &source_files, DEPLOYMENT, MARKER).unwrap();
        // Completed backup/reset operations leave their recognized owner file;
        // it is not an active operation and must not block relocation.
        for (name, owner) in [
            ("recovery", &b"FUTURE_CLASSROOM_RECOVERY_FILES_V1\n"[..]),
            (
                "reinitialization",
                &b"FUTURE_CLASSROOM_REINITIALIZATION_V1\n"[..],
            ),
        ] {
            mkdir(
                &roots.source,
                &source_files,
                &roots.source.root().join(name),
            )
            .unwrap();
            write_new(
                &roots.source,
                &source_files,
                &format!("{name}/.future-classroom-owner"),
                owner,
            )
            .unwrap();
        }
        let original = Path::new("teachers/1/subjects/2/3/4.txt");
        std::fs::create_dir_all(roots.source.root().join(original.parent().unwrap())).unwrap();
        std::fs::write(
            roots.source.root().join(original),
            b"synthetic retained original",
        )
        .unwrap();
        let selected_source = roots.source.isolated_migration_source().unwrap();
        let saw_unpublished = AtomicBool::new(false);
        migrate_known(&selected_source, &roots.target, &|| {
            if roots.target.database().exists() {
                assert!(!roots.target.root().join(DEPLOYMENT).exists());
                saw_unpublished.store(true, Ordering::Relaxed);
            }
            true
        })
        .unwrap();
        assert!(saw_unpublished.load(Ordering::Relaxed));
        assert_eq!(
            read_small(&roots.target.root().join(DEPLOYMENT), 128).unwrap(),
            MARKER
        );
        assert_eq!(
            std::fs::read(roots.target.root().join(original)).unwrap(),
            b"synthetic retained original"
        );
        let mut reopened = fixture.reopen(&roots.target.database());
        let lease = reopened.login("admin", password()).unwrap();
        assert_eq!(reopened.read_schedule(lease).unwrap(), expected);
        reopened.close().unwrap();
        // Root retirement is an application contract, not recursive ACL magic.
        // A stale/corrupted old backup key must not gate a completed target.
        std::fs::write(
            roots.source.root().join(KEY),
            b"stale synthetic retired key",
        )
        .unwrap();
        migrate_known(&selected_source, &roots.target, &|| true).unwrap();
        let mut reopened = fixture.reopen(&roots.target.database());
        reopened.login("admin", password()).unwrap();
        reopened.close().unwrap();
        drop(target_instance);
        drop(roots);
    }
}
