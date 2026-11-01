from pathlib import Path
import hashlib,json
w=Path('/tmp/fc-round68-production-https/worktree');e=w.parent/'evidence'
files=['crates/fc-bootstrap/src/application/https_runtime_controller.rs','crates/fc-bootstrap/src/application/ordinary_https_probe.rs','scripts/windows-ordinary-https-probe.py']
rows=[]
for f in files:
 p=w/f;d=e/'ui-C-before-third'/f;d.parent.mkdir(parents=True,exist_ok=True);d.write_bytes(p.read_bytes());rows.append({'relativePath':f,'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()})
with (e/'ui-C-before-third.json').open('x') as out:json.dump({'schemaVersion':1,'status':'PRE_EDIT_SNAPSHOT','scope':'before replacing unrun GlobalAdmin web journey with independent TeacherAdmin; no runtime claims','files':rows},out,indent=2)
p=w/files[1];s=p.read_text()
s=s.replace('const ORDER: [&str; 24]', 'const ORDER: [&str; 25]').replace('    "native-create-synthetic-class",','    "native-create-synthetic-class",\n    "native-create-independent-teacher-and-complete-first-password-change",',1)
s=s.replace('type Check<T>', 'const TEACHER_LOGIN: &str = "synthetic-ordinary-teacher";\n\ntype Check<T>',1)
needle='''fn workflow(
'''
insert='''fn fixture_password() -> Check<fc_security::web::SecretText> {
    let mut bytes = [0u8; 32];
    checked(SystemRandom.fill(&mut bytes), "private teacher fixture randomness")?;
    let text = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    bytes.fill(0);
    Ok(fc_security::web::SecretText::new(text))
}
fn create_teacher(app: &mut App, deadline: Instant) -> Check<()> {
    use accounts_editor as e;
    // Real native form commands go through the original shared workers, normal
    // credential KDFs and identity transactions; no SQLite/session fabrication.
    submit(app, TO_SUBJECTS, &[], deadline)?;
    submit(app, SAVE, &[(OLD_NAME, ""), (SUBJECT, "Synthetic HTTPS subject"), (COLOR, "123456")], deadline)?;
    submit(app, BACK, &[], deadline)?;
    submit(app, TO_ACCOUNTS, &[], deadline)?;
    let initial = fixture_password()?;
    let changed = fixture_password()?;
    submit(app, e::NEW, &[], deadline)?;
    submit(app, e::SCOPES, &[(e::LOGIN, TEACHER_LOGIN), (e::NAME, "Synthetic HTTPS teacher"), (e::ROLE, "teacher")], deadline)?;
    submit(app, e::ADD_SCOPE, &[(e::CLASS, "1"), (e::SUBJECT, "1")], deadline)?;
    submit(app, e::RETURN, &[], deadline)?;
    submit(app, e::REVIEW, &[], deadline)?;
    submit(app, e::CONFIRM, &[], deadline)?;
    submit(app, e::CONFIRM, &[(e::PASSWORD, initial.as_str()), (e::CONFIRM_PASSWORD, initial.as_str())], deadline)?;
    require(app.screen == Screen::Code, "teacher created with real one-time recovery page")?;
    submit(app, ACK, &[], deadline)?;
    submit(app, LOCK, &[], deadline)?;
    submit(app, LOG_IN, &[(LOGIN, TEACHER_LOGIN), (PASSWORD, initial.as_str())], deadline)?;
    require(app.screen == Screen::Password, "teacher first-login password change required")?;
    submit(app, SAVE, &[(PASSWORD, initial.as_str()), (NEW_PASSWORD, changed.as_str()), (CONFIRM, changed.as_str())], deadline)?;
    drop(initial);
    require(app.screen == Screen::Login, "real password change invalidates prior teacher lease")?;
    submit(app, LOG_IN, &[(LOGIN, TEACHER_LOGIN), (PASSWORD, changed.as_str())], deadline)?;
    drop(changed);
    let teacher = app.state.account_by_login(TEACHER_LOGIN).map_err(|_| "teacher absent")?;
    require(app.screen == Screen::Home && teacher.principal.enabled && teacher.principal.role == Role::TeacherAdmin
        && !teacher.must_change_password && app.state.assignments.iter().filter(|scope| scope.teacher_id == teacher.principal.id
            && scope.enabled && scope.class_id.get() == 1 && scope.subject_id.get() == 1).count() == 1,
        "independent enabled teacher and exact class/subject scope")?;
    submit(app, LOCK, &[], deadline)?;
    login(app, deadline)
}
'''
assert needle in s;s=s.replace(needle,insert+needle,1)
s=s.replace('''    submit(app, BACK, &[], deadline)?;
    mark(steps, "native-create-synthetic-class")?;''','''    submit(app, BIND_CLASS, &[(CLASS, "Synthetic ordinary HTTPS class")], deadline)?;
    submit(app, BACK, &[], deadline)?;
    mark(steps, "native-create-synthetic-class")?;
    create_teacher(app, deadline)?;
    mark(steps, "native-create-independent-teacher-and-complete-first-password-change")?;''',1)
s=s.replace('{LOGIN_NAME}\\n",\n            code.as_str()', '{TEACHER_LOGIN}\\n",\n            code.as_str()',1)
s=s.replace('''    submit(
        app,
        web_editor::APPROVE,''','''    checked(app.window.probe_choice(web_editor::REQUEST, &request), "native request ComboBox selection")?;
    pump(app, deadline)?;
    require(app.view().actions.iter().any(|action| action.id == web_editor::APPROVE && action.enabled), "selected actual request enables approval")?;
    submit(
        app,
        web_editor::APPROVE,''',1)
p.write_text(s)
p=w/files[2];s=p.read_text().replace('"native-create-synthetic-class", "native-teacher-page-default-off",','"native-create-synthetic-class",\n    "native-create-independent-teacher-and-complete-first-password-change", "native-teacher-page-default-off",',1)
s=s.replace('steps=24 shared_workers','steps=25 shared_workers').replace('ordinary exact twenty-four steps','ordinary exact twenty-five steps').replace('synthetic-voice-settings-admin','synthetic-ordinary-teacher')
s=s.replace('"session": "ACTUAL_NATIVE_APPROVAL", "classId": "1"','"session": "ACTUAL_NATIVE_APPROVAL", "role": "TEACHER_ADMIN", "classId": "1"')
# Exact session role is validated against the actual server DTO, not just fixture creation.
s=s.replace('''    client.csrf = session["csrf"]''','''    require(session.get("principal",{}).get("role") == "TEACHER_ADMIN", "actual wire session has teacher role")
    client.csrf = session["csrf"]''',1)
p.write_text(s)
