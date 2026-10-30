//! Strict, bounded DTOs. Immutable versions independently verify actual payload hashes.
use super::{codec::*, *};
use fc_domain::{layout::LayoutScope, profile_content::*};
use sha2::{Digest, Sha256};
pub(super) fn slot(s: ContentSlotKey) -> Value {
    json!({"scope":match s.scope {LayoutScope::Public=>Value::Null,LayoutScope::Subject(k)=>profiles_codec::key(k)},"node":s.node_id.get()})
}
pub(super) fn read_slot(v: &Value) -> Result<ContentSlotKey, StorageError> {
    object_fields(v, &["scope", "node"])?;
    Ok(ContentSlotKey {
        scope: if v["scope"].is_null() {
            LayoutScope::Public
        } else {
            LayoutScope::Subject(profiles_codec::read_key(&v["scope"])?)
        },
        node_id: profiles_codec::id(v, "node", LayoutNodeId::new)?,
    })
}
pub(super) fn hash(v: &Value) -> Result<[u8; 32], StorageError> {
    let a = v.as_array().ok_or(StorageError::Database)?;
    if a.len() != 32 {
        return Err(StorageError::Database);
    }
    let mut h = [0; 32];
    for (i, x) in a.iter().enumerate() {
        h[i] = u8::try_from(x.as_u64().ok_or(StorageError::Database)?)
            .map_err(|_| StorageError::Database)?;
    }
    Ok(h)
}
pub(super) fn media(r: &MediaReference) -> Value {
    json!({"id":r.version_id.get(),"owner":slot(r.owner),"sha":r.sha256,"bytes":r.bytes,"width":r.width,"height":r.height,"format":match r.format{ImageFormat::Png=>"png",ImageFormat::Jpeg=>"jpeg",ImageFormat::Bmp=>"bmp"}})
}
pub(super) fn read_media(v: &Value) -> Result<MediaReference, StorageError> {
    object_fields(
        v,
        &["id", "owner", "sha", "bytes", "width", "height", "format"],
    )?;
    let r = MediaReference {
        version_id: profiles_codec::id(v, "id", MediaVersionId::new)?,
        owner: read_slot(&v["owner"])?,
        sha256: hash(&v["sha"])?,
        bytes: number(v, "bytes")?,
        width: u32::try_from(number(v, "width")?).map_err(|_| StorageError::Database)?,
        height: u32::try_from(number(v, "height")?).map_err(|_| StorageError::Database)?,
        format: match text(v, "format")?.as_str() {
            "png" => ImageFormat::Png,
            "jpeg" => ImageFormat::Jpeg,
            "bmp" => ImageFormat::Bmp,
            _ => return Err(StorageError::Database),
        },
    };
    r.validate().map_err(|_| StorageError::Database)?;
    Ok(r)
}
pub(super) fn payload(p: &ContentPayload) -> Value {
    match p {
        ContentPayload::Empty => json!({"kind":"empty"}),
        ContentPayload::Text(t) => json!({"kind":"text","text":t}),
        ContentPayload::Images(a) => {
            json!({"kind":"images","pages":a.iter().map(media).collect::<Vec<_>>()})
        }
    }
}
pub(super) fn read_payload(v: &Value) -> Result<ContentPayload, StorageError> {
    match text(v, "kind")?.as_str() {
        "empty" => {
            object_fields(v, &["kind"])?;
            Ok(ContentPayload::Empty)
        }
        "text" => {
            object_fields(v, &["kind", "text"])?;
            Ok(ContentPayload::Text(text(v, "text")?.to_owned()))
        }
        "images" => {
            object_fields(v, &["kind", "pages"])?;
            let a = array(v, "pages")?;
            if a.len() > MAX_CONTENT_PAGES {
                return Err(StorageError::Database);
            }
            Ok(ContentPayload::Images(
                a.iter().map(read_media).collect::<Result<_, _>>()?,
            ))
        }
        _ => Err(StorageError::Database),
    }
}
pub(super) fn digests(
    s: ContentSlotKey,
    p: &ContentPayload,
) -> Result<ContentDigests, StorageError> {
    Ok(ContentDigests {
        payload_sha256: Sha256::digest(
            p.fingerprint_bytes(s)
                .map_err(StorageError::ProfileContent)?,
        )
        .into(),
        text_sha256: match p {
            ContentPayload::Text(t) => Some(Sha256::digest(t.as_bytes()).into()),
            _ => None,
        },
    })
}
pub(super) fn version(v: &ContentVersion) -> Value {
    json!({"revision":v.revision.get(),"payload":payload(&v.payload),"sha":v.digests.payload_sha256,"text_sha":v.digests.text_sha256,"actor":v.last_actor_id.get(),"at":time(v.updated_at)})
}
pub(super) fn read_version(v: &Value, s: ContentSlotKey) -> Result<ContentVersion, StorageError> {
    object_fields(
        v,
        &["revision", "payload", "sha", "text_sha", "actor", "at"],
    )?;
    let p = read_payload(&v["payload"])?;
    let actual = digests(s, &p).map_err(|_| StorageError::Database)?;
    if hash(&v["sha"])? != actual.payload_sha256
        || if v["text_sha"].is_null() {
            None
        } else {
            Some(hash(&v["text_sha"])?)
        } != actual.text_sha256
    {
        return Err(StorageError::Database);
    }
    let r = ContentVersion {
        revision: Revision::new(number(v, "revision")?),
        payload: p,
        digests: actual,
        last_actor_id: profiles_codec::id(v, "actor", PrincipalId::new)?,
        updated_at: read_time(&v["at"])?,
    };
    if r.revision == Revision::default() {
        return Err(StorageError::Database);
    }
    Ok(r)
}
pub(super) fn request(r: &BeginContentRequest) -> Value {
    json!({"command":r.command_id.get(),"slot":slot(r.slot),"expected":r.expected_revision.get(),"operation":match r.operation{ContentOperation::Publish(ContentPayloadKind::Text)=>json!({"kind":"text"}),ContentOperation::Publish(ContentPayloadKind::Images)=>json!({"kind":"images"}),ContentOperation::Clear=>json!({"kind":"clear"}),ContentOperation::Undo{target_revision}=>json!({"kind":"undo","target":target_revision.get()})},"fingerprint":r.payload_fingerprint,"expires":time(r.expires_at)})
}
fn read_request(v: &Value) -> Result<BeginContentRequest, StorageError> {
    object_fields(
        v,
        &[
            "command",
            "slot",
            "expected",
            "operation",
            "fingerprint",
            "expires",
        ],
    )?;
    let o = &v["operation"];
    let operation = match text(o, "kind")?.as_str() {
        "text" => {
            object_fields(o, &["kind"])?;
            ContentOperation::Publish(ContentPayloadKind::Text)
        }
        "images" => {
            object_fields(o, &["kind"])?;
            ContentOperation::Publish(ContentPayloadKind::Images)
        }
        "clear" => {
            object_fields(o, &["kind"])?;
            ContentOperation::Clear
        }
        "undo" => {
            object_fields(o, &["kind", "target"])?;
            ContentOperation::Undo {
                target_revision: Revision::new(number(o, "target")?),
            }
        }
        _ => return Err(StorageError::Database),
    };
    Ok(BeginContentRequest {
        command_id: profiles_codec::id(v, "command", CommandId::new)?,
        slot: read_slot(&v["slot"])?,
        expected_revision: Revision::new(number(v, "expected")?),
        operation,
        payload_fingerprint: hash(&v["fingerprint"])?,
        expires_at: read_time(&v["expires"])?,
    })
}
pub(super) fn admission(a: &ContentAdmission) -> Value {
    json!({"request":request(&a.request),"sequence":a.sequence,"actor":a.actor_id.get(),"epoch":a.auth_epoch.get()})
}
pub(super) fn read_admission(v: &Value) -> Result<ContentAdmission, StorageError> {
    object_fields(v, &["request", "sequence", "actor", "epoch"])?;
    let a = ContentAdmission {
        request: read_request(&v["request"])?,
        sequence: number(v, "sequence")?,
        actor_id: profiles_codec::id(v, "actor", PrincipalId::new)?,
        auth_epoch: AuthEpoch::new(number(v, "epoch")?),
    };
    if a.sequence == 0 {
        return Err(StorageError::Database);
    }
    Ok(a)
}
pub(super) fn snapshot(s: &PublishedContentSnapshot) -> Value {
    json!({"slot":slot(s.slot),"revision":s.revision.get(),"view":s.view_revision.get(),"page":s.current_page,"content":match &s.content{PublishedContentDescriptor::Empty=>json!({"kind":"empty"}),PublishedContentDescriptor::Text{characters,utf8_bytes,sha256}=>json!({"kind":"text","characters":characters,"bytes":utf8_bytes,"sha":sha256}),PublishedContentDescriptor::Image{current,total_pages}=>json!({"kind":"image","current":media(current),"total":total_pages})}})
}
pub(super) fn read_snapshot(v: &Value) -> Result<PublishedContentSnapshot, StorageError> {
    object_fields(v, &["slot", "revision", "view", "page", "content"])?;
    let c = &v["content"];
    let content = match text(c, "kind")?.as_str() {
        "empty" => {
            object_fields(c, &["kind"])?;
            PublishedContentDescriptor::Empty
        }
        "text" => {
            object_fields(c, &["kind", "characters", "bytes", "sha"])?;
            PublishedContentDescriptor::Text {
                characters: u32::try_from(number(c, "characters")?)
                    .map_err(|_| StorageError::Database)?,
                utf8_bytes: u32::try_from(number(c, "bytes")?)
                    .map_err(|_| StorageError::Database)?,
                sha256: hash(&c["sha"])?,
            }
        }
        "image" => {
            object_fields(c, &["kind", "current", "total"])?;
            PublishedContentDescriptor::Image {
                current: read_media(&c["current"])?,
                total_pages: u8::try_from(number(c, "total")?)
                    .map_err(|_| StorageError::Database)?,
            }
        }
        _ => return Err(StorageError::Database),
    };
    let snapshot = PublishedContentSnapshot {
        slot: read_slot(&v["slot"])?,
        revision: Revision::new(number(v, "revision")?),
        view_revision: Revision::new(number(v, "view")?),
        current_page: u8::try_from(number(v, "page")?).map_err(|_| StorageError::Database)?,
        content,
    };
    snapshot.validate().map_err(|_| StorageError::Database)?;
    Ok(snapshot)
}
