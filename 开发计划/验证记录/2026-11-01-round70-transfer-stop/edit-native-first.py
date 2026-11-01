from pathlib import Path
p=Path('/tmp/fc-round70-ordinary-transfer-stop/worktree/crates/fc-bootstrap/src/application/ordinary_https_probe.rs');s=p.read_text()
s=s.replace('const ORDER: [&str; 25]', 'const ORDER: [&str; 31]')
s=s.replace('    "first-real-authenticated-reminder-post-and-get",','    "first-real-authenticated-reminder-post-and-get",\n    "first-material-ready-and-partial-upload-established",')
s=s.replace('    "physical-first-domain-drained",','    "physical-first-domain-drained",\n    "first-two-staging-files-removed",')
s=s.replace('    "second-real-tls-response-complete",','''    "second-real-tls-response-complete",
    "native-open-second-runtime-pairing-window",
    "native-approve-second-network-pairing-request",
    "second-old-cookie-rejected-and-two-upload-slots-reused",
    "second-typed-material-command-receipt-and-cancel",''')
start=s.index('fn confirm_disable(');end=s.index('fn fixture_password',start)
s=s[:start]+'''fn confirm_disable(app: &mut App, cycle: u8, marker: &str, deadline: Instant) -> Check<()> {
    let generation = running(app).ok_or("stop requires original running generation")?;
    submit(app, web_editor::DISABLE, &[], deadline)?;
    let began = Instant::now();
    let stop_deadline = deadline.min(began + Duration::from_secs(5));
    submit(app, web_editor::CONFIRM, &[], stop_deadline)?;
    wait(app, stop_deadline, |a| {
        a.teacher_https_quiescent() && a.web_readiness == web_editor::Readiness::Disabled
    })?;
    let elapsed = began.elapsed();
    require(elapsed <= Duration::from_secs(5), "one original stop interval")?;
    let report = app.teacher_https.owner.as_ref().and_then(|o| o.network_report())
        .ok_or("original stopped network report absent")?;
    require(report.native_drained && report.active_records == 0 && report.pending_jobs == 0
        && report.cleanup_pending == 0 && report.retained_cleanup == 0,
        "original Net/native ownership actually drained")?;
    println!("ORDINARY_HTTPS_STOPPED {{\\"cycle\\":{cycle},\\"marker\\":\\"{marker}\\",\\"generation\\":{generation},\\"elapsedNs\\":{},\\"activeRecords\\":0,\\"pendingJobs\\":0,\\"cleanupPending\\":0,\\"retainedCleanup\\":0,\\"nativeDrained\\":true}}", elapsed.as_nanos());
    checked(std::io::stdout().flush(), "stop observation flush")
}
fn stage_path(app: &App, id: u64) -> PathBuf {
    app.root.join("web-material-staging").join(format!("web-material-{id}.part"))
}
fn stage_length(app: &App, id: u64) -> Check<u64> {
    use std::os::windows::fs::MetadataExt;
    let metadata = fs::symlink_metadata(stage_path(app, id))
        .map_err(|_| "exact owned material stage metadata unavailable")?;
    require(metadata.file_type().is_file() && metadata.file_attributes() & 0x400 == 0,
        "owned stage must be regular and non-reparse")?;
    Ok(metadata.len())
}
fn absent_stage(app: &App, id: u64) -> Check<()> {
    require(matches!(fs::symlink_metadata(stage_path(app, id)), Err(e)
        if e.kind() == std::io::ErrorKind::NotFound), "exact old material stage absent")
}
fn completion(app: &mut App, root: &Path, marker: &str, cycle: u8,
    generation: u64, deadline: Instant) -> Check<[u64; 2]> {
    let path = root.join(format!("ordinary-business-{cycle}.done"));
    loop {
        pump(app, deadline)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                require(metadata.file_type().is_file() && metadata.len() <= 256,
                    "bounded regular completion marker")?;
                let bytes = fs::read(&path).map_err(|_| "owned completion read failed")?;
                let text = std::str::from_utf8(&bytes).map_err(|_| "completion encoding")?;
                let fields: Vec<_> = text.split('\\n').collect();
                require(fields.len() == 7 && fields[0] == "FC_ORDINARY_DONE_V2"
                    && fields[1] == marker && fields[2] == format!("{generation:020}")
                    && fields[3] == cycle.to_string() && fields[6].is_empty(),
                    "exact completion marker/cycle/generation")?;
                let mut ids = [0; 2];
                for (index, input) in fields[4..6].iter().enumerate() {
                    ids[index] = input.parse().map_err(|_| "completion upload ID")?;
                    require(ids[index] > 0 && *input == format!("{:020}", ids[index]),
                        "canonical bounded completion ID")?;
                }
                require(ids[0] != ids[1] && Instant::now() < deadline,
                    "distinct upload IDs before original deadline")?;
                return Ok(ids);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("owned completion metadata failed".into()),
        }
        require(Instant::now() < deadline, "original authenticated transfer deadline")?;
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
fn pair(app: &mut App, root: &Path, marker: &str, cycle: u8, generation: u64,
    port: u16, steps: &mut usize, deadline: Instant) -> Check<()> {
    submit(app, web_editor::REFRESH, &[], deadline)?;
    submit(app, web_editor::BEGIN, &[], deadline)?;
    let code = app.web_editor.as_ref().and_then(web_editor::Editor::code_text)
        .ok_or("actual ordinary pairing code absent")?;
    {
        use std::io::Write;
        let mut output = fs::OpenOptions::new().write(true).create_new(true)
            .open(root.join(format!("ordinary-pairing-{cycle}.secret")))
            .map_err(|_| "fresh owned pairing sidechannel unavailable")?;
        write!(output, "FC_ORDINARY_PAIR_V2\\n{marker}\\n{generation:020}\\n{cycle}\\n{}\\n{TEACHER_LOGIN}\\n", code.as_str())
            .map_err(|_| "owned pairing sidechannel write failed")?;
    }
    drop(code);
    println!("ORDINARY_HTTPS_PAIR_READY {{\\"cycle\\":{cycle},\\"marker\\":\\"{marker}\\",\\"port\\":{port},\\"generation\\":{generation},\\"origin\\":\\"https://127.0.0.1:{port}\\"}}");
    checked(std::io::stdout().flush(), "pairing rendezvous flush")?;
    mark(steps, if cycle == 1 { "native-open-actual-runtime-pairing-window" }
        else { "native-open-second-runtime-pairing-window" })?;
    let request = loop {
        require(Instant::now() < deadline, "original network pairing deadline")?;
        submit(app, web_editor::REFRESH, &[], deadline)?;
        let choices = app.view().fields.into_iter().find_map(|field| {
            if field.id != web_editor::REQUEST { return None; }
            match field.kind { FieldKind::Choice(choices) => Some(choices), _ => None }
        }).ok_or("actual native pairing list absent")?;
        require(choices.len() <= 1, "only one synthetic pairing request")?;
        if let Some(choice) = choices.first() { break choice.value.clone(); }
        std::thread::park_timeout(Duration::from_millis(100));
    };
    checked(app.window.probe_choice(web_editor::REQUEST, &request), "native request selection")?;
    pump(app, deadline)?;
    require(app.view().actions.iter().any(|a| a.id == web_editor::APPROVE && a.enabled),
        "actual selected request enables approval")?;
    submit(app, web_editor::APPROVE, &[(web_editor::REQUEST, &request)], deadline)?;
    mark(steps, if cycle == 1 { "native-approve-actual-network-pairing-request" }
        else { "native-approve-second-network-pairing-request" })
}
''' +s[end:]
# Replace first pairing + old marker loop with bounded shared helpers.
start=s.index('    // Refresh coherent logical metadata');end=s.index('    wait(app, deadline, |a| {',start)
s=s[:start]+'''    let package_root = completion.parent().ok_or("owned completion parent absent")?;
    pair(app, package_root, marker, 1, first, port, steps, deadline)?;
    let first_ids = completion(app, package_root, marker, 1, first, deadline)?;
    require(stage_length(app, first_ids[0])? == 65536,
        "actual completed MaterialReady stage retained before stop")?;
    let partial = stage_length(app, first_ids[1])?;
    require(partial > 0 && partial < 1048576,
        "actual partial body consumed before native stop")?;
''' +s[end:]
# Avoid shadow helper by renaming local completion Path.
s=s.replace('let completion = app','let completion_path = app',1).replace('let package_root = completion.parent()','let package_root = completion_path.parent()')
s=s.replace('    mark(steps, "first-real-authenticated-reminder-post-and-get")?;','''    mark(steps, "first-real-authenticated-reminder-post-and-get")?;
    mark(steps, "first-material-ready-and-partial-upload-established")?;''')
