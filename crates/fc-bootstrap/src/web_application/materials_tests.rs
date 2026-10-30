//! Real SQLite/credential/Web boundary coverage. TXT preparation validates actual
//! fixture bytes; Windows WIC/PDF/stream integration is separately probed.
use super::*;
use fc_domain::{auth::*, identity::*, ids::*, materials::*};
use fc_ports::{MaterialAppendMetadata, MaterialPreparation, MaterialRepositoryError};
use std::time::Duration;
fn scope() -> MaterialScope {
    MaterialScope {
        teacher_id: PrincipalId::new(1).unwrap(),
        subject_id: SubjectId::new(1).unwrap(),
    }
}
fn catalog(f: &mut BackendFixture, subjects: u64) {
    f.backend
        .identity(
            f.admin,
            IdentityCommand::CreateClass {
                id: ClassId::new(1).unwrap(),
                name: "Synthetic material class".into(),
            },
        )
        .unwrap();
    for id in (1..=subjects).rev() {
        f.backend
            .identity(
                f.admin,
                IdentityCommand::PutSubject {
                    id: SubjectId::new(id).unwrap(),
                    name: format!("Synthetic material subject {id}"),
                    enabled: true,
                    color: Rgb {
                        red: 2,
                        green: 3,
                        blue: 4,
                    },
                    expected_revision: None,
                },
            )
            .unwrap();
    }
    f.backend
        .identity(
            f.admin,
            IdentityCommand::UpdateAccount {
                id: PrincipalId::new(1).unwrap(),
                display_name: "Synthetic material global".into(),
                role: Role::GlobalAdmin,
                enabled: true,
                scopes: (1..=subjects)
                    .rev()
                    .map(|id| AssignmentScope {
                        class_id: ClassId::new(1).unwrap(),
                        subject_id: SubjectId::new(id).unwrap(),
                    })
                    .collect(),
            },
        )
        .unwrap();
    f.admin = f
        .backend
        .login("admin", Password::new(PASSWORD.into()).unwrap())
        .unwrap();
}
fn auth(f: &mut BackendFixture, device: &str) -> (String, String, SessionLease) {
    let cookie = f.pair(device, false);
    let session = f.dispatch(&head("GET", "session", 0, Some(&cookie), None), b"");
    assert_eq!(session.head.status, 200);
    let csrf = field(&session.body, "csrf");
    let secret = cookie_secret(cookie.as_bytes()).unwrap();
    let lease = f.service.authenticate(&secret).unwrap().lease();
    (cookie, csrf, lease)
}
fn get(f: &mut BackendFixture, cookie: &str, path: &str) -> WebResponse {
    f.dispatch(&head("GET", path, 0, Some(cookie), None), b"")
}
fn post(f: &mut BackendFixture, cookie: &str, csrf: &str, path: &str, body: &str) -> WebResponse {
    f.dispatch(
        &head("POST", path, body.len(), Some(cookie), Some(csrf)),
        body.as_bytes(),
    )
}
fn expect(r: &WebResponse, status: u16, needle: &str) {
    assert_eq!(r.head.status, status, "{}", text(&r.body));
    assert!(text(&r.body).contains(needle), "{}", text(&r.body));
}
struct TextControl;
impl fc_materials::office::ValidationControl for TextControl {
    fn canceled(&self) -> bool {
        false
    }
    fn thread_cpu_time(&self) -> Result<Duration, fc_materials::office::ContainerError> {
        Ok(Duration::ZERO)
    }
}
struct TxtPreparation {
    version: ResourceVersionId,
}
impl MaterialPreparation for TxtPreparation {
    type Input = std::fs::File;
    fn prepare(
        &mut self,
        mut file: Self::Input,
    ) -> Result<MaterialResource, MaterialRepositoryError> {
        let report = fc_materials::validation::validate_file(&mut file, "proof.txt", &TextControl)
            .map_err(|_| MaterialRepositoryError::Domain(MaterialError::InvalidResource))?;
        Ok(MaterialResource {
            version_id: self.version,
            sha256: report.sha256,
            kind: report.kind,
            bytes: report.bytes,
        })
    }
}
fn append(f: &mut BackendFixture, lease: SessionLease, cmd: u64, id: u64) -> MaterialRecord {
    let fixture = f.directory.0.join(format!("actual-txt-{id}.txt"));
    std::fs::write(&fixture, b"Actual synthetic material UTF-8 bytes\n").unwrap();
    let proof = fc_ports::prepare_material_append(
        &mut TxtPreparation {
            version: ResourceVersionId::new(id + 10000).unwrap(),
        },
        std::fs::File::open(fixture).unwrap(),
        MaterialAppendMetadata {
            command_id: CommandId::new(cmd).unwrap(),
            scope: scope(),
            material_id: MaterialId::new(id).unwrap(),
            display_name: "proof.txt".into(),
            allowed_class_ids: vec![ClassId::new(1).unwrap()],
        },
    )
    .unwrap();
    f.backend.web_material_append(lease, proof).unwrap()
}
#[test]
fn material_http_metadata_delete_cas_fixed_version_and_historical_receipts() {
    let mut f = BackendFixture::new();
    catalog(&mut f, 1);
    let (cookie, csrf, lease) = auth(&mut f, "material-metadata");
    assert!(f
        .backend
        .web_material_authorize(f.admin, scope(), &[])
        .is_err());
    let empty = get(&mut f, &cookie, "materials/folders/1/1");
    expect(&empty, 200, "\"total\":0");
    assert_ne!(field(&empty.body, "folderId"), "0");
    let record = append(&mut f, lease, 71001, 72001);
    let page = get(&mut f, &cookie, "materials/folders/1/1");
    expect(&page, 200, "\"displayName\":\"proof.txt\"");
    expect(&page, 200, "\"kind\":\"TXT\"");
    expect(&page, 200, "\"allowedClassIds\":[\"1\"]");
    let remove = |cmd: u64, version: u64, rev: u64| {
        format!(
            r#"{{"schemaVersion":1,"commandId":"{cmd}","materialId":"72001","resourceVersionId":"{version}","expectedMaterialRevision":"{rev}"}}"#
        )
    };
    let route = "materials/folders/1/1/delete";
    let original = record.resource.version_id.get();
    expect(
        &post(
            &mut f,
            &cookie,
            &csrf,
            route,
            &remove(71002, original + 1, 1),
        ),
        404,
        "NOT_FOUND",
    );
    expect(
        &post(&mut f, &cookie, &csrf, route, &remove(71002, original, 2)),
        409,
        "REVISION_CONFLICT",
    );
    expect(
        &post(
            &mut f,
            &cookie,
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            route,
            &remove(71002, original, 1),
        ),
        403,
        "FORBIDDEN",
    );
    assert_eq!(
        f.backend
            .web_material_download_record(lease, scope(), record.id, record.resource.version_id)
            .unwrap()
            .revision,
        Revision::new(1)
    );
    let deleted = post(&mut f, &cookie, &csrf, route, &remove(71002, original, 1));
    expect(&deleted, 200, "\"status\":\"DELETED\"");
    expect(&deleted, 200, "\"revision\":\"2\"");
    expect(
        &post(&mut f, &cookie, &csrf, route, &remove(71002, original, 1)),
        200,
        "\"status\":\"DELETED\"",
    );
    expect(
        &post(
            &mut f,
            &cookie,
            &csrf,
            route,
            &remove(71002, original + 1, 1),
        ),
        409,
        "IDEMPOTENCY_CONFLICT",
    );
    expect(
        &get(&mut f, &cookie, "materials/commands/71001"),
        200,
        "\"status\":\"PUBLISHED\"",
    );
    expect(
        &get(&mut f, &cookie, "materials/commands/71002"),
        200,
        "\"status\":\"DELETED\"",
    );
    expect(
        &get(&mut f, &cookie, "materials/commands/71999"),
        200,
        "\"state\":\"NOT_FOUND\",\"scope\":null,\"record\":null",
    );
    assert!(f
        .backend
        .web_material_download_record(lease, scope(), record.id, record.resource.version_id)
        .is_err());
    expect(
        &get(&mut f, &cookie, "materials/folders/1/1"),
        200,
        "\"total\":0",
    );
}
#[test]
fn material_scopes_are_numeric_pages_and_original_device_checkpoint_never_rebinds() {
    let mut f = BackendFixture::new();
    catalog(&mut f, 22);
    let (cookie, _csrf, lease) = auth(&mut f, "material-original");
    let page = get(&mut f, &cookie, "materials/scopes/0");
    expect(&page, 200, "\"offset\":0,\"total\":22,\"nextOffset\":20");
    let value = text(&page.body);
    assert!(
        value.find("\"subjectId\":\"2\"").unwrap() < value.find("\"subjectId\":\"10\"").unwrap()
    );
    expect(
        &get(&mut f, &cookie, "materials/scopes/20"),
        200,
        "\"offset\":20,\"total\":22,\"nextOffset\":null",
    );
    expect(
        &get(&mut f, &cookie, "materials/scopes/22"),
        200,
        "\"items\":[]",
    );
    expect(
        &get(&mut f, &cookie, "materials/scopes/23"),
        400,
        "INVALID_REQUEST",
    );
    assert!(f
        .service
        .check_material_lease(lease, scope(), &[ClassId::new(1).unwrap()])
        .is_ok());
    assert!(f
        .service
        .check_material_lease(lease, scope(), &[ClassId::new(999).unwrap()])
        .is_err());
    f.backend.logout_account(lease).unwrap();
    f.admin = f
        .backend
        .login("admin", Password::new(PASSWORD.into()).unwrap())
        .unwrap();
    let (_cookie2, _csrf2, replacement) = auth(&mut f, "material-replacement");
    assert_ne!(lease, replacement);
    assert!(f.service.check_material_lease(lease, scope(), &[]).is_err());
    assert!(f
        .service
        .check_material_lease(replacement, scope(), &[])
        .is_ok());
}
#[test]
fn material_cursor_never_rebinds_to_mutated_folder_and_binary_slice_is_not_a_spool() {
    let mut f = BackendFixture::new();
    catalog(&mut f, 1);
    let (cookie, csrf, lease) = auth(&mut f, "material-cursor");
    let first = append(&mut f, lease, 73001, 74001);
    let before = get(&mut f, &cookie, "materials/folders/1/1");
    let folder = field(&before.body, "folderId");
    let revision = field(&before.body, "folderRevision");
    let _ = append(&mut f, lease, 73002, 74002);
    expect(
        &get(
            &mut f,
            &cookie,
            &format!("materials/folders/1/1/{folder}/{revision}/0"),
        ),
        409,
        "STALE_CURSOR",
    );
    let mut upload_head = head(
        "POST",
        "materials/uploads/1/bytes",
        1,
        Some(&cookie),
        Some(&csrf),
    );
    upload_head
        .headers
        .iter_mut()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .unwrap()
        .value = b"application/octet-stream".to_vec();
    expect(&f.dispatch(&upload_head, b"x"), 400, "INVALID_REQUEST");
    assert!(f
        .backend
        .web_material_download_record(
            lease,
            scope(),
            first.id,
            ResourceVersionId::new(first.resource.version_id.get() + 1).unwrap()
        )
        .is_err());
}

