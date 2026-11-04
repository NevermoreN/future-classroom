//! Typed Web management runs on the original shared worker, outside Backend
//! mutex scopes. Registry effects retain the existing journal and fresh guard.
use super::*;
use fc_bootstrap::{
    local_https::LocalControlHandle,
    web_application::{
        admin::{PlatformError, PlatformHook, PlatformReply, PlatformRequest},
        WebApplication,
    },
};
use fc_domain::{autostart::AutostartObservation, https_settings::*};
use fc_protocol::web::admin::platform as dto;
use std::sync::Weak;

pub(super) fn hook(
    backend: &Arc<Mutex<Backend>>,
    control: LocalControlHandle,
    app: Arc<OnceLock<Weak<WebApplication>>>,
    diagnostics: Arc<Mutex<local_web_controller::Diagnostics>>,
    workers: fc_runtime::worker::WorkerHandle<Work>,
) -> PlatformHook {
    let backend = Arc::downgrade(backend);
    Arc::new(move |lease, request, context| {
        let backend = backend.upgrade().ok_or(PlatformError::Unavailable)?;
        let service = {
            let value = backend.lock().map_err(|_| PlatformError::Unavailable)?;
            value
                .check_session(lease, SessionPurpose::Manage)
                .map_err(map_backend)?;
            value.web_service()
        };
        if !service.web_session_current(lease) {
            return Err(PlatformError::Denied);
        }
        context
            .checkpoint()
            .map_err(|_| PlatformError::Unavailable)?;
        match request {
            PlatformRequest::Status => {
                let (desktop, events) = diagnostics
                    .try_lock()
                    .map(|snapshot| (snapshot.desktop.clone(), snapshot.events.clone()))
                    .unwrap_or_default();
                let mut rows = Vec::with_capacity(12 + DIAGNOSTIC_EVENT_LIMIT);
                let mut row = |label: &str, value: String| {
                    rows.push(dto::Diagnostic {
                        label: label.into(),
                        value,
                    })
                };
                if let Some(stats) = workers.try_stats() {
                    row(
                        "后台队列",
                        format!(
                            "排队 {} 项 / {} 字节；执行或释放中 {} 项",
                            stats.queued, stats.queued_bytes, stats.running
                        ),
                    );
                }
                if let Ok(resources) = fc_windows::system::current_process_resources() {
                    row(
                        "内存",
                        format!(
                            "工作集 {} 字节；私有提交 {} 字节",
                            resources.working_set_bytes, resources.private_bytes
                        ),
                    );
                    row(
                        "资源对象",
                        format!(
                            "句柄 {}；GDI {}；USER {}",
                            resources.handles, resources.gdi_objects, resources.user_objects
                        ),
                    );
                    row(
                        "累计 CPU 时间",
                        format!("{} 毫秒", resources.cpu_time.as_millis()),
                    );
                }
                if let Some(metrics) = &desktop {
                    row("绘制方式", metrics.render_capability.label().into());
                    row(
                        "组件与绘图表面",
                        format!("组件 {}；表面 {}", metrics.components, metrics.surfaces),
                    );
                    row(
                        "绘图内存",
                        format!(
                            "DIB {} 字节；暂存峰值 {} 字节",
                            metrics.dib_bytes, metrics.scratch_peak_bytes
                        ),
                    );
                    row(
                        "累计帧数",
                        format!(
                            "绘制 {}；上传 {}",
                            metrics.drawn_frames, metrics.uploaded_frames
                        ),
                    );
                    row(
                        "桌面窗口",
                        format!(
                            "可见 {}；输入区域 {}",
                            metrics.visible_windows, metrics.input_regions
                        ),
                    );
                }
                for event in events {
                    row(
                        &format!("动作 +{} ms", event.after_ms),
                        format!(
                            "{} · {} · 耗时 {} ms（后台返回不等于效果完成）",
                            event.operation, event.phase, event.elapsed_ms
                        ),
                    );
                }
                let value = backend.lock().map_err(|_| PlatformError::Unavailable)?;
                let identity = value.management_identity(lease).map_err(map_backend)?;
                let installation = identity.installation.as_ref();
                let monitors = fc_windows::desktop::display_monitors()
                    .map_err(|_| PlatformError::Unavailable)?;
                Ok(PlatformReply::Status(
                    value.global_revision().map_err(map_backend)?,
                    dto::Status {
                        application_version: env!("CARGO_PKG_VERSION").into(),
                        school_name: installation.map(|i| i.school_name.clone()),
                        current_class_id: installation
                            .and_then(|i| i.current_class.map(|id| id.get().to_string())),
                        desktop_state: if !fc_windows::host::interactive_session_available() {
                            "Windows会话不可显示"
                        } else if desktop
                            .as_ref()
                            .is_some_and(|metrics| metrics.visible_windows > 0)
                        {
                            "正在显示"
                        } else {
                            "当前没有可见桌面组件"
                        }
                        .into(),
                        listener_state: lan_label(&control),
                        storage_state: "数据库与权限状态可读取".into(),
                        monitors: monitors
                            .into_iter()
                            .map(|m| dto::Monitor {
                                id: m.id,
                                name: m.name,
                                primary: m.primary,
                            })
                            .collect(),
                        diagnostics: rows,
                    },
                ))
            }
            PlatformRequest::Autostart => autostart(&backend, lease, None, context),
            PlatformRequest::ChangeAutostart {
                expected_revision,
                change,
            } => autostart(&backend, lease, Some((expected_revision, change)), context),
            PlatformRequest::Network => network(&backend, lease, &control),
            PlatformRequest::ChangeNetwork {
                expected_revision,
                change,
            } => {
                let current = backend
                    .lock()
                    .map_err(|_| PlatformError::Unavailable)?
                    .read_https_settings(lease)
                    .map_err(map_backend)?;
                if current.settings.revision != expected_revision {
                    return Err(PlatformError::Conflict);
                }
                match change {
                    dto::NetworkChange::Stop => {
                        let authority = Arc::new(move || service.web_session_current(lease));
                        control
                            .configure_lan(None, None, None, context, authority)
                            .map_err(|_| PlatformError::Unavailable)?
                            .wait_in_job(context)
                            .map_err(|_| PlatformError::Unavailable)?;
                    }
                    dto::NetworkChange::Start => {
                        let configuration = current
                            .settings
                            .configuration
                            .ok_or(PlatformError::Invalid)?;
                        let prefix = fc_ports::http::ListenPrefix::https(&format!(
                            "{}/fc/",
                            configuration.origin()
                        ))
                        .map_err(|_| PlatformError::Invalid)?;
                        let original = app
                            .get()
                            .and_then(Weak::upgrade)
                            .ok_or(PlatformError::Unavailable)?;
                        let lan = Arc::new(
                            original
                                .lan_variant(&prefix)
                                .map_err(|_| PlatformError::Invalid)?,
                        );
                        let assets = control
                            .assets_for_prefix(&prefix)
                            .map_err(|_| PlatformError::Unavailable)?;
                        let authority = Arc::new(move || service.web_session_current(lease));
                        control
                            .configure_lan(
                                Some(configuration),
                                Some(lan),
                                Some(assets),
                                context,
                                authority,
                            )
                            .map_err(|_| PlatformError::Unavailable)?
                            .wait_in_job(context)
                            .map_err(|_| PlatformError::Unavailable)?;
                    }
                    change => {
                        let (host, port, certificate, networks) = match change {
                            dto::NetworkChange::Save(value) => (
                                value.host,
                                value.port,
                                HttpsCertificateRef::parse(&value.certificate_thumbprint)
                                    .map_err(|_| PlatformError::Invalid)?,
                                value.source_networks,
                            ),
                            dto::NetworkChange::Generate(value) => {
                                // The startup owner already generated/trusted one
                                // per-installation certificate for actual local IPs.
                                let hosts=fc_windows::tls_frontdoor::certificate_provision::local_certificate_hosts().map_err(|_|PlatformError::Unavailable)?;
                                if !hosts.contains(&value.host) {
                                    return Err(PlatformError::Invalid);
                                }
                                (
                                    value.host,
                                    value.port,
                                    control.certificate(),
                                    value.source_networks,
                                )
                            }
                            _ => return Err(PlatformError::Invalid),
                        };
                        let policy = HttpsSourceNetworks::parse(&networks.join("\n"))
                            .map_err(|_| PlatformError::Invalid)?;
                        let configuration = HttpsConfiguration::new(
                            HttpsHost::parse(&host).map_err(|_| PlatformError::Invalid)?,
                            port,
                            certificate,
                        )
                        .map_err(|_| PlatformError::Invalid)?
                        .with_source_networks(policy);
                        let resume_lan = control.lan_status().phase
                            == fc_windows::tls_frontdoor::LanListenerPhase::Listening;
                        if resume_lan {
                            let service = service.clone();
                            control
                                .configure_lan(
                                    None,
                                    None,
                                    None,
                                    context,
                                    Arc::new(move || service.web_session_current(lease)),
                                )
                                .map_err(|_| PlatformError::Unavailable)?
                                .wait_in_job(context)
                                .map_err(|_| PlatformError::Unavailable)?;
                        }
                        context
                            .checkpoint()
                            .map_err(|_| PlatformError::Unavailable)?;
                        let mut value = backend.lock().map_err(|_| PlatformError::Unavailable)?;
                        let issued = value
                            .issue_for_web_admin(
                                lease,
                                fc_ports::command_window::CommandFamily::HttpsSettings,
                            )
                            .map_err(map_backend)?;
                        let mut scoped = value.begin_command_scope(issued).map_err(map_backend)?;
                        let command_id = scoped.issued_command_id().map_err(map_backend)?;
                        scoped
                            .save_https_settings(
                                lease,
                                fc_ports::HttpsSettingsRequest {
                                    command_id,
                                    expected_revision,
                                    expected_identity_revision: current.identity_revision,
                                    configuration: Some(configuration.clone()),
                                },
                            )
                            .map_err(map_backend)?;
                        drop(scoped);
                        drop(value);
                        if resume_lan {
                            let prefix = fc_ports::http::ListenPrefix::https(&format!(
                                "{}/fc/",
                                configuration.origin()
                            ))
                            .map_err(|_| PlatformError::Invalid)?;
                            let original = app
                                .get()
                                .and_then(Weak::upgrade)
                                .ok_or(PlatformError::Unavailable)?;
                            let lan = Arc::new(
                                original
                                    .lan_variant(&prefix)
                                    .map_err(|_| PlatformError::Invalid)?,
                            );
                            let assets = control
                                .assets_for_prefix(&prefix)
                                .map_err(|_| PlatformError::Unavailable)?;
                            control
                                .configure_lan(
                                    Some(configuration),
                                    Some(lan),
                                    Some(assets),
                                    context,
                                    Arc::new(move || service.web_session_current(lease)),
                                )
                                .map_err(|_| PlatformError::Unavailable)?
                                .wait_in_job(context)
                                .map_err(|_| PlatformError::Unavailable)?;
                        }
                    }
                }
                network(&backend, lease, &control)
            }
        }
    })
}
fn map_backend(error: BackendError) -> PlatformError {
    match error {
        BackendError::Session(_) | BackendError::InvalidCredentials => PlatformError::Denied,
        BackendError::HttpsSettings(fc_ports::HttpsSettingsRepositoryError::Domain(
            HttpsSettingsError::RevisionConflict { .. },
        )) => PlatformError::Conflict,
        BackendError::InvalidInput => PlatformError::Invalid,
        _ => PlatformError::Unavailable,
    }
}
fn lan_label(control: &LocalControlHandle) -> String {
    let status = control.lan_status();
    let label = match status.phase {
        fc_windows::tls_frontdoor::LanListenerPhase::Disabled => "已关闭",
        fc_windows::tls_frontdoor::LanListenerPhase::Draining => "正在切换或关闭",
        fc_windows::tls_frontdoor::LanListenerPhase::Listening => "已开启",
    };
    status.error.map_or_else(
        || label.into(),
        |error| format!("{label}；{}", error.message()),
    )
}
fn network(
    backend: &Arc<Mutex<Backend>>,
    lease: SessionLease,
    control: &LocalControlHandle,
) -> std::result::Result<PlatformReply, PlatformError> {
    let state = backend
        .lock()
        .map_err(|_| PlatformError::Unavailable)?
        .read_https_settings(lease)
        .map_err(map_backend)?;
    let c = state.settings.configuration.as_ref();
    let selected_certificate = c.map_or(control.certificate(), |c| c.certificate);
    let certificate = fc_windows::tls_frontdoor::certificate_provision::export_owned_certificate(
        &selected_certificate.to_hex(),
    )
    .ok();
    Ok(PlatformReply::Network(
        state.settings.revision,
        dto::Network {
            host: c.map(|c| c.host.as_str().into()),
            port: c.map(|c| c.port),
            certificate_thumbprint: Some(
                c.map_or(control.certificate(), |c| c.certificate).to_hex(),
            ),
            source_networks: c
                .and_then(|c| c.source_networks.as_ref())
                .map_or_else(Vec::new, |p| {
                    p.networks().iter().map(|n| n.as_str().into()).collect()
                }),
            listener_state: lan_label(control),
            local_management_url: format!("https://127.0.0.1:{}/fc/", control.port()),
            teacher_url: c.map(|c| format!("{}/fc/", c.origin())),
            certificate_trusted: (selected_certificate == control.certificate()).then_some(true),
            certificate_expires_at: None,
            certificate_public_pem: certificate
                .as_ref()
                .map(|certificate| pem(&certificate.public_der)),
        },
    ))
}
fn autostart(
    backend: &Arc<Mutex<Backend>>,
    lease: SessionLease,
    change: Option<(Revision, dto::AutostartChange)>,
    context: &JobContext,
) -> std::result::Result<PlatformReply, PlatformError> {
    let scope = fc_windows::system::RunScope::current_executable()
        .map_err(|_| PlatformError::Unavailable)?;
    let mut system = scope.open().map_err(|_| PlatformError::Unavailable)?;
    let loaded = fc_bootstrap::autostart::Job {
        backend: backend.clone(),
        lease: Some(lease),
        operation: fc_bootstrap::autostart::Operation::Load,
    }
    .run(context, &mut system)
    .map_err(|_| PlatformError::Unavailable)?;
    let loaded = if let Some((expected, change)) = change {
        let current = loaded.state.as_ref().ok_or(PlatformError::Invalid)?;
        if current.revision != expected {
            return Err(PlatformError::Conflict);
        }
        let desired = match change {
            dto::AutostartChange::Set(enabled) => enabled,
            dto::AutostartChange::Repair => current.desired,
        };
        let issued = backend
            .lock()
            .map_err(|_| PlatformError::Unavailable)?
            .issue_for_web_admin(lease, fc_ports::command_window::CommandFamily::Autostart)
            .map_err(map_backend)?;
        fc_bootstrap::autostart::Job {
            backend: backend.clone(),
            lease: Some(lease),
            operation: fc_bootstrap::autostart::Operation::Change {
                expected,
                desired,
                reviewed_target: loaded.target_command,
                previous_observed: loaded.observed,
            },
        }
        .run_with_command(context, &mut system, Some(issued))
        .map_err(|_| PlatformError::Unavailable)?
    } else {
        loaded
    };
    let state = loaded.state.ok_or(PlatformError::Invalid)?;
    let actual_state = match &loaded.observed {
        AutostartObservation::Absent => "关闭：没有本项目启动项".into(),
        AutostartObservation::Command(command) if command == &loaded.target_command => {
            "开启：指向当前程序".into()
        }
        AutostartObservation::Command(_) => "路径发生变化，请修复".into(),
        AutostartObservation::Opaque { .. } => "现有值不属于有效启动命令，请核对".into(),
        AutostartObservation::Unavailable(reason) => format!("状态读取失败：{reason:?}"),
    };
    Ok(PlatformReply::Autostart(
        state.revision,
        dto::Autostart {
            desired_enabled: state.desired,
            actual_state,
            last_error: loaded
                .operation_outcome
                .and_then(|outcome| outcome.failure.map(|e| format!("{e:?}"))),
        },
    ))
}
fn pem(bytes: &[u8]) -> String {
    const BASE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            encoded.push(if i > c.len() {
                '='
            } else {
                BASE[((n >> (18 - i * 6)) & 63) as usize] as char
            });
        }
    }
    let mut result = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in encoded.as_bytes().chunks(64) {
        result.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        result.push('\n');
    }
    result.push_str("-----END CERTIFICATE-----\n");
    result
}