s=s.replace('    confirm_disable(app, deadline)?;', '    confirm_disable(app, 1, marker, deadline)?;',1)
s=s.replace('    mark(steps, "physical-first-domain-drained")?;', '''    mark(steps, "physical-first-domain-drained")?;
    for id in first_ids { absent_stage(app, id)?; }
    mark(steps, "first-two-staging-files-removed")?;''')
needle='''    let second = running(app).unwrap();
    submit(app, LOCK''';assert s.count(needle)==1
s=s.replace(needle,'''    let second = running(app).unwrap();
    pair(app, package_root, marker, 2, second, port, steps, deadline)?;
    let second_ids = completion(app, package_root, marker, 2, second, deadline)?;
    for id in second_ids { absent_stage(app, id)?; }
    require(second_ids.iter().all(|id| !first_ids.contains(id)), "new material IDs do not alias old generation")?;
    mark(steps, "second-old-cookie-rejected-and-two-upload-slots-reused")?;
    mark(steps, "second-typed-material-command-receipt-and-cancel")?;
    submit(app, web_editor::CLOSE, &[], deadline)?;
    submit(app, LOCK''')
s=s.replace('    confirm_disable(app, deadline)?;', '    confirm_disable(app, 2, marker, deadline)?;',1)
s=s.replace('.join("ordinary-business-first.done")','.join("ordinary-business-1.done")')
s=s.replace('material_ready_stop=NOT_EXERCISED campus=NOT_VALIDATED','material_ready_stop=PASS partial_upload_stop=PASS upload_slots_reused=2 typed_material_receipt=PASS campus=NOT_VALIDATED')
p.write_text(s)