#[cfg(windows)]
#[test]
fn material_windows_upload_device_capacity_prepared_permit_cancel_and_consumed_receipt_replay() {
    let _scenario = super::super::shared_upload::test_guard();
    use std::{
        io::Cursor,
        sync::{Arc, Mutex},
    };
    let replacement_directory = Directory::new();
    let mut f = BackendFixture::new();
    catalog(&mut f, 1);
    let (cookie, csrf, owner_lease) = auth(&mut f, "stream-owner");
    let (foreign, foreign_csrf, _) = auth(&mut f, "stream-other-device");
    f.app.configure_material_files(&f.directory.0).unwrap();
    let replacement = BackendFixture::open(&replacement_directory, f.clock.clone());
    struct Shared(Arc<Mutex<Backend>>);
    impl Drop for Shared {
        fn drop(&mut self) {
            self.0.lock().unwrap().close().unwrap();
        }
    }
    let shared = Shared(Arc::new(Mutex::new(std::mem::replace(
        &mut f.backend,
        replacement,
    ))));
    let request = |method: &str, path: &str, body: &str| {
        f.app.dispatch(
            &mut shared.0.lock().unwrap(),
            &head(method, path, body.len(), Some(&cookie), Some(&csrf)),
            body.as_bytes(),
        )
    };
    let reservation = r#"{"schemaVersion":1,"displayName":"stream.txt","bytes":15}"#;
    let first = request("POST", "materials/uploads/1/1", reservation);
    expect(&first, 201, "UPLOAD_READY");
    let first_id = field(&first.body, "uploadId");
    let second = request("POST", "materials/uploads/1/1", reservation);
    expect(&second, 201, "UPLOAD_READY");
    let second_id = field(&second.body, "uploadId");
    expect(
        &request("POST", "materials/uploads/1/1", reservation),
        429,
        "CAPACITY",
    );
    let upload_head = |cookie: &str, csrf: &str, id: &str| {
        let mut h = head(
            "POST",
            &format!("materials/uploads/{id}/bytes"),
            15,
            Some(cookie),
            Some(csrf),
        );
        h.headers
            .iter_mut()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .unwrap()
            .value = b"application/octet-stream".to_vec();
        h
    };
    expect(
        &f.app.dispatch_material_upload(
            &upload_head(&foreign, &foreign_csrf, &first_id),
            Cursor::new(b"synthetic text\n"),
        ),
        403,
        "FORBIDDEN",
    );
    // The other device cannot consume/delete the owner's existing waiting bytes.
    expect(
        &f.app.dispatch_material_upload(
            &upload_head(&cookie, &csrf, &first_id),
            Cursor::new(b"synthetic text\n"),
        ),
        200,
        "UPLOADED",
    );
    expect(
        &f.app.dispatch_material_upload(
            &upload_head(&cookie, &csrf, &first_id),
            Cursor::new(b"synthetic text\n"),
        ),
        409,
        "UPLOAD_NOT_READY",
    );
    let append_body = format!(
        r#"{{"schemaVersion":1,"commandId":"75001","uploadId":"{first_id}","allowedClassIds":["1"]}}"#
    );
    let append_head = head(
        "POST",
        "materials/folders/1/1/append",
        append_body.len(),
        Some(&cookie),
        Some(&csrf),
    );
    let proof = match f
        .app
        .prepare_material_append(&append_head, append_body.as_bytes())
    {
        Ok(p) => p,
        Err(r) => panic!("{}", text(&r.body)),
    };
    expect(
        &request("POST", "materials/uploads/1/1", reservation),
        429,
        "CAPACITY",
    );
    expect(
        &request(
            "POST",
            &format!("materials/uploads/{first_id}/cancel"),
            r#"{"schemaVersion":1}"#,
        ),
        409,
        "UPLOAD_NOT_READY",
    );
    // The proof retains its actual file/permit through the SQL commit phase.
    let committed = f
        .app
        .commit_material_append(&mut shared.0.lock().unwrap(), proof);
    expect(&committed, 200, "COMMITTED");
    expect(
        &f.app
            .dispatch_material_append(&shared.0, &append_head, append_body.as_bytes()),
        200,
        "COMMITTED",
    );
    let changed = format!(
        r#"{{"schemaVersion":1,"commandId":"75001","uploadId":"{first_id}","allowedClassIds":["999"]}}"#
    );
    expect(
        &f.app.dispatch_material_append(
            &shared.0,
            &head(
                "POST",
                "materials/folders/1/1/append",
                changed.len(),
                Some(&cookie),
                Some(&csrf),
            ),
            changed.as_bytes(),
        ),
        409,
        "IDEMPOTENCY_CONFLICT",
    );
    // Deterministic real-writer barrier at the old vulnerable ordering: the
    // exact lease remains live while a delete publishes inside authorization.
    let (control, revision) = {
        let backend = shared.0.lock().unwrap();
        (
            backend.session_control(),
            backend.global_revision().unwrap(),
        )
    };
    let material = field(&committed.body, "materialId");
    let version = field(&committed.body, "resourceVersionId");
    let delete = format!(
        r#"{{"schemaVersion":1,"commandId":"75004","materialId":"{material}","resourceVersionId":"{version}","expectedMaterialRevision":"1"}}"#
    );
    let released = super::super::material_files::download_checkpoint(
        &control,
        revision,
        std::time::Instant::now() + Duration::from_secs(30),
        || {
            assert!(f
                .service
                .check_material_lease(owner_lease, scope(), &[])
                .is_ok());
            expect(
                &request("POST", "materials/folders/1/1/delete", &delete),
                200,
                "COMMITTED",
            );
            true
        },
    );
    assert!(
        !released,
        "a delete that finishes during authorization must cancel the old download revision"
    );
    assert!(f
        .service
        .check_material_lease(owner_lease, scope(), &[])
        .is_ok());
    let foreign_cancel = r#"{"schemaVersion":1}"#;
    let denied = f.app.dispatch(
        &mut shared.0.lock().unwrap(),
        &head(
            "POST",
            &format!("materials/uploads/{second_id}/cancel"),
            foreign_cancel.len(),
            Some(&foreign),
            Some(&foreign_csrf),
        ),
        foreign_cancel.as_bytes(),
    );
    expect(&denied, 403, "FORBIDDEN");
    expect(
        &request(
            "POST",
            &format!("materials/uploads/{second_id}/cancel"),
            foreign_cancel,
        ),
        200,
        "UPLOAD_CANCELLED",
    );
    expect(
        &request(
            "POST",
            &format!("materials/uploads/{second_id}/cancel"),
            foreign_cancel,
        ),
        409,
        "UPLOAD_EXPIRED",
    );
    let third = request("POST", "materials/uploads/1/1", reservation);
    expect(&third, 201, "UPLOAD_READY");
    let third_id = field(&third.body, "uploadId");
    // An owned short stream is rejected and its partial exclusive file is removed.
    expect(
        &f.app.dispatch_material_upload(
            &upload_head(&cookie, &csrf, &third_id),
            Cursor::new(b"short"),
        ),
        400,
        "INVALID_RESOURCE",
    );
    // Regression: revoked proofs still physically own their final inodes.
    // Sweeping their dead lease must not let a third physical owner enter.
    let mut pending_proofs = Vec::new();
    for cmd in [75002, 75003] {
        let ready = request("POST", "materials/uploads/1/1", reservation);
        expect(&ready, 201, "UPLOAD_READY");
        let id = field(&ready.body, "uploadId");
        expect(
            &f.app.dispatch_material_upload(
                &upload_head(&cookie, &csrf, &id),
                Cursor::new(b"synthetic text\n"),
            ),
            200,
            "UPLOADED",
        );
        let body = format!(
            r#"{{"schemaVersion":1,"commandId":"{cmd}","uploadId":"{id}","allowedClassIds":["1"]}}"#
        );
        let proof = match f.app.prepare_material_append(
            &head(
                "POST",
                "materials/folders/1/1/append",
                body.len(),
                Some(&cookie),
                Some(&csrf),
            ),
            body.as_bytes(),
        ) {
            Ok(p) => p,
            Err(r) => panic!("{}", text(&r.body)),
        };
        pending_proofs.push(proof);
    }
    let device = f
        .service
        .devices(f.admin)
        .unwrap()
        .into_iter()
        .find(|d| d.device_name == "stream-owner")
        .unwrap();
    f.service.revoke_device(f.admin, device.device_id).unwrap();
    f.app.sweep_material_uploads().unwrap();
    let foreign_request = |path: &str, body: &str| {
        f.app.dispatch(
            &mut shared.0.lock().unwrap(),
            &head(
                "POST",
                path,
                body.len(),
                Some(&foreign),
                Some(&foreign_csrf),
            ),
            body.as_bytes(),
        )
    };
    expect(
        &foreign_request("materials/uploads/1/1", reservation),
        429,
        "CAPACITY",
    );
    drop(pending_proofs.pop());
    let replacement = foreign_request("materials/uploads/1/1", reservation);
    expect(&replacement, 201, "UPLOAD_READY");
    let id = field(&replacement.body, "uploadId");
    expect(
        &foreign_request(&format!("materials/uploads/{id}/cancel"), foreign_cancel),
        200,
        "UPLOAD_CANCELLED",
    );
    drop(pending_proofs);
    f.app.sweep_material_uploads().unwrap();
    // A new registry cannot forget physical owners from the old application.
    let mut old_proofs = Vec::new();
    for cmd in [75005, 75006] {
        let ready = foreign_request("materials/uploads/1/1", reservation);
        expect(&ready, 201, "UPLOAD_READY");
        let id = field(&ready.body, "uploadId");
        expect(
            &f.app.dispatch_material_upload(
                &upload_head(&foreign, &foreign_csrf, &id),
                Cursor::new(b"synthetic text\n"),
            ),
            200,
            "UPLOADED",
        );
        let body = format!(
            r#"{{"schemaVersion":1,"commandId":"{cmd}","uploadId":"{id}","allowedClassIds":["1"]}}"#
        );
        let proof = match f.app.prepare_material_append(
            &head(
                "POST",
                "materials/folders/1/1/append",
                body.len(),
                Some(&foreign),
                Some(&foreign_csrf),
            ),
            body.as_bytes(),
        ) {
            Ok(p) => p,
            Err(r) => panic!("{}", text(&r.body)),
        };
        old_proofs.push(proof);
    }
    let replacement_app = WebApplication::new(
        &ListenPrefix::https("https://board.school:443/fc/").unwrap(),
        f.service.clone(),
    )
    .unwrap();
    let old_app = std::mem::replace(&mut f.app, replacement_app);
    drop(old_app);
    f.app.configure_material_files(&f.directory.0).unwrap();
    let replacement_request = |path: &str, body: &str| {
        f.app.dispatch(
            &mut shared.0.lock().unwrap(),
            &head(
                "POST",
                path,
                body.len(),
                Some(&foreign),
                Some(&foreign_csrf),
            ),
            body.as_bytes(),
        )
    };
    expect(
        &replacement_request("materials/uploads/1/1", reservation),
        429,
        "CAPACITY",
    );
    drop(old_proofs.pop());
    let ready = replacement_request("materials/uploads/1/1", reservation);
    expect(&ready, 201, "UPLOAD_READY");
    let id = field(&ready.body, "uploadId");
    expect(
        &replacement_request(&format!("materials/uploads/{id}/cancel"), foreign_cancel),
        200,
        "UPLOAD_CANCELLED",
    );
    drop(old_proofs);
    f.app.sweep_material_uploads().unwrap();
    let staging = f.directory.0.join("web-material-staging");
    assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 1); // owner marker only
}

