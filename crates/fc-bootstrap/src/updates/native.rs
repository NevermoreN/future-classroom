//! Windows composition on the existing shared worker. The network loop only
//! exchanges bounded records; this owner retains every file until I/O returns.
use super::*;
use crate::{
    backend::Backend,
    update_network::*,
    update_packages::{
        self as packages, CheckpointProtection, OwnedMetadata, PackageError, TrustedCatalog,
    },
};
use fc_domain::{https_settings::*, update_state::*, updates::*};
use fc_ports::RandomSource;
use fc_protocol::update_release::ReleaseClaims;
use fc_security::update_group::UpdateGroupKey;
use fc_windows::{
    installer_lifecycle::{self, InstallIdentity},
    system::{CurrentUserDpapi, SecretBytes},
    update_cache::{self, UpdateCache, VerifiedTarget},
    update_install,
};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ROOT: &[u8] = include_bytes!("../../../../packaging/update-root.json");
const MAX_IMPORT: u64 = 128 * 1024 * 1024;
pub enum UpdateTick {
    Idle,
    Handoff(update_install::PreparedHandoff),
}
struct Upload {
    token: String,
    path: PathBuf,
    file: Option<File>,
    length: u64,
    received: u64,
    queued: bool,
    touched: Instant,
    _reservation: update_cache::ExternalReservation,
}
enum Handle {
    Upload(update_cache::BlobLease),
    Download {
        target: VerifiedTarget,
        download: update_cache::Download,
    },
}
struct Candidate {
    claims: ReleaseClaims,
    target: VerifiedTarget,
    metadata: OwnedMetadata,
    ready: bool,
}
pub(super) struct Native {
    root: PathBuf,
    cache: UpdateCache,
    node: [u8; 16],
    upload: Option<Upload>,
    handles: BTreeMap<u64, Handle>,
    next_handle: u64,
    policy: Option<UpdatePolicy>,
    candidate: Option<Candidate>,
    transport: Option<(HttpsConfiguration, [u8; 32])>,
    handoff: bool,
    restored: bool,
    recovered_staging: bool,
    validated_stage: Option<[u8; 16]>,
    firewall: Option<(bool, String)>,
    stages: BTreeMap<PathBuf, update_cache::ExternalReservation>,
}
fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    fc_windows::SystemRandom
        .fill(&mut bytes)
        .map_err(|_| "随机源不可用。")?;
    Ok(bytes)
}
fn err(e: impl std::fmt::Debug) -> String {
    format!("更新操作未完成：{e:?}")
}
fn checked_dir(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    if !path.try_exists().map_err(err)? {
        fs::create_dir(path).map_err(err)?;
    }
    let m = fs::symlink_metadata(path).map_err(err)?;
    if !m.is_dir() || m.file_attributes() & 0x400 != 0 {
        return Err("更新目录不安全。".into());
    }
    Ok(())
}
fn read_small(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x00200000)
        .open(path)
        .map_err(err)?;
    let m = file.metadata().map_err(err)?;
    if !m.is_file() || m.len() > max || m.file_attributes() & 0x400 != 0 {
        return Err("更新记录无效。".into());
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(err)?;
    Ok(bytes)
}
fn protected_write(path: &Path, bytes: Vec<u8>) -> Result<(), String> {
    let data = CurrentUserDpapi::default()
        .protect(&SecretBytes::new(bytes).map_err(err)?)
        .map_err(err)?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(err)?;
    f.write_all(&data).map_err(err)?;
    f.sync_all().map_err(err)
}
fn secret(path: &Path) -> Result<SecretBytes, String> {
    CurrentUserDpapi::default()
        .unprotect(&read_small(path, 70 * 1024)?)
        .map_err(err)
}
struct Protection;
impl CheckpointProtection for Protection {
    fn protect(&self, digest: &[u8; 32]) -> Result<Vec<u8>, PackageError> {
        CurrentUserDpapi::default()
            .protect(&SecretBytes::new(digest.to_vec()).map_err(|_| PackageError::Checkpoint)?)
            .map_err(|_| PackageError::Checkpoint)
    }
    fn unprotect(&self, data: &[u8]) -> Result<[u8; 32], PackageError> {
        let value = CurrentUserDpapi::default()
            .unprotect(data)
            .map_err(|_| PackageError::Checkpoint)?;
        value
            .expose()
            .try_into()
            .map_err(|_| PackageError::Checkpoint)
    }
}
fn now() -> Result<i64, String> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(err)?
            .as_secs(),
    )
    .map_err(err)
}
impl UpdateControl {
    pub fn open() -> Result<Self, String> {
        Self::open_with_state(&UpdateMaintenanceState::default())
    }
    pub fn open_with_state(state: &UpdateMaintenanceState) -> Result<Self, String> {
        let root = InstallIdentity::for_current_user()
            .map_err(err)?
            .update_directory()
            .map_err(err)?
            .join("distribution");
        checked_dir(&root)?;
        for name in ["cache", "checkpoints", "staging"] {
            checked_dir(&root.join(name))?;
        }
        let node_path = root.join("node");
        let node = if node_path.try_exists().map_err(err)? {
            secret(&node_path)?
                .expose()
                .try_into()
                .map_err(|_| "节点记录无效。")?
        } else {
            let node = random::<16>()?;
            protected_write(&node_path, node.to_vec())?;
            node
        };
        let mut protected = Vec::new();
        if let Some(installed) = state.installed_release {
            protected.push(installed.payload_sha256);
        }
        if let Some(previous) = state
            .jobs
            .iter()
            .rev()
            .filter(|job| job.phase == UpdateJobPhase::Succeeded)
            .nth(1)
        {
            protected.push(previous.release.payload_sha256);
        }
        let cache = UpdateCache::open(&root.join("cache"), &protected).map_err(err)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(State {
                status: UpdateStatus {
                    version: env!("CARGO_PKG_VERSION").into(),
                    automatic_updates: state.policy.enabled
                        && state.policy.download
                        && state.policy.install_mode == UpdateInstallMode::AutomaticAfterDownload,
                    update_available: false,
                    update_message: if !state.policy.enabled {
                        "自动更新已关闭。"
                    } else if state.policy.group_id.is_some() {
                        "正在启动更新服务。"
                    } else {
                        "正在启动默认局域网更新服务。"
                    }
                    .into(),
                    action_token: hex(&random::<32>()?),
                    phase: if !state.policy.enabled {
                        "disabled"
                    } else {
                        "starting"
                    }
                    .into(),
                    received_bytes: 0,
                    total_bytes: 0,
                    group_id: state.policy.group_id.map(|group| hex(&group)),
                },
                check: true,
            })),
            native: Arc::new(Mutex::new(Native {
                root,
                cache,
                node,
                upload: None,
                handles: BTreeMap::new(),
                next_handle: 0,
                policy: None,
                candidate: None,
                transport: None,
                handoff: false,
                restored: false,
                recovered_staging: false,
                validated_stage: None,
                firewall: None,
                stages: BTreeMap::new(),
            })),
            network: NetworkControl::new(),
        })
    }
    pub fn network(&self) -> NetworkControl {
        self.network.clone()
    }
    pub fn has_network_work(&self) -> bool {
        self.network.has_work()
    }
    pub fn prepare_group_create(&self) -> Result<GroupEnrollment, String> {
        self.prepare_group(random::<16>()?, random::<32>()?)
    }
    pub fn prepare_group_join(&self, text: &str) -> Result<GroupEnrollment, String> {
        let mut parts = text.trim().split(':');
        if parts.next() != Some("fc-update-group-v1") {
            return Err("学校更新组代码格式不正确。".into());
        }
        let group = unhex(parts.next().ok_or("缺少学校组编号。")?)?;
        let key = unhex(parts.next().ok_or("缺少学校组密钥。")?)?;
        if parts.next().is_some() {
            return Err("学校更新组代码格式不正确。".into());
        }
        self.prepare_group(group, key)
    }
    fn prepare_group(&self, group: [u8; 16], key: [u8; 32]) -> Result<GroupEnrollment, String> {
        if group == [0; 16] || key == [0; 32] {
            return Err("学校更新组代码无效。".into());
        }
        let native = self.native.lock().map_err(err)?;
        let token = random::<16>()?;
        let mut bytes = Vec::from(group);
        bytes.extend_from_slice(&key);
        protected_write(&native.root.join(format!("{}.pending", hex(&token))), bytes)?;
        Ok(GroupEnrollment {
            group_id: group,
            token,
        })
    }
    pub fn commit_group(&self, token: [u8; 16]) -> Result<(), String> {
        let native = self.native.lock().map_err(err)?;
        commit_group(&native.root, token)
    }
    pub fn discard_group(&self, token: [u8; 16]) -> Result<(), String> {
        let native = self.native.lock().map_err(err)?;
        let path = native.root.join(format!("{}.pending", hex(&token)));
        if path.try_exists().map_err(err)? {
            fs::remove_file(path).map_err(err)?;
        }
        Ok(())
    }
    pub fn export_group(&self) -> Result<String, String> {
        let native = self.native.lock().map_err(err)?;
        let policy = native.policy.as_ref().ok_or("更新设置尚未就绪。")?;
        let group = policy.group_id.ok_or("尚未加入学校更新组。")?;
        let data = secret(&native.root.join(format!("{}.group", hex(&group))))?;
        if data.expose().len() != 48 || data.expose()[..16] != group {
            return Err("学校更新组记录无效。".into());
        }
        Ok(format!(
            "fc-update-group-v1:{}:{}",
            hex(&group),
            hex(&data.expose()[16..])
        ))
    }
    pub fn begin_import(&self, length: u64) -> Result<String, String> {
        if length == 0 || length > MAX_IMPORT {
            return Err("更新包不能超过128MiB。".into());
        }
        let mut native = self.native.lock().map_err(err)?;
        if !native.restored
            || !native
                .policy
                .as_ref()
                .is_some_and(UpdatePolicy::participation_enabled)
        {
            return Err("请先加入学校更新组，等待更新状态恢复后再导入。".into());
        }
        if native.upload.is_some() {
            return Err("另一个更新包正在导入。".into());
        }
        let reservation = native.cache.reserve_external(length).map_err(err)?;
        let token = hex(&random::<16>()?);
        let path = native
            .root
            .join("staging")
            .join(format!("{token}.fcupdate"));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(err)?;
        native.upload = Some(Upload {
            token: token.clone(),
            path,
            file: Some(file),
            length,
            received: 0,
            queued: false,
            touched: Instant::now(),
            _reservation: reservation,
        });
        Ok(token)
    }
    pub fn append_import(&self, token: &str, offset: u64, bytes: &[u8]) -> Result<(), String> {
        let mut native = self.native.lock().map_err(err)?;
        let upload = native
            .upload
            .as_mut()
            .filter(|u| u.token == token && !u.queued)
            .ok_or("更新导入已失效。")?;
        if bytes.is_empty()
            || bytes.len() > 65536
            || offset != upload.received
            || bytes.len() as u64 > upload.length - upload.received
        {
            return Err("更新包分块或位置不正确。".into());
        }
        upload
            .file
            .as_mut()
            .ok_or("更新导入已关闭。")?
            .write_all(bytes)
            .map_err(err)?;
        upload.received += bytes.len() as u64;
        upload.touched = Instant::now();
        Ok(())
    }
    pub fn finish_import(&self, token: &str) -> Result<(), String> {
        let mut native = self.native.lock().map_err(err)?;
        let upload = native
            .upload
            .as_mut()
            .filter(|u| u.token == token && !u.queued)
            .ok_or("更新导入已失效。")?;
        if upload.received != upload.length {
            return Err("更新包尚未传完。".into());
        }
        upload
            .file
            .as_ref()
            .ok_or("更新导入已关闭。")?
            .sync_all()
            .map_err(err)?;
        drop(upload.file.take());
        upload.queued = true;
        Ok(())
    }
    pub fn cancel_import(&self, token: &str) -> Result<(), String> {
        let mut native = self.native.lock().map_err(err)?;
        if native
            .upload
            .as_ref()
            .is_some_and(|u| u.token == token && !u.queued)
        {
            let upload = native.upload.take().unwrap();
            native.remove_upload(upload)?;
        }
        Ok(())
    }
    pub fn tick(&self, backend: &mut Backend) -> Result<UpdateTick, String> {
        let mut native = self.native.lock().map_err(err)?;
        let result = native.tick(self, backend, &|| false);
        if let Err(ref message) = result {
            let mut state = self.inner.lock().map_err(err)?;
            state.status.phase = "error".into();
            state.status.update_message = message.clone();
        }
        result
    }
    pub fn tick_cancellable(
        &self,
        backend: &mut Backend,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<UpdateTick, String> {
        let mut native = self.native.lock().map_err(err)?;
        let result = native.tick(self, backend, cancelled);
        if let Err(ref message) = result {
            let mut state = self.inner.lock().map_err(err)?;
            state.status.phase = "error".into();
            state.status.update_message = message.clone();
        }
        result
    }
    pub fn abort_handoff(&self, backend: &mut Backend, job_id: [u8; 16]) -> Result<(), String> {
        let mut native = self.native.lock().map_err(err)?;
        update_install::cancel_prepared_handoff(job_id).map_err(err)?;
        let state = backend.update_runtime_state().map_err(err)?;
        if let Some(job) = state
            .jobs
            .iter()
            .find(|j| job_key(j) == job_id && !j.phase.settled())
        {
            if job.phase == UpdateJobPhase::Handoff {
                let state = backend
                    .advance_background_update(job, UpdateJobPhase::NeedsReconcile)
                    .map_err(err)?;
                let job = state.jobs.iter().find(|j| job_key(j) == job_id).unwrap();
                backend
                    .advance_background_update(job, UpdateJobPhase::Failed)
                    .map_err(err)?;
            }
        }
        native.handoff = false;
        native.remove_stage(job_id)?;
        Ok(())
    }
    pub fn retire(&self) -> Result<(), String> {
        self.network.stop();
        let mut native = self.native.lock().map_err(err)?;
        native.handles.clear();
        self.network.confirm_file_owners_returned();
        if let Some(upload) = native.upload.take() {
            native.remove_upload(upload)?;
        }
        Ok(())
    }
}
fn commit_group(root: &Path, token: [u8; 16]) -> Result<(), String> {
    let path = root.join(format!("{}.pending", hex(&token)));
    let data = secret(&path)?;
    if data.expose().len() != 48 {
        return Err("待确认的学校组记录无效。".into());
    }
    let destination = root.join(format!("{}.group", hex(&data.expose()[..16])));
    if destination.try_exists().map_err(err)? {
        if secret(&destination)?.expose() != data.expose() {
            return Err("学校组记录冲突。".into());
        }
        fs::remove_file(path).map_err(err)?;
    } else {
        fs::rename(path, destination).map_err(err)?;
    }
    Ok(())
}
fn restore_private_group(root: &Path, group: [u8; 16]) -> Result<UpdateGroupKey, String> {
    let path = root.join(format!("{}.group", hex(&group)));
    // A DB commit can succeed just before a power loss during group-file
    // promotion. Only the DB's already-authorized group may be recovered.
    if !path.try_exists().map_err(err)? {
        for entry in fs::read_dir(root).map_err(err)?.take(128) {
            let entry = entry.map_err(err)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(token) = name
                .strip_suffix(".pending")
                .and_then(|s| unhex::<16>(s).ok())
            {
                let pending = secret(&entry.path())?;
                if pending.expose().len() == 48 && pending.expose()[..16] == group {
                    commit_group(root, token)?;
                    break;
                }
            }
        }
    }
    let data = secret(&path)?;
    if data.expose().len() != 48 || data.expose()[..16] != group {
        return Err("学校更新组密钥不可用，请重新加入。".into());
    }
    UpdateGroupKey::new(group, data.expose()[16..].try_into().unwrap())
        .ok_or_else(|| "学校组密钥无效。".into())
}
fn job_key(job: &UpdateJob) -> [u8; 16] {
    let mut out = [0; 16];
    out[..8].copy_from_slice(&job.id.to_le_bytes());
    out[8..].copy_from_slice(&job.policy_revision.get().to_le_bytes());
    out
}
fn hash_file(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<[u8; 32], String> {
    let mut file = File::open(path).map_err(err)?;
    let mut hash = fc_security::Sha256Stream::default();
    let mut buf = [0; 65536];
    loop {
        if cancelled() {
            return Err("更新任务已取消。".into());
        }
        let n = file.read(&mut buf).map_err(err)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finish())
}
impl Native {
    fn notice(&self, control: &UpdateControl, phase: &str, message: &str) -> Result<(), String> {
        let mut state = control.inner.lock().map_err(err)?;
        state.status.phase = phase.into();
        state.status.update_message = if let Some((false, reason)) = &self.firewall {
            if self.policy.as_ref().is_some_and(|p| p.share) {
                format!("{message}；本机共享未开启：{reason}")
            } else {
                message.into()
            }
        } else {
            message.into()
        };
        Ok(())
    }
    fn configure(
        &mut self,
        control: &UpdateControl,
        policy: &UpdatePolicy,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        if self.policy.as_ref() == Some(policy) {
            return Ok(());
        }
        // A failed private-group restore must not retain the previous public
        // LAN configuration (or another private group's configuration).
        control.network.set_configuration(None)?;
        control.network.publish(None)?;
        self.candidate = None;
        self.restored = false;
        {
            let mut state = control.inner.lock().map_err(err)?;
            state.status.automatic_updates = policy.enabled
                && policy.download
                && policy.install_mode == UpdateInstallMode::AutomaticAfterDownload;
            state.status.group_id = policy.group_id.map(|g| hex(&g));
        }
        if !policy.participation_enabled() {
            if let Some(upload) = self.upload.take() {
                self.remove_upload(upload)?;
            }
            self.policy = Some(policy.clone());
            return self.notice(control, "disabled", "自动更新已关闭。");
        }
        if policy.share {
            let status = fc_windows::system::prepare_update_firewall();
            self.firewall = Some((status.can_share(), status.message().to_string()));
        }
        let auth = Arc::new(match policy.group_id {
            Some(group) => restore_private_group(&self.root, group)?,
            None => UpdateGroupKey::public_lan(),
        });
        if self.transport.is_none() {
            use fc_windows::tls_frontdoor::{
                certificate_der_sha256, certificate_provision as certificates,
                CertificateThumbprint,
            };
            let hosts = certificates::local_certificate_hosts().map_err(err)?;
            let host = hosts
                .iter()
                .find(|s| {
                    s.parse::<std::net::Ipv4Addr>()
                        .is_ok_and(|ip| ip.is_private())
                })
                .ok_or("未找到校园网IPv4地址。")?;
            let certificate =
                certificates::provision_ip_certificate(host, || !cancelled()).map_err(err)?;
            let pin = certificate_der_sha256(
                CertificateThumbprint::parse(&certificate.thumbprint).map_err(err)?,
            )
            .map_err(err)?;
            let listener = HttpsConfiguration::new(
                HttpsHost::parse(host).map_err(err)?,
                8445,
                HttpsCertificateRef::parse(&certificate.thumbprint).map_err(err)?,
            )
            .map_err(err)?
            .with_source_networks(
                HttpsSourceNetworks::parse("10.0.0.0/8\n172.16.0.0/12\n192.168.0.0/16")
                    .map_err(err)?,
            );
            self.transport = Some((listener, pin));
        }
        let (listener, pin) = self.transport.as_ref().unwrap();
        control
            .network
            .set_configuration(Some(NetworkConfiguration {
                auth,
                node: self.node,
                listener: listener.clone(),
                certificate_sha256: *pin,
                seeds: Vec::new(),
                download: policy.download,
                share: policy.share && self.firewall.as_ref().is_some_and(|(allowed, _)| *allowed),
            }))?;
        self.policy = Some(policy.clone());
        self.notice(control, "discovering", "正在查找学校内网中的可信更新。")
    }
    fn remove_upload(&mut self, mut upload: Upload) -> Result<(), String> {
        drop(upload.file.take());
        let path = upload.path;
        self.stages.insert(path.clone(), upload._reservation);
        fs::remove_file(&path).map_err(err)?;
        self.stages.remove(&path);
        Ok(())
    }
    fn recover_staging(
        &mut self,
        state: &UpdateMaintenanceState,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        use std::os::windows::fs::MetadataExt;
        for entry in fs::read_dir(self.root.join("staging")).map_err(err)? {
            if cancelled() {
                return Err("更新恢复已取消。".into());
            }
            let entry = entry.map_err(err)?;
            let path = entry.path();
            if self
                .upload
                .as_ref()
                .is_some_and(|upload| upload.path == path)
            {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let key = unhex::<16>(name).ok();
            let temporary = name
                .strip_suffix(".fcupdate")
                .or_else(|| name.strip_prefix("share-"))
                .or_else(|| name.strip_prefix("verify-"))
                .is_some_and(|token| unhex::<16>(token).is_ok());
            if key.is_none() && !temporary {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(err)?;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("更新临时目录存在别名，已暂停清理。".into());
            }
            if let Some(key) = key {
                if state.jobs.iter().any(|job| {
                    job_key(job) == key
                        && matches!(
                            job.phase,
                            UpdateJobPhase::Handoff
                                | UpdateJobPhase::Installing
                                | UpdateJobPhase::NeedsReconcile
                        )
                }) {
                    self.stages.insert(
                        path.clone(),
                        self.cache.reserve_external(MAX_IMPORT).map_err(err)?,
                    );
                    continue;
                }
            }
            if metadata.is_dir() {
                fs::remove_dir_all(path).map_err(err)?;
            } else if metadata.is_file() {
                fs::remove_file(path).map_err(err)?;
            } else {
                return Err("更新临时文件类型无效。".into());
            }
        }
        Ok(())
    }
    fn tick(
        &mut self,
        control: &UpdateControl,
        backend: &mut Backend,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<UpdateTick, String> {
        if cancelled() {
            return Err("更新任务已取消。".into());
        }
        let mut state = backend.update_runtime_state().map_err(err)?;
        self.reconcile(backend, &state)?;
        state = backend.update_runtime_state().map_err(err)?;
        let previous = state
            .jobs
            .iter()
            .rev()
            .filter(|j| j.phase == UpdateJobPhase::Succeeded)
            .nth(1)
            .map(|j| j.release.payload_sha256);
        self.cache
            .protect_sources(state.installed_release.map(|r| r.payload_sha256), previous)
            .map_err(err)?;
        if !self.recovered_staging {
            self.recover_staging(&state, cancelled)?;
            self.recovered_staging = true;
        }
        self.configure(control, &state.policy, cancelled)?;
        if self.handoff {
            return Ok(UpdateTick::Idle);
        }
        let check = {
            let mut state = control.inner.lock().map_err(err)?;
            std::mem::take(&mut state.check)
        };
        if check {
            control.network.check_now();
        }
        if let Some(work) = control.network.take_work() {
            let id = work.id;
            let result = if cancelled() {
                Err("更新任务已取消。".into())
            } else {
                self.io(work.operation, cancelled)
            };
            control.network.complete_work(NetworkReply { id, result });
        }
        if !state.policy.participation_enabled() {
            return Ok(UpdateTick::Idle);
        }
        if !self.restored {
            if let Some(metadata) =
                TrustedCatalog::restore_metadata(&self.root.join("checkpoints"), &Protection)
                    .map_err(err)?
            {
                if let Err(message) = self.catalog(control, backend, metadata, None, cancelled) {
                    // An old authenticated checkpoint still establishes floors
                    // inside refresh; expired cached roles must not prevent
                    // receiving a fresh signed catalog from the school.
                    self.notice(
                        control,
                        "catalog-unavailable",
                        &format!("缓存目录暂不可复用，等待新的签名目录：{message}"),
                    )?;
                }
            }
            self.restored = true;
        }
        if self
            .upload
            .as_ref()
            .is_some_and(|u| !u.queued && u.touched.elapsed() > Duration::from_secs(600))
        {
            let upload = self.upload.take().unwrap();
            self.remove_upload(upload)?;
        }
        if self.upload.as_ref().is_some_and(|u| u.queued) {
            let upload = self.upload.take().unwrap();
            self.notice(control, "verifying", "正在校验官方更新包、签名和安装源。")?;
            let imported = packages::import_envelope_cancellable(
                &upload.path,
                ROOT,
                &self.root.join("checkpoints"),
                now()?,
                &Protection,
                &self.cache,
                cancelled,
            );
            self.remove_upload(upload)?;
            let imported = imported.map_err(err)?;
            self.catalog(control, backend, imported.metadata, None, cancelled)?;
        }
        while let Some(event) = control.network.take_event() {
            match event {
                NetworkEvent::Metadata { peer, metadata } => {
                    self.catalog(control, backend, metadata, Some(peer), cancelled)?
                }
                NetworkEvent::Downloaded { target } => {
                    if let Some(candidate) = self
                        .candidate
                        .as_mut()
                        .filter(|c| c.target.sha256 == target.sha256)
                    {
                        candidate.ready = true;
                    }
                    self.notice(control, "verifying", "下载完成，正在核对安装资格。")?;
                }
                NetworkEvent::Progress { received, total } => {
                    let mut status = control.inner.lock().map_err(err)?;
                    status.status.received_bytes = received;
                    status.status.total_bytes = total;
                    status.status.phase = "downloading".into();
                }
                NetworkEvent::Status(message) => self.notice(control, "discovering", message)?,
            }
        }
        let Some(candidate) = self.candidate.as_ref().filter(|c| c.ready) else {
            return Ok(UpdateTick::Idle);
        };
        let claims = candidate.claims.clone();
        let target = candidate.target;
        let metadata = candidate.metadata.clone();
        let mut state = backend.update_runtime_state().map_err(err)?;
        if state
            .installed_release
            .is_some_and(|r| r.sequence >= claims.identity.sequence)
        {
            return Ok(UpdateTick::Idle);
        }
        if let Some(previous) = state.jobs.iter().find(|job| !job.phase.settled()).cloned() {
            if matches!(
                previous.phase,
                UpdateJobPhase::Handoff
                    | UpdateJobPhase::Installing
                    | UpdateJobPhase::NeedsReconcile
            ) {
                return Ok(UpdateTick::Idle);
            }
            if previous.policy_revision != state.policy.revision
                || previous.release != claims.identity
            {
                state = backend
                    .advance_background_update(&previous, UpdateJobPhase::Cancelled)
                    .map_err(err)?;
                self.remove_stage(job_key(&previous))?;
            }
        }
        let mut job = if let Some(job) = state.jobs.iter().find(|job| !job.phase.settled()) {
            job.clone()
        } else {
            if state.jobs.iter().any(|job| {
                job.release == claims.identity
                    && (job.phase == UpdateJobPhase::Failed
                        || (job.phase == UpdateJobPhase::Cancelled
                            && job.policy_revision == state.policy.revision))
            }) {
                return self
                    .notice(
                        control,
                        "failed",
                        "此版本的上次更新未完成，已暂停自动重试并保留结果；后续更高版本仍可检查。",
                    )
                    .map(|()| UpdateTick::Idle);
            }
            backend
                .begin_background_update(state.revision, state.policy.revision, claims.identity)
                .map_err(err)?
                .jobs
                .last()
                .unwrap()
                .clone()
        };
        let key = job_key(&job);
        let directory = self.root.join("staging").join(hex(&key));
        if self.validated_stage != Some(key) {
            self.remove_stage(key)?;
            let reservation = self.cache.reserve_external(MAX_IMPORT).map_err(err)?;
            fs::create_dir(&directory).map_err(err)?;
            self.stages.insert(directory.clone(), reservation);
            let result = (|| {
                let mut blob = self
                    .cache
                    .acquire_cancellable(target, cancelled)
                    .map_err(err)?;
                packages::extract_install_sources_cancellable(&mut blob, &directory, cancelled)
                    .map_err(err)?;
                let facts = installer_lifecycle::inspect_update_sources(&directory).map_err(err)?;
                if !claims.allowed_predecessors.contains(&facts.predecessor) {
                    return Err("更新包的升级前驱与签名声明不一致。".into());
                }
                if ReleaseVersion::parse(&facts.version).map_err(err)? != claims.version {
                    return Err("更新包版本与签名声明不一致。".into());
                }
                // Unknown initial payload identity is not invented. Its sequence
                // boundary is fixed at the first D14 base; native predecessor
                // proof still binds the complete actual installed product.
                let installed = state.installed_release.unwrap_or(ReleaseIdentity {
                    sequence: ReleaseSequence::new(1).unwrap(),
                    payload_sha256: [0; 32],
                });
                preflight_release(
                    &claims.compatibility(),
                    &InstalledUpdateContext {
                        product: "FutureClassroom",
                        channel: &state.policy.channel,
                        installed_release: installed,
                        highest_accepted_release: state.accepted_release,
                        windows: fc_windows::update_windows_baseline().map_err(err)?,
                        architecture: UpdateArchitecture::X86,
                        coordinator: ReleaseVersion::parse(env!("CARGO_PKG_VERSION"))
                            .map_err(err)?,
                        schema: fc_storage::SCHEMA_VERSION as u32,
                        protocol: 3,
                        formal_predecessor: Some(&facts.predecessor),
                    },
                )
                .map_err(err)?;
                installer_lifecycle::qualify_update_predecessor(
                    &facts.predecessor_wire,
                    &facts.predecessor,
                )
                .map_err(err)?;
                Ok(())
            })();
            if let Err(error) = result {
                backend
                    .advance_background_update(&job, UpdateJobPhase::Failed)
                    .map_err(err)?;
                self.remove_stage(key)?;
                return Err(error);
            }
            self.validated_stage = Some(key);
        }
        if job.phase == UpdateJobPhase::Prepared || job.phase == UpdateJobPhase::Downloading {
            let saved = backend
                .advance_background_update(&job, UpdateJobPhase::Ready)
                .map_err(err)?;
            job = saved.jobs.iter().find(|j| j.id == job.id).unwrap().clone();
        }
        // Only fully native-inspected sources are advertised, including an
        // imported package used as the school's first distribution seed.
        if state.policy.share && self.firewall.as_ref().is_some_and(|(allowed, _)| *allowed) {
            control.network.publish(Some(PublishedRepository::new(
                metadata.clone(),
                vec![target],
            )?))?;
        }
        {
            let mut status = control.inner.lock().map_err(err)?;
            status.status.update_available = true;
            status.status.phase = "ready".into();
            status.status.received_bytes = target.length;
            status.status.total_bytes = target.length;
            status.status.update_message = "可信更新已就绪，等待正常退出后安装。".into();
        }
        if !state.policy.automatic_after_download_enabled() {
            return Ok(UpdateTick::Idle);
        }
        if cancelled() {
            return Err("更新安装交接已取消。".into());
        }
        // Ready may have waited across an idle/manual policy. Re-establish
        // publisher expiry and the current trusted catalog at the actual
        // irreversible intent, once; periodic Ready polling writes no checkpoint.
        let authorized = TrustedCatalog::refresh(
            ROOT,
            &self.root.join("checkpoints"),
            metadata,
            now()?,
            &Protection,
        )
        .map_err(err)?;
        if !authorized
            .targets
            .iter()
            .any(|current| current.sha256() == target.sha256 && current.length() == target.length)
        {
            return Err("更新包已不在当前签名目录中，等待重新检查。".into());
        }
        let handoff = update_install::prepare_handoff(update_install::PreparedInstall {
            job_id: key,
            policy_revision: job.policy_revision.get(),
            release_sequence: claims.identity.sequence.get(),
            setup_sha256: hash_file(&directory.join("Setup.exe"), cancelled)?,
            msi_sha256: hash_file(&directory.join("FutureClassroom.msi"), cancelled)?,
            ready_directory: directory,
        });
        let handoff = match handoff {
            Ok(prepared) => prepared,
            Err(error) => {
                backend
                    .advance_background_update(&job, UpdateJobPhase::Failed)
                    .map_err(err)?;
                self.remove_stage(key)?;
                return Err(err(error));
            }
        };
        if let Err(error) = backend.advance_background_update(&job, UpdateJobPhase::Handoff) {
            let _ = update_install::cancel_prepared_handoff(key);
            return Err(err(error));
        }
        self.handoff = true;
        self.notice(control, "handoff", "即将正常退出并安装更新，随后自动启动。")?;
        Ok(UpdateTick::Handoff(handoff))
    }
    fn remove_stage(&mut self, key: [u8; 16]) -> Result<(), String> {
        let path = self.root.join("staging").join(hex(&key));
        if path.try_exists().map_err(err)? {
            fs::remove_dir_all(&path).map_err(err)?;
        }
        self.stages.remove(&path);
        if self.validated_stage == Some(key) {
            self.validated_stage = None;
        }
        Ok(())
    }
    fn reconcile(
        &mut self,
        backend: &mut Backend,
        state: &UpdateMaintenanceState,
    ) -> Result<(), String> {
        if backend.state().map_err(err)?.installation.is_none() {
            return Ok(());
        }
        for receipt in update_install::handoff_receipts().map_err(err)? {
            let Some(current) = state.jobs.iter().find(|j| {
                job_key(j) == receipt.job_id
                    && j.release.sequence.get() == receipt.release_sequence
                    && j.policy_revision.get() == receipt.policy_revision
            }) else {
                continue;
            };
            let mut job = current.clone();
            // A crash can fall between durable preparation and spawning the
            // coordinator. An unlocked Prepared record proves no coordinator
            // owns it; cancel under its exclusive record lock instead of
            // leaving Handoff blocked forever or running Setup a second time.
            if receipt.state == update_install::HandoffState::Prepared && !self.handoff {
                if !matches!(
                    job.phase,
                    UpdateJobPhase::Prepared
                        | UpdateJobPhase::Downloading
                        | UpdateJobPhase::Ready
                        | UpdateJobPhase::WaitingIdle
                        | UpdateJobPhase::Handoff
                ) {
                    continue;
                }
                if update_install::cancel_prepared_handoff(receipt.job_id).is_err() {
                    continue; // An independently running helper may now own it.
                }
                if job.phase == UpdateJobPhase::Handoff {
                    let saved = backend
                        .advance_background_update(&job, UpdateJobPhase::NeedsReconcile)
                        .map_err(err)?;
                    job = saved.jobs.iter().find(|j| j.id == job.id).unwrap().clone();
                    backend
                        .advance_background_update(&job, UpdateJobPhase::Failed)
                        .map_err(err)?;
                } else {
                    backend
                        .advance_background_update(&job, UpdateJobPhase::Cancelled)
                        .map_err(err)?;
                }
                self.remove_stage(receipt.job_id)?;
                continue;
            }
            if matches!(
                receipt.state,
                update_install::HandoffState::AwaitingProductConfirmation
                    | update_install::HandoffState::Confirmed
            ) {
                if receipt.version != env!("CARGO_PKG_VERSION") {
                    continue;
                }
                if receipt.state == update_install::HandoffState::AwaitingProductConfirmation {
                    update_install::confirm_handoff(
                        receipt.job_id,
                        receipt.release_sequence,
                        receipt.policy_revision,
                    )
                    .map_err(err)?;
                }
                if job.phase == UpdateJobPhase::Handoff {
                    let saved = backend
                        .advance_background_update(
                            &job,
                            if job.policy_revision == state.policy.revision {
                                UpdateJobPhase::Installing
                            } else {
                                UpdateJobPhase::NeedsReconcile
                            },
                        )
                        .map_err(err)?;
                    job = saved.jobs.iter().find(|j| j.id == job.id).unwrap().clone();
                }
                if !job.phase.settled() {
                    backend
                        .advance_background_update(&job, UpdateJobPhase::Succeeded)
                        .map_err(err)?;
                }
                update_install::retire_confirmed_handoff(receipt.job_id).map_err(err)?;
                self.remove_stage(receipt.job_id)?;
            } else if receipt.state == update_install::HandoffState::Failed {
                if job.phase == UpdateJobPhase::Handoff {
                    let saved = backend
                        .advance_background_update(&job, UpdateJobPhase::NeedsReconcile)
                        .map_err(err)?;
                    job = saved.jobs.iter().find(|j| j.id == job.id).unwrap().clone();
                }
                if !job.phase.settled() {
                    backend
                        .advance_background_update(&job, UpdateJobPhase::Failed)
                        .map_err(err)?;
                }
                update_install::retire_failed_handoff(receipt.job_id).map_err(err)?;
                self.remove_stage(receipt.job_id)?;
            } else if matches!(
                receipt.state,
                update_install::HandoffState::Prepared | update_install::HandoffState::Installing
            ) && !job.phase.settled()
            {
                if job.phase == UpdateJobPhase::Handoff {
                    backend
                        .advance_background_update(&job, UpdateJobPhase::NeedsReconcile)
                        .map_err(err)?;
                }
                return Err(
                    "上次更新的安装交接尚未确认，已保留原记录并暂停自动重试，请管理员核对结果。"
                        .into(),
                );
            }
        }
        Ok(())
    }
    fn catalog(
        &mut self,
        control: &UpdateControl,
        backend: &mut Backend,
        metadata: OwnedMetadata,
        peer: Option<Peer>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        let catalog = TrustedCatalog::refresh(
            ROOT,
            &self.root.join("checkpoints"),
            metadata,
            now()?,
            &Protection,
        )
        .map_err(err)?;
        self.candidate = None;
        control.network.publish(None)?;
        let state = backend.update_runtime_state().map_err(err)?;
        let mut candidates = Vec::new();
        let mut shareable = Vec::new();
        for target in &catalog.targets {
            if cancelled() {
                return Err("更新任务已取消。".into());
            }
            let claims = ReleaseClaims::from_verified_custom(
                &serde_json::to_vec(target.custom().ok_or("更新声明缺失。")?).map_err(err)?,
                target.sha256(),
                target.length(),
            )
            .map_err(err)?;
            let cached = VerifiedTarget {
                sha256: target.sha256(),
                length: target.length(),
            };
            if state.policy.share
                && self.firewall.as_ref().is_some_and(|(allowed, _)| *allowed)
                && claims.channel == state.policy.channel
            {
                match self.cache.acquire_cancellable(cached, cancelled) {
                    Ok(mut blob) => {
                        let reservation = self.cache.reserve_external(MAX_IMPORT).map_err(err)?;
                        let stage = self
                            .root
                            .join("staging")
                            .join(format!("share-{}", hex(&random::<16>()?)));
                        fs::create_dir(&stage).map_err(err)?;
                        self.stages.insert(stage.clone(), reservation);
                        let valid: Result<(), String> = (|| {
                            packages::extract_install_sources_cancellable(
                                &mut blob, &stage, cancelled,
                            )
                            .map_err(err)?;
                            let facts =
                                installer_lifecycle::inspect_update_sources(&stage).map_err(err)?;
                            if claims.allowed_predecessors.contains(&facts.predecessor)
                                && ReleaseVersion::parse(&facts.version).map_err(err)?
                                    == claims.version
                            {
                                Ok(())
                            } else {
                                Err("更新源与签名声明不一致。".into())
                            }
                        })();
                        fs::remove_dir_all(&stage).map_err(err)?;
                        self.stages.remove(&stage);
                        valid?;
                        shareable.push(cached);
                    }
                    Err(update_cache::CacheError::NotReady) => (),
                    Err(error) => return Err(err(error)),
                }
            }
            if claims.channel != state.policy.channel
                || claims.identity.sequence.get()
                    <= state.installed_release.map_or(1, |r| r.sequence.get())
            {
                continue;
            }
            if claims.minimum_coordinator
                > ReleaseVersion::parse(env!("CARGO_PKG_VERSION")).map_err(err)?
                || !claims
                    .source_schema
                    .contains(fc_storage::SCHEMA_VERSION as u32)
                || !claims.source_protocol.contains(3)
                || claims.architecture != UpdateArchitecture::X86
                || claims.minimum_windows > fc_windows::update_windows_baseline().map_err(err)?
            {
                continue;
            }
            candidates.push((
                claims,
                VerifiedTarget {
                    sha256: target.sha256(),
                    length: target.length(),
                },
            ));
        }
        // Updated machines remain distribution seeds after restarting. Sharing
        // qualifies the source itself, independently of local upgrade eligibility.
        if state.policy.share
            && self.firewall.as_ref().is_some_and(|(allowed, _)| *allowed)
            && !shareable.is_empty()
        {
            control.network.publish(Some(PublishedRepository::new(
                catalog.metadata.clone(),
                shareable,
            )?))?;
        }
        let previous = state
            .jobs
            .iter()
            .rev()
            .filter(|j| j.phase == UpdateJobPhase::Succeeded)
            .nth(1)
            .map(|j| j.release.payload_sha256);
        self.cache
            .protect_sources(state.installed_release.map(|r| r.payload_sha256), previous)
            .map_err(err)?;
        candidates.sort_by_key(|(c, _)| c.identity.sequence);
        let Some((claims, target)) = candidates.pop() else {
            return Ok(());
        };
        backend
            .accept_update_release(state.policy.revision, claims.identity)
            .map_err(err)?;
        let ready = match self.cache.acquire_cancellable(target, cancelled) {
            Ok(_) => true,
            Err(update_cache::CacheError::NotReady) => false,
            Err(e) => return Err(err(e)),
        };
        if !ready && state.policy.download {
            if let Some(peer) = peer {
                control.network.request_blob(peer, target)?;
            }
        }
        {
            let mut status = control.inner.lock().map_err(err)?;
            status.status.update_available = true;
            status.status.total_bytes = target.length;
            status.status.phase = if ready { "verifying" } else { "downloading" }.into();
            status.status.update_message = if ready {
                "正在核对更新安装源。"
            } else {
                "已发现可信的新版本，正在后台下载。"
            }
            .into();
        }
        self.candidate = Some(Candidate {
            claims,
            target,
            metadata: catalog.metadata,
            ready,
        });
        Ok(())
    }
    fn io(
        &mut self,
        operation: NetworkOperation,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<NetworkIoResult, String> {
        match operation {
            NetworkOperation::OpenUpload { target, offset } => {
                if !self.policy.as_ref().is_some_and(|p| p.enabled && p.share) {
                    return Err("更新共享已关闭。".into());
                }
                let mut blob = self
                    .cache
                    .acquire_cancellable(target, cancelled)
                    .map_err(err)?;
                blob.seek(SeekFrom::Start(offset)).map_err(err)?;
                self.next_handle = self.next_handle.checked_add(1).ok_or("传输编号已用尽。")?;
                let handle = self.next_handle;
                self.handles.insert(handle, Handle::Upload(blob));
                Ok(NetworkIoResult::Opened { handle, offset })
            }
            NetworkOperation::ReadUpload {
                handle,
                offset,
                length,
            } => {
                if length > 65536 {
                    return Err("传输分块过大。".into());
                }
                let Some(Handle::Upload(blob)) = self.handles.get_mut(&handle) else {
                    return Err("传输已关闭。".into());
                };
                blob.seek(SeekFrom::Start(offset)).map_err(err)?;
                let mut bytes = vec![0; length];
                let n = blob.read(&mut bytes).map_err(err)?;
                bytes.truncate(n);
                Ok(NetworkIoResult::Bytes { bytes, eof: n == 0 })
            }
            NetworkOperation::BeginDownload { target } => {
                if !self
                    .policy
                    .as_ref()
                    .is_some_and(|p| p.enabled && p.download)
                {
                    return Err("更新下载已关闭。".into());
                }
                let download = self
                    .cache
                    .begin_cancellable(target, cancelled)
                    .map_err(err)?;
                let offset = download.offset();
                self.next_handle = self.next_handle.checked_add(1).ok_or("传输编号已用尽。")?;
                let handle = self.next_handle;
                self.handles
                    .insert(handle, Handle::Download { target, download });
                Ok(NetworkIoResult::Opened { handle, offset })
            }
            NetworkOperation::AppendDownload {
                handle,
                offset,
                bytes,
            } => {
                let Some(Handle::Download { download, .. }) = self.handles.get_mut(&handle) else {
                    return Err("传输已关闭。".into());
                };
                if offset != download.offset() {
                    return Err("传输位置不一致。".into());
                }
                download.append(&bytes).map_err(err)?;
                Ok(NetworkIoResult::Written {
                    offset: download.offset(),
                })
            }
            NetworkOperation::FinishDownload { handle } => {
                let Some(Handle::Download { target, download }) = self.handles.remove(&handle)
                else {
                    return Err("传输已关闭。".into());
                };
                let stage = self
                    .root
                    .join("staging")
                    .join(format!("verify-{}", hex(&random::<16>()?)));
                let reservation = self.cache.reserve_external(MAX_IMPORT).map_err(err)?;
                fs::create_dir(&stage).map_err(err)?;
                self.stages.insert(stage.clone(), reservation);
                let result = download.finish(|reader| {
                    packages::extract_install_sources_cancellable(reader, &stage, cancelled)
                        .map_err(|_| update_cache::CacheError::InvalidPackage)?;
                    installer_lifecycle::inspect_update_sources(&stage)
                        .map_err(|_| update_cache::CacheError::InvalidPackage)?;
                    Ok(())
                });
                if !cancelled()
                    && matches!(
                        &result,
                        Err(update_cache::CacheError::HashMismatch
                            | update_cache::CacheError::InvalidPackage)
                    )
                {
                    // finish has returned the original file owner. A complete
                    // corrupt prefix must not be resumed forever at EOF.
                    self.cache.discard_partial(target.sha256).map_err(err)?;
                }
                fs::remove_dir_all(&stage).map_err(err)?;
                self.stages.remove(&stage);
                result.map_err(err)?;
                Ok(NetworkIoResult::Finished)
            }
            NetworkOperation::Close { handle } => {
                self.handles.remove(&handle);
                Ok(NetworkIoResult::Closed)
            }
        }
    }
}
