//! Real controlled file/SQLite worker port diagnostic; never listens or opens files in a Shell.
use super::*;
use fc_application::identity::SessionLease;
use fc_bootstrap::web_application::WebResponse;
use fc_domain::materials::*;
use fc_ports::MaterialListRequest;
use std::{
    io::{Cursor, Read},
    path::Path,
};
fn body(response: &WebResponse) -> &str {
    std::str::from_utf8(&response.body).unwrap_or("")
}
fn json(
    backend: &mut Backend,
    app: &WebApplication,
    method: &str,
    path: &str,
    cookie: &str,
    csrf: &str,
    bytes: &[u8],
) -> Result<WebResponse, String> {
    Ok(app.dispatch(
        backend,
        &head(method, path, bytes, Some(cookie), Some(csrf))?,
        bytes,
    ))
}
// Diagnostic inputs stay explicit; this is not a product dispatch API.
#[allow(clippy::too_many_arguments)]
fn upload(
    backend: &mut Backend,
    app: &WebApplication,
    scope: MaterialScope,
    cookie: &str,
    csrf: &str,
    name: &str,
    bytes: u64,
    reader: impl Read,
) -> Result<u64, String> {
    let route = format!(
        "materials/uploads/{}/{}",
        scope.teacher_id.get(),
        scope.subject_id.get()
    );
    let input = format!(r#"{{"schemaVersion":1,"displayName":"{name}","bytes":{bytes}}}"#);
    let created = json(backend, app, "POST", &route, cookie, csrf, input.as_bytes())?;
    require(
        created.head.status == 201,
        "material upload reservation failed",
    )?;
    let id = body(&created)
        .split("\"uploadId\":\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or("uploadId absent")?;
    require(id != 0, "uploadId zero")?;
    let mut request = head(
        "POST",
        &format!("materials/uploads/{id}/bytes"),
        &[],
        Some(cookie),
        Some(csrf),
    )?;
    request.content_length = Some(bytes);
    request.has_body = true;
    for h in &mut request.headers {
        if h.name == "Content-Type" {
            h.value = b"application/octet-stream".to_vec();
        }
        if h.name == "Content-Length" {
            h.value = bytes.to_string().into_bytes();
        }
    }
    let uploaded = app.dispatch_material_upload(&request, reader);
    require(
        uploaded.head.status == 200 && body(&uploaded).contains("\"UPLOADED\""),
        "controlled material upload failed",
    )?;
    Ok(id)
}
fn append(
    backend: &mut Backend,
    app: &WebApplication,
    scope: MaterialScope,
    cookie: &str,
    csrf: &str,
    upload: u64,
    command: u64,
) -> Result<MaterialRecord, String> {
    let route = format!(
        "materials/folders/{}/{}/append",
        scope.teacher_id.get(),
        scope.subject_id.get()
    );
    let input = format!(
        r#"{{"schemaVersion":1,"commandId":"{command}","uploadId":"{upload}","allowedClassIds":["7"]}}"#
    );
    let request = head("POST", &route, input.as_bytes(), Some(cookie), Some(csrf))?;
    let proof = app
        .prepare_material_append(&request, input.as_bytes())
        .map_err(|r| format!("material prepare status {}: {}", r.head.status, body(&r)))?;
    let committed = app.commit_material_append(backend, proof);
    require(
        committed.head.status == 200 && body(&committed).contains("\"COMMITTED\""),
        "prepared material commit failed",
    )?;
    let page = check(backend.web_materials(
        lease_from_cookie(app, backend, cookie)?,
        MaterialListRequest {
            scope,
            cursor: None,
        },
    ))?;
    let record = page
        .items
        .first()
        .cloned()
        .ok_or("committed material absent")?;
    require(
        record.resource.version_id.get() == upload,
        "committed upload fixed version mismatch",
    )?;
    let result = json(
        backend,
        app,
        "GET",
        &format!("materials/commands/{command}"),
        cookie,
        csrf,
        &[],
    )?;
    require(
        result.head.status == 200 && body(&result).contains("\"COMMITTED\""),
        "original material receipt unavailable",
    )?;
    Ok(record)
}
fn lease_from_cookie(
    _: &WebApplication,
    backend: &Backend,
    cookie: &str,
) -> Result<SessionLease, String> {
    let raw = cookie.split_once('=').ok_or("invalid own cookie")?.1;
    let secret = check(fc_security::web::WebSecret::parse_hex(raw.to_owned()))?;
    Ok(check(backend.web_service().authenticate(&secret))?.lease())
}
fn download(
    backend: &Backend,
    app: &WebApplication,
    scope: MaterialScope,
    record: &MaterialRecord,
    cookie: &str,
    csrf: &str,
) -> Result<fc_bootstrap::web_application::MaterialDownload, String> {
    let path = format!(
        "materials/download/{}/{}/{}/{}",
        scope.teacher_id.get(),
        scope.subject_id.get(),
        record.id.get(),
        record.resource.version_id.get()
    );
    app.begin_material_download(backend, &head("GET", &path, &[], Some(cookie), Some(csrf))?)
        .map_err(|r| format!("material download status {}", r.head.status))
}
pub(super) fn run(
    backend: &mut Backend,
    app: &mut WebApplication,
    root: &Path,
    cookie: &str,
    csrf: &str,
    lease: SessionLease,
) -> Result<(), String> {
    let files = root.join("web-material-files");
    check(fs::create_dir(&files))?;
    check(app.configure_material_files(&files))?;
    let scope = MaterialScope {
        teacher_id: lease.principal_id(),
        subject_id: check(SubjectId::new(9))?,
    };
    let scopes = json(backend, app, "GET", "materials/scopes/0", cookie, csrf, &[])?;
    require(
        scopes.head.status == 200 && body(&scopes).contains("Synthetic content subject"),
        "material scopes unavailable",
    )?;
    let folder = format!("materials/folders/{}/9", lease.principal_id().get());
    let page = json(backend, app, "GET", &folder, cookie, csrf, &[])?;
    require(page.head.status == 200, "material empty folder unavailable")?;
    let wrong = if csrf.starts_with('a') {
        "b".repeat(64)
    } else {
        "a".repeat(64)
    };
    let reservation = br#"{"schemaVersion":1,"displayName":"synthetic.txt","bytes":3}"#;
    let rejected = json(
        backend,
        app,
        "POST",
        &format!("materials/uploads/{}/9", scope.teacher_id.get()),
        cookie,
        &wrong,
        reservation,
    )?;
    require(
        rejected.head.status == 403,
        "material reservation ignored CSRF",
    )?;
    let samples = [
        (
            "synthetic.txt",
            b"Owned synthetic classroom material\n".to_vec(),
        ),
        ("synthetic.docx", crate::application::material_probe::docx()),
        ("synthetic.bmp", crate::application::material_probe::bmp()),
        (
            "synthetic.pdf",
            crate::application::material_probe::pdf(false),
        ),
    ];
    for (i, (name, bytes)) in samples.into_iter().enumerate() {
        let id = upload(
            backend,
            app,
            scope,
            cookie,
            csrf,
            name,
            bytes.len() as u64,
            Cursor::new(bytes.clone()),
        )?;
        let record = append(backend, app, scope, cookie, csrf, id, 910001 + i as u64)?;
        let mut stream = download(backend, app, scope, &record, cookie, csrf)?;
        require(
            stream.head.content_length == bytes.len() as u64,
            "download length differs",
        )?;
        let mut actual = Vec::new();
        loop {
            let chunk = check(stream.read_chunk_without_backend())?;
            require(chunk.len() <= 65536, "download chunk exceeds bound")?;
            if chunk.is_empty() {
                break;
            }
            actual.extend_from_slice(chunk);
        }
        require(
            actual == bytes,
            "download bytes differ from validated input",
        )?;
    }
    // Full 50MiB boundary: the fixture and transport keep only one 64KiB block;
    // no whole original Vec is built by this diagnostic or production adapter.
    let size = MAX_MATERIAL_BYTES;
    let id = upload(
        backend,
        app,
        scope,
        cookie,
        csrf,
        "synthetic-50MiB.txt",
        size,
        std::io::repeat(b'a').take(size),
    )?;
    let record = append(backend, app, scope, cookie, csrf, id, 910010)?;
    let mut stream = download(backend, app, scope, &record, cookie, csrf)?;
    let mut total = 0u64;
    loop {
        let chunk = check(stream.read_chunk_without_backend())?;
        require(
            chunk.len() <= 65536 && chunk.iter().all(|b| *b == b'a'),
            "large download corrupt or overbound",
        )?;
        if chunk.is_empty() {
            break;
        }
        total += chunk.len() as u64;
    }
    require(total == size, "50MiB stream truncated")?;
    drop(stream);
    let mut pending = download(backend, app, scope, &record, cookie, csrf)?;
    require(
        !check(pending.read_chunk_without_backend())?.is_empty(),
        "download did not start",
    )?;
    let delete_route = format!("{folder}/delete");
    let deletion = |expected: u64, command: u64| {
        format!(
            r#"{{"schemaVersion":1,"commandId":"{command}","materialId":"{}","resourceVersionId":"{}","expectedMaterialRevision":"{expected}"}}"#,
            record.id.get(),
            record.resource.version_id.get()
        )
    };
    let stale = deletion(record.revision.get() + 1, 910020);
    let response = json(
        backend,
        app,
        "POST",
        &delete_route,
        cookie,
        csrf,
        stale.as_bytes(),
    )?;
    require(
        response.head.status == 409,
        "stale material delete was accepted",
    )?;
    let request = deletion(record.revision.get(), 910021);
    let response = json(
        backend,
        app,
        "POST",
        &delete_route,
        cookie,
        csrf,
        request.as_bytes(),
    )?;
    require(
        response.head.status == 200 && body(&response).contains("\"DELETED\""),
        "fixed material delete failed",
    )?;
    require(
        pending.read_chunk_without_backend().is_err(),
        "deleted material kept streaming",
    )?;
    drop(pending);
    let revision = check(backend.global_revision())?;
    let replay = json(
        backend,
        app,
        "POST",
        &delete_route,
        cookie,
        csrf,
        request.as_bytes(),
    )?;
    require(
        replay.head.status == 200 && revision == check(backend.global_revision())?,
        "delete replay wrote twice",
    )?;
    let missing = json(
        backend,
        app,
        "GET",
        "materials/commands/919999",
        cookie,
        csrf,
        &[],
    )?;
    require(
        missing.head.status == 200 && body(&missing).contains("\"NOT_FOUND\""),
        "unknown material command leaked",
    )?;
    let id = upload(
        backend,
        app,
        scope,
        cookie,
        csrf,
        "malformed.bmp",
        3,
        Cursor::new(b"bad"),
    )?;
    let input = format!(
        r#"{{"schemaVersion":1,"commandId":"910030","uploadId":"{id}","allowedClassIds":["7"]}}"#
    );
    let request = head(
        "POST",
        &format!("{folder}/append"),
        input.as_bytes(),
        Some(cookie),
        Some(csrf),
    )?;
    require(
        app.prepare_material_append(&request, input.as_bytes())
            .is_err(),
        "malformed WIC image accepted",
    )?;
    let id = upload(
        backend,
        app,
        scope,
        cookie,
        csrf,
        "active.pdf",
        crate::application::material_probe::pdf(true).len() as u64,
        Cursor::new(crate::application::material_probe::pdf(true)),
    )?;
    let input = format!(
        r#"{{"schemaVersion":1,"commandId":"910031","uploadId":"{id}","allowedClassIds":["7"]}}"#
    );
    let request = head(
        "POST",
        &format!("{folder}/append"),
        input.as_bytes(),
        Some(cookie),
        Some(csrf),
    )?;
    require(
        app.prepare_material_append(&request, input.as_bytes())
            .is_err(),
        "active PDF accepted",
    )?;
    println!("WEB_MATERIALS_PASS real_DPAPI_SQLite_controlled_files_TXT_Office_WIC_PDFhelper_50MiB_upload_download_64KiB_chunks_CSRF_CAS_delete_receipt_delete_invalidates_stream=PASS synthetic_request_metadata=true listener_TLS_browser=NOT_RUN network_Read_deadline=NOT_VALIDATED");
    Ok(())
}
