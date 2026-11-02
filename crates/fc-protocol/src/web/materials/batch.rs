//! Ordered group metadata, never a file or execution capability. New mutations
//! require C2 explicitly and retain the legacy single-file boundary unchanged.
use super::*;

#[derive(Debug, PartialEq, Eq)]
pub struct BatchFileInput {
    pub display_name: String,
    pub bytes: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub struct BatchUploadInput {
    pub files: Vec<BatchFileInput>,
    pub allowed_class_ids: Vec<ClassId>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct BatchAppendInput {
    pub command_id: CommandId,
    pub command_ticket: CommandTicket,
    pub batch_id: u64,
    pub upload_ids: Vec<u64>,
    pub allowed_class_ids: Vec<ClassId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FileDto {
    display_name: String,
    bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct UploadDto {
    schema_version: u16,
    files: Vec<FileDto>,
    allowed_class_ids: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AppendDto {
    schema_version: u16,
    command_id: String,
    command_ticket: String,
    batch_id: String,
    upload_ids: Vec<String>,
    allowed_class_ids: Vec<String>,
}
fn count(length: usize) -> Result<(), ProtocolError> {
    if (1..=MAX_MATERIAL_BATCH_FILES).contains(&length) {
        Ok(())
    } else {
        Err(ProtocolError::InvalidCommand)
    }
}
fn file_metadata(name: &str, bytes: u64) -> Result<(), ProtocolError> {
    validate_material_name(name).map_err(|_| ProtocolError::InvalidCommand)?;
    let kind = MaterialKind::from_name(name).ok_or(ProtocolError::InvalidCommand)?;
    if bytes == 0
        || bytes > MAX_MATERIAL_BYTES
        || matches!(
            kind,
            MaterialKind::Png | MaterialKind::Jpeg | MaterialKind::Bmp
        ) && bytes > 10 * 1024 * 1024
    {
        return Err(ProtocolError::TooLarge);
    }
    Ok(())
}
fn classes(values: Vec<String>) -> Result<Vec<ClassId>, ProtocolError> {
    if values.is_empty() || values.len() > fc_domain::identity::MAX_CLASSES {
        return Err(ProtocolError::InvalidCommand);
    }
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        let value = ClassId::new(id(&value).ok_or(ProtocolError::InvalidCommand)?)
            .map_err(|_| ProtocolError::InvalidCommand)?;
        if result.contains(&value) {
            return Err(ProtocolError::InvalidCommand);
        }
        result.push(value);
    }
    Ok(result)
}
pub fn batch_upload(body: &[u8]) -> Result<BatchUploadInput, ProtocolError> {
    let dto: UploadDto = parse(body)?;
    schema(dto.schema_version)?;
    count(dto.files.len())?;
    let allowed_class_ids = classes(dto.allowed_class_ids)?;
    let mut files = Vec::with_capacity(dto.files.len());
    for file in dto.files {
        file_metadata(&file.display_name, file.bytes)?;
        // Same names intentionally remain separate entries, in confirmed order.
        files.push(BatchFileInput {
            display_name: file.display_name,
            bytes: file.bytes,
        });
    }
    Ok(BatchUploadInput {
        files,
        allowed_class_ids,
    })
}
pub fn batch_append(body: &[u8]) -> Result<BatchAppendInput, ProtocolError> {
    let dto: AppendDto = parse(body)?;
    if dto.schema_version != 2 {
        return Err(ProtocolError::UnsupportedSchema);
    }
    count(dto.upload_ids.len())?;
    let command_ticket =
        CommandTicket::parse(&dto.command_ticket).map_err(|_| ProtocolError::InvalidCommand)?;
    let mut upload_ids = Vec::with_capacity(dto.upload_ids.len());
    for value in dto.upload_ids {
        let value = id(&value).ok_or(ProtocolError::InvalidCommand)?;
        if upload_ids.contains(&value) {
            return Err(ProtocolError::InvalidCommand);
        }
        upload_ids.push(value);
    }
    Ok(BatchAppendInput {
        command_id: CommandId::new(id(&dto.command_id).ok_or(ProtocolError::InvalidCommand)?)
            .map_err(|_| ProtocolError::InvalidCommand)?,
        command_ticket,
        batch_id: id(&dto.batch_id).ok_or(ProtocolError::InvalidCommand)?,
        upload_ids,
        allowed_class_ids: classes(dto.allowed_class_ids)?,
    })
}

pub struct BatchReadyMember<'a> {
    pub upload_id: u64,
    pub display_name: &'a str,
    pub bytes: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadyMember<'a> {
    upload_id: String,
    display_name: &'a str,
    bytes: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Ready<'a> {
    schema_version: u16,
    status: &'static str,
    batch_id: String,
    expires_in_seconds: u16,
    max_chunk_bytes: u32,
    items: Vec<ReadyMember<'a>>,
}
pub fn batch_ready(
    batch_id: u64,
    members: &[BatchReadyMember<'_>],
) -> Result<Vec<u8>, ProtocolError> {
    count(members.len())?;
    if batch_id == 0 {
        return Err(ProtocolError::InvalidCommand);
    }
    let mut items = Vec::with_capacity(members.len());
    for (index, member) in members.iter().enumerate() {
        if member.upload_id == 0
            || members[..index]
                .iter()
                .any(|prior| prior.upload_id == member.upload_id)
        {
            return Err(ProtocolError::InvalidCommand);
        }
        file_metadata(member.display_name, member.bytes)?;
        items.push(ReadyMember {
            upload_id: member.upload_id.to_string(),
            display_name: member.display_name,
            bytes: member.bytes,
        });
    }
    super::super::responses::encode(&Ready {
        schema_version: 1,
        status: "MATERIAL_BATCH_READY",
        batch_id: batch_id.to_string(),
        expires_in_seconds: 300,
        max_chunk_bytes: 65536,
        items,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Uploaded {
    schema_version: u16,
    status: &'static str,
    batch_id: String,
    upload_id: String,
    bytes: u64,
}
pub fn batch_member_uploaded(
    batch_id: u64,
    upload_id: u64,
    bytes: u64,
) -> Result<Vec<u8>, ProtocolError> {
    if batch_id == 0 || upload_id == 0 || bytes == 0 || bytes > MAX_MATERIAL_BYTES {
        return Err(ProtocolError::InvalidCommand);
    }
    super::super::responses::encode(&Uploaded {
        schema_version: 1,
        status: "MATERIAL_BATCH_MEMBER_UPLOADED",
        batch_id: batch_id.to_string(),
        upload_id: upload_id.to_string(),
        bytes,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Cancelled {
    schema_version: u16,
    status: &'static str,
    batch_id: String,
}
pub fn batch_cancelled(batch_id: u64) -> Result<Vec<u8>, ProtocolError> {
    if batch_id == 0 {
        return Err(ProtocolError::InvalidCommand);
    }
    super::super::responses::encode(&Cancelled {
        schema_version: 1,
        status: "MATERIAL_BATCH_CANCELLED",
        batch_id: batch_id.to_string(),
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Committed<'a> {
    schema_version: u16,
    status: &'static str,
    command_id: String,
    command_ticket: String,
    scope: Scope,
    records: Vec<Record<'a>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct History<'a> {
    schema_version: u16,
    status: &'static str,
    command_id: String,
    command_ticket: String,
    state: &'static str,
    scope: Option<Scope>,
    records: Option<Vec<Record<'a>>>,
}
fn committed(
    command: CommandReference,
    scope: MaterialScope,
    records: Vec<Record<'_>>,
) -> Result<Vec<u8>, ProtocolError> {
    super::super::responses::encode(&Committed {
        schema_version: 2,
        status: "BATCH_COMMITTED",
        command_id: command.command_id.get().to_string(),
        command_ticket: command.ticket.to_string(),
        scope: scope.into(),
        records,
    })
}
fn history(
    command: CommandReference,
    result: Option<(MaterialScope, Vec<Record<'_>>)>,
) -> Result<Vec<u8>, ProtocolError> {
    let (scope, records) = match result {
        Some((scope, records)) => (Some(scope.into()), Some(records)),
        None => (None, None),
    };
    super::super::responses::encode(&History {
        schema_version: 2,
        status: "COMMAND_BATCH_RESULT",
        command_id: command.command_id.get().to_string(),
        command_ticket: command.ticket.to_string(),
        state: if records.is_some() {
            "COMMITTED"
        } else {
            "NOT_FOUND"
        },
        scope,
        records,
    })
}
fn valid_records(records: &[MaterialRecord]) -> Result<(), ProtocolError> {
    count(records.len())?;
    let first = &records[0];
    for (index, item) in records.iter().enumerate() {
        item.validate().map_err(|_| ProtocolError::InvalidCommand)?;
        if item.status != MaterialStatus::Published
            || item.revision.get() != 1
            || item.folder_id != first.folder_id
            || item.added_at != first.added_at
            || item.allowed_class_ids != first.allowed_class_ids
            || records[..index].iter().any(|prior| {
                prior.id == item.id || prior.resource.version_id == item.resource.version_id
            })
            || index > 0
                && records[index - 1].added_sequence.checked_add(1) != Some(item.added_sequence)
        {
            return Err(ProtocolError::InvalidCommand);
        }
    }
    Ok(())
}
pub fn batch_committed(
    command: CommandReference,
    scope: MaterialScope,
    records: &[MaterialRecord],
) -> Result<Vec<u8>, ProtocolError> {
    valid_records(records)?;
    committed(command, scope, records.iter().map(record).collect())
}
pub fn batch_command_window(
    command: CommandReference,
    result: Option<(MaterialScope, &[MaterialRecord])>,
) -> Result<Vec<u8>, ProtocolError> {
    if let Some((_, records)) = result {
        valid_records(records)?;
    }
    history(
        command,
        result.map(|(scope, records)| (scope, records.iter().map(record).collect())),
    )
}

/// Conservative, pre-COMMIT response/history budget. This checks bytes that are
/// already prepared; it does no file I/O, auth, issuance, or Writer admission.
/// Unknown IDs/folder/sequence/ticket width use maximum legal decimal lengths.
pub fn batch_result_fits(
    scope: MaterialScope,
    appends: &[MaterialAppend],
) -> Result<(), ProtocolError> {
    count(appends.len())?;
    for (index, append) in appends.iter().enumerate() {
        append
            .validate()
            .map_err(|_| ProtocolError::InvalidCommand)?;
        if append.allowed_class_ids != appends[0].allowed_class_ids
            || appends[..index].iter().any(|prior| {
                prior.material_id == append.material_id
                    || prior.resource.version_id == append.resource.version_id
            })
        {
            return Err(ProtocolError::InvalidCommand);
        }
    }
    let command = CommandReference {
        command_id: CommandId::new(u64::MAX).map_err(|_| ProtocolError::InvalidCommand)?,
        ticket: CommandTicket::new([0xff; 16], i64::MAX as u64)
            .map_err(|_| ProtocolError::InvalidCommand)?,
    };
    let worst_records = || {
        appends
            .iter()
            .map(|append| Record {
                material_id: u64::MAX.to_string(),
                folder_id: u64::MAX.to_string(),
                display_name: &append.display_name,
                resource_version_id: u64::MAX.to_string(),
                kind: match append.resource.kind {
                    MaterialKind::Png => "PNG",
                    MaterialKind::Jpeg => "JPEG",
                    MaterialKind::Bmp => "BMP",
                    MaterialKind::Txt => "TXT",
                    MaterialKind::Pdf => "PDF",
                    MaterialKind::Pptx => "PPTX",
                    MaterialKind::Docx => "DOCX",
                    MaterialKind::Xlsx => "XLSX",
                },
                bytes: append.resource.bytes,
                sha256: append
                    .resource
                    .sha256
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                allowed_class_ids: append
                    .allowed_class_ids
                    .iter()
                    .map(|id| id.get().to_string())
                    .collect(),
                added_at: "9999-12-31T23:59:59".into(),
                added_sequence: u64::MAX.to_string(),
                status: "PUBLISHED",
                revision: "1".into(),
            })
            .collect()
    };
    committed(command, scope, worst_records())?;
    history(command, Some((scope, worst_records())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::AllowedMethods;
    use fc_domain::time::{LocalDate, LocalDateTime};
    use serde_json::{json, Value};

    fn scope() -> MaterialScope {
        MaterialScope {
            teacher_id: PrincipalId::new(u64::MAX).unwrap(),
            subject_id: SubjectId::new(u64::MAX - 1).unwrap(),
        }
    }
    fn reference() -> CommandReference {
        CommandReference {
            command_id: CommandId::new(u64::MAX).unwrap(),
            ticket: CommandTicket::new([0xaf; 16], i64::MAX as u64).unwrap(),
        }
    }
    fn request() -> Value {
        json!({"schemaVersion":2,"commandId":u64::MAX.to_string(),"commandTicket":reference().ticket.to_string(),
               "batchId":u64::MAX.to_string(),"uploadIds":["9","3"],"allowedClassIds":["7","2"]})
    }
    fn record(id: u64) -> MaterialRecord {
        MaterialRecord {
            id: MaterialId::new(id).unwrap(),
            folder_id: FolderId::new(u64::MAX).unwrap(),
            display_name: format!("同名课堂资料-{id}.txt"),
            resource: MaterialResource {
                version_id: ResourceVersionId::new(100 + id).unwrap(),
                kind: MaterialKind::Txt,
                bytes: 17,
                sha256: [7; 32],
            },
            allowed_class_ids: vec![ClassId::new(7).unwrap(), ClassId::new(2).unwrap()],
            added_at: LocalDateTime::new(LocalDate::new(9999, 12, 31).unwrap(), 86399).unwrap(),
            added_sequence: u64::MAX - 20 + id,
            status: MaterialStatus::Published,
            revision: Revision::new(1),
        }
    }
    fn append(record: &MaterialRecord) -> MaterialAppend {
        MaterialAppend {
            material_id: record.id,
            display_name: record.display_name.clone(),
            resource: record.resource.clone(),
            allowed_class_ids: record.allowed_class_ids.clone(),
        }
    }
    #[test]
    fn batch_routes_have_canonical_ids_exact_method_and_bounded_binary_body() {
        let ticket = reference().ticket;
        for (path, method, body) in [
            (
                "batches/1/2".into(),
                AllowedMethods::Post,
                crate::MAX_JSON_BYTES as u64,
            ),
            (
                format!("batches/{}/files/3/bytes", u64::MAX),
                AllowedMethods::Post,
                MAX_MATERIAL_BYTES,
            ),
            (
                "batches/1/cancel".into(),
                AllowedMethods::Post,
                crate::MAX_JSON_BYTES as u64,
            ),
            (
                "folders/1/2/append-batch".into(),
                AllowedMethods::Post,
                crate::MAX_JSON_BYTES as u64,
            ),
            (
                format!("batch-commands/1/2/{ticket}/{}", u64::MAX),
                AllowedMethods::Get,
                0,
            ),
        ] {
            let route = super::super::route(&path).unwrap();
            assert_eq!(route.allowed_methods(), method, "{path}");
            assert_eq!(route.body_limit(), body, "{path}");
        }
        for path in [
            "batches/01/2".into(),
            "batches/1/files/0/bytes".into(),
            "batches/0/cancel".into(),
            "batches/1/2/extra".into(),
            "folders/1/0/append-batch".into(),
            format!("batch-commands/1/2/{ticket}/01"),
            format!("batch-commands/1/2/{ticket}/1?retry=1"),
            "batch-commands/1/2/7".into(),
            format!("batch-commands/1/2/{}/1", ticket.to_string().to_uppercase()),
        ] {
            assert!(super::super::route(&path).is_none(), "{path}");
        }
        // The legacy endpoints retain their separate schemas and method gates.
        assert!(matches!(
            super::super::route("uploads/1/2"),
            Some(MaterialRoute::UploadCreate { .. })
        ));
    }
    #[test]
    fn create_preserves_twenty_ordered_duplicate_names_but_rejects_bad_manifest_metadata() {
        let input = json!({"schemaVersion":1,"files":(1..=20).map(|n|json!({"displayName":"同名.txt","bytes":n})).collect::<Vec<_>>(),"allowedClassIds":["9","3"]});
        let parsed = batch_upload(&serde_json::to_vec(&input).unwrap()).unwrap();
        assert_eq!(
            parsed
                .files
                .iter()
                .map(|file| file.bytes)
                .collect::<Vec<_>>(),
            (1..=20).collect::<Vec<_>>()
        );
        assert_eq!(
            parsed
                .allowed_class_ids
                .iter()
                .map(|id| id.get())
                .collect::<Vec<_>>(),
            [9, 3]
        );
        for files in [
            json!([]),
            json!((0..21)
                .map(|_| json!({"displayName":"x.txt","bytes":1}))
                .collect::<Vec<_>>()),
            json!([{"displayName":"../foreign.txt","bytes":1}]),
            json!([{"displayName":"x.png","bytes":10485761}]),
            json!([{"displayName":"x.txt","bytes":0}]),
            json!([{"displayName":"x.txt","bytes":1.5}]),
            json!([{"displayName":"x.txt","bytes":1,"sha256":"pretend-proof"}]),
        ] {
            let mut bad = input.clone();
            bad["files"] = files;
            assert!(batch_upload(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        for ids in [
            json!([]),
            json!(["9", "9"]),
            json!(["0"]),
            json!(["01"]),
            json!([1]),
        ] {
            let mut bad = input.clone();
            bad["allowedClassIds"] = ids;
            assert!(batch_upload(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        assert!(batch_upload(br#"{"schemaVersion":1,"files":[{"displayName":"x.txt","bytes":1,"bytes":2}],"allowedClassIds":["1"]}"#).is_err());
    }
    #[test]
    fn append_is_c2_only_and_preserves_full_u64_and_confirmed_order_without_fallback() {
        let original = request();
        let parsed = batch_append(&serde_json::to_vec(&original).unwrap()).unwrap();
        assert_eq!(parsed.command_id.get(), u64::MAX);
        assert_eq!(parsed.batch_id, u64::MAX);
        assert_eq!(parsed.command_ticket, reference().ticket);
        assert_eq!(parsed.upload_ids, [9, 3]);
        for (key, value) in [
            ("schemaVersion", json!(1)),
            ("commandTicket", Value::Null),
            ("commandTicket", json!("7")),
            ("commandId", json!("01")),
            ("batchId", json!("0")),
            ("uploadIds", json!(["9", "9"])),
            ("uploadIds", json!([])),
            ("uploadIds", json!(["01"])),
            ("allowedClassIds", json!(["7", "7"])),
            ("actorId", json!("1")),
        ] {
            let mut bad = original.clone();
            bad[key] = value;
            assert!(
                batch_append(&serde_json::to_vec(&bad).unwrap()).is_err(),
                "{key}"
            );
        }
        let mut missing = original;
        missing.as_object_mut().unwrap().remove("commandTicket");
        assert!(batch_append(&serde_json::to_vec(&missing).unwrap()).is_err());
        assert!(batch_append(&vec![b' '; crate::MAX_JSON_BYTES + 1]).is_err());
    }
    #[test]
    fn ready_member_cancel_and_history_envelopes_keep_group_and_original_pair_separate() {
        let ready = batch_ready(
            u64::MAX,
            &[BatchReadyMember {
                upload_id: 9,
                display_name: "x.txt",
                bytes: 7,
            }],
        )
        .unwrap();
        let ready: Value = serde_json::from_slice(&ready).unwrap();
        assert_eq!(ready["batchId"], u64::MAX.to_string());
        assert_eq!(ready["items"][0]["uploadId"], "9");
        assert_eq!(ready["status"], "MATERIAL_BATCH_READY");
        assert_eq!(ready["maxChunkBytes"], 65536);
        assert!(batch_ready(
            1,
            &[
                BatchReadyMember {
                    upload_id: 9,
                    display_name: "x.txt",
                    bytes: 7
                },
                BatchReadyMember {
                    upload_id: 9,
                    display_name: "y.txt",
                    bytes: 8
                }
            ]
        )
        .is_err());
        let uploaded: Value =
            serde_json::from_slice(&batch_member_uploaded(2, 9, 7).unwrap()).unwrap();
        assert_eq!(uploaded["batchId"], "2");
        assert_eq!(uploaded["status"], "MATERIAL_BATCH_MEMBER_UPLOADED");
        assert!(batch_cancelled(0).is_err());
        let cancelled: Value = serde_json::from_slice(&batch_cancelled(2).unwrap()).unwrap();
        assert_eq!(cancelled["status"], "MATERIAL_BATCH_CANCELLED");
        let absent: Value =
            serde_json::from_slice(&batch_command_window(reference(), None).unwrap()).unwrap();
        assert_eq!(absent["schemaVersion"], 2);
        assert_eq!(absent["status"], "COMMAND_BATCH_RESULT");
        assert_eq!(absent["state"], "NOT_FOUND");
        assert_eq!(absent["commandId"], u64::MAX.to_string());
        assert_eq!(absent["commandTicket"], reference().ticket.to_string());
        assert!(absent["scope"].is_null() && absent["records"].is_null());
        let records = (1..=20).map(record).collect::<Vec<_>>();
        let saved: Value =
            serde_json::from_slice(&batch_committed(reference(), scope(), &records).unwrap())
                .unwrap();
        assert_eq!(saved["status"], "BATCH_COMMITTED");
        assert_eq!(saved["records"][19]["addedSequence"], u64::MAX.to_string());
        let history: Value = serde_json::from_slice(
            &batch_command_window(reference(), Some((scope(), &records))).unwrap(),
        )
        .unwrap();
        assert_eq!(history["records"], saved["records"]);
        assert_eq!(history["scope"], saved["scope"]);
    }
    #[test]
    fn history_rejects_mixed_or_reordered_results_and_budget_rejects_before_commit() {
        let original = (1..=20).map(record).collect::<Vec<_>>();
        assert!(
            batch_result_fits(scope(), &original.iter().map(append).collect::<Vec<_>>()).is_ok()
        );
        let mut broken = original.clone();
        broken.swap(0, 1);
        assert!(batch_committed(reference(), scope(), &broken).is_err());
        let mut broken = original.clone();
        broken[1].added_at = LocalDateTime::new(LocalDate::new(2026, 11, 2).unwrap(), 0).unwrap();
        assert!(batch_command_window(reference(), Some((scope(), &broken))).is_err());
        let mut broken = original.clone();
        broken[1].resource.version_id = broken[0].resource.version_id;
        assert!(batch_committed(reference(), scope(), &broken).is_err());
        let mut broken = original;
        broken[0].status = MaterialStatus::Deleted;
        assert!(batch_committed(reference(), scope(), &broken).is_err());
        let mut large = (1..=20)
            .map(record)
            .map(|record| append(&record))
            .collect::<Vec<_>>();
        for item in &mut large {
            item.display_name = format!("{}.txt", "资".repeat(176));
            item.allowed_class_ids = (0..128)
                .map(|n| ClassId::new(u64::MAX - n).unwrap())
                .collect();
            assert_eq!(item.display_name.chars().count(), 180);
            item.validate().unwrap();
        }
        assert_eq!(
            batch_result_fits(scope(), &large),
            Err(ProtocolError::TooLarge)
        );
        large.truncate(1);
        assert!(batch_result_fits(scope(), &large).is_ok());
    }
    #[test]
    fn conservative_precommit_bound_contains_actual_boundary_width_replies() {
        let records = (1..=20).map(record).collect::<Vec<_>>();
        let appends = records.iter().map(append).collect::<Vec<_>>();
        batch_result_fits(scope(), &appends).unwrap();
        assert!(
            batch_committed(reference(), scope(), &records)
                .unwrap()
                .len()
                <= crate::MAX_JSON_BYTES
        );
        assert!(
            batch_command_window(reference(), Some((scope(), &records)))
                .unwrap()
                .len()
                <= crate::MAX_JSON_BYTES
        );
        let mut duplicate = appends;
        duplicate[1].material_id = duplicate[0].material_id;
        assert_eq!(
            batch_result_fits(scope(), &duplicate),
            Err(ProtocolError::InvalidCommand)
        );
    }
}