#[test]
fn material_class_pages_preserve_all_classes_under_the_same_identity_revision() {
    let mut f = BackendFixture::new();
    catalog(&mut f, 1);
    for id in 2..=23 {
        f.backend
            .identity(
                f.admin,
                IdentityCommand::CreateClass {
                    id: ClassId::new(id).unwrap(),
                    name: format!("Synthetic authorized class {id}"),
                },
            )
            .unwrap();
    }
    f.backend
        .identity(
            f.admin,
            IdentityCommand::UpdateAccount {
                id: PrincipalId::new(1).unwrap(),
                display_name: "Synthetic material global".into(),
                role: Role::GlobalAdmin,
                enabled: true,
                scopes: (1..=23)
                    .rev()
                    .map(|id| AssignmentScope {
                        class_id: ClassId::new(id).unwrap(),
                        subject_id: SubjectId::new(1).unwrap(),
                    })
                    .collect(),
            },
        )
        .unwrap();
    f.admin = f
        .backend
        .login("admin", Password::new(PASSWORD.into()).unwrap())
        .unwrap();
    let (cookie, _, _) = auth(&mut f, "material-class-pages");
    let scopes = get(&mut f, &cookie, "materials/scopes/0");
    expect(&scopes, 200, "\"classTotal\":23,\"nextClassOffset\":20");
    assert_eq!(text(&scopes.body).matches("\"classId\"").count(), 20);
    let page = get(&mut f, &cookie, "materials/classes/1/1/20");
    expect(&page, 200, "\"status\":\"MATERIAL_CLASSES\"");
    expect(&page, 200, "\"offset\":20,\"total\":23,\"nextOffset\":null");
    assert_eq!(text(&page.body).matches("\"classId\"").count(), 3);
    assert!(text(&page.body).contains("\"classId\":\"21\""));
    assert_eq!(
        field(&scopes.body, "identityRevision"),
        field(&page.body, "identityRevision")
    );
    expect(
        &get(&mut f, &cookie, "materials/classes/1/1/23"),
        200,
        "\"items\":[]",
    );
    expect(
        &get(&mut f, &cookie, "materials/classes/1/1/24"),
        400,
        "INVALID_REQUEST",
    );
    expect(
        &get(&mut f, &cookie, "materials/classes/999/1/0"),
        403,
        "FORBIDDEN",
    );
}
