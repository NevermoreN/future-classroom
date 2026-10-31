use super::*;

fn same(actual: &str, expected: String) -> Result<(), StorageError> {
    if actual == expected {
        Ok(())
    } else {
        Err(StorageError::RecoveryInvalidPackage)
    }
}
fn parts(s: &str, n: usize) -> Result<Vec<u64>, StorageError> {
    let a = s.split('/').collect::<Vec<_>>();
    if a.len() != n {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    a.into_iter()
        .map(|x| {
            let v = x.parse::<u64>().map_err(invalid)?;
            if v == 0 || x != format!("{v:020}") {
                return Err(StorageError::RecoveryInvalidPackage);
            }
            Ok(v)
        })
        .collect()
}
fn profile_suffix(k: SubjectProfileKey) -> String {
    format!(
        "{:020}/{:020}/{:020}",
        k.class_id.get(),
        k.subject_id.get(),
        k.teacher_id.get()
    )
}
fn slot_suffix(s: ContentSlotKey) -> String {
    match s.scope {
        LayoutScope::Public => format!("public/{:020}", s.node_id.get()),
        LayoutScope::Subject(k) => format!("subject/{}/{:020}", profile_suffix(k), s.node_id.get()),
    }
}
fn profile_parts(s: &str) -> Result<SubjectProfileKey, StorageError> {
    let a = parts(s, 3)?;
    Ok(SubjectProfileKey {
        class_id: ClassId::new(a[0]).map_err(invalid)?,
        subject_id: SubjectId::new(a[1]).map_err(invalid)?,
        teacher_id: PrincipalId::new(a[2]).map_err(invalid)?,
    })
}
fn media(r: &mut MediaReference, m: &MigrationMapping) -> Result<(), StorageError> {
    r.owner = slot(r.owner, m)?;
    Ok(())
}
fn version(
    v: &mut ContentVersion,
    s: ContentSlotKey,
    m: &MigrationMapping,
) -> Result<(), StorageError> {
    v.last_actor_id = m.principal(v.last_actor_id).map_err(invalid)?;
    if let ContentPayload::Images(a) = &mut v.payload {
        for r in a {
            media(r, m)?;
        }
    }
    v.digests = profile_content_codec::digests(s, &v.payload)?;
    Ok(())
}
fn context(
    c: &mut fc_domain::lesson_context::EffectiveLessonContext,
    m: &MigrationMapping,
) -> Result<(), StorageError> {
    c.key.class_id = m.class(c.key.class_id).map_err(invalid)?;
    course(&mut c.base, m)?;
    course(&mut c.effective, m)?;
    c.teacher_id = c
        .teacher_id
        .map(|x| m.principal(x).map_err(invalid))
        .transpose()?;
    Ok(())
}

pub(crate) fn map_document(
    key: &str,
    bytes: &[u8],
    m: &MigrationMapping,
) -> Result<(String, Vec<u8>), StorageError> {
    if !recovery::allowed(key) {
        return Err(StorageError::RecoveryInvalidPackage);
    }
    let mut v = codec::value(bytes)?;
    let mut mapped = key.to_owned();
    if key.starts_with("business/homework/") || key.starts_with("business/reminder/") {
        let mut r = codec::read_record(&v)?;
        same(key, content_key(&r.key))?;
        r.last_actor_id = m.principal(r.last_actor_id).map_err(invalid)?;
        match &mut r.key {
            ContentKey::Homework(k) => {
                k.class_id = m.class(k.class_id).map_err(invalid)?;
                k.subject_id = m.subject(k.subject_id).map_err(invalid)?;
                k.owner_teacher_id = m.principal(k.owner_teacher_id).map_err(invalid)?;
            }
            ContentKey::SharedReminder(k) => k.class_id = m.class(k.class_id).map_err(invalid)?,
        }
        mapped = content_key(&r.key);
        v = codec::record(&r);
    } else if key == "business/schedule/v1" {
        let mut d = schedule_codec::read_document(&v)?;
        for p in &mut d.semesters {
            p.semester.class_id = m.class(p.semester.class_id).map_err(invalid)?;
            for t in p
                .semester
                .tables
                .iter_mut()
                .chain(p.dual_history.iter_mut().flat_map(|h| h.tables.iter_mut()))
            {
                for r in &mut t.rows {
                    for c in r.courses.iter_mut().flatten() {
                        course(c, m)?;
                    }
                }
            }
        }
        v = schedule_codec::document(&d);
    } else if key == "business/lesson-context/v1" || key == "business/recovery/lesson-audit/v1" {
        let mut s = lesson_codec::read_snapshot(&v)?;
        if s.current.is_some() || s.active_override.is_some() || s.base_trust.is_some() {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        s.class_id = s
            .class_id
            .map(|c| m.class(c).map_err(invalid))
            .transpose()?;
        for h in &mut s.history {
            context(&mut h.context, m)?;
        }
        v = lesson_codec::snapshot(&s);
    } else if key == "business/voice/v1" {
        let mut s = voice_codec::read_snapshot(&v)?;
        if !s.assets.is_empty()
            || s.jobs
                .iter()
                .any(|j| !j.state.terminal() || j.asset_id.is_some() || j.authorization.is_some())
        {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        for j in &mut s.jobs {
            j.owner = m.principal(j.owner).map_err(invalid)?;
            j.class_id = m.class(j.class_id).map_err(invalid)?;
        }
        v = voice_codec::snapshot(&s);
    } else if let Some(suffix) = key.strip_prefix("business/profiles/v1/documents/") {
        let mut d = profiles_codec::read_subject(&v)?;
        same(suffix, format!("{:020}", d.id.get()))?;
        d.key = profile(d.key, m)?;
        d.tree.scope = LayoutScope::Subject(d.key);
        v = profiles_codec::subject(&d);
    } else if let Some(suffix) = key.strip_prefix("business/profiles/v1/slots/") {
        codec::object_fields(&v, &["id"])?;
        ProfileId::new(codec::number(&v, "id")?).map_err(invalid)?;
        let k = profile_parts(suffix)?;
        mapped = format!(
            "business/profiles/v1/slots/{}",
            profile_suffix(profile(k, m)?)
        );
    } else if key.starts_with("business/profiles/v1/pages/") {
        codec::object_fields(&v, &["format", "entries"])?;
        for e in v["entries"]
            .as_array_mut()
            .ok_or(StorageError::RecoveryInvalidPackage)?
        {
            codec::object_fields(e, &["id", "key", "revision", "enabled"])?;
            e["key"] = profiles_codec::key(profile(profiles_codec::read_key(&e["key"])?, m)?);
        }
    } else if let Some(suffix) = key.strip_prefix("business/material/v1/folders/") {
        let mut f = materials_codec::read_folder(&v)?;
        same(suffix, format!("{:020}", f.id.get()))?;
        f.scope = material(f.scope, m)?;
        v = materials_codec::folder(&f);
    } else if let Some(suffix) = key.strip_prefix("business/material/v1/scope/") {
        codec::object_fields(&v, &["id"])?;
        FolderId::new(codec::number(&v, "id")?).map_err(invalid)?;
        let a = parts(suffix, 2)?;
        let s = material(
            MaterialScope {
                teacher_id: PrincipalId::new(a[0]).map_err(invalid)?,
                subject_id: SubjectId::new(a[1]).map_err(invalid)?,
            },
            m,
        )?;
        mapped = format!(
            "business/material/v1/scope/{:020}/{:020}",
            s.teacher_id.get(),
            s.subject_id.get()
        );
    } else if let Some(suffix) = key.strip_prefix("business/material/v1/items/") {
        let mut r = materials_codec::read_record(&v)?;
        same(suffix, format!("{:020}", r.id.get()))?;
        for c in &mut r.allowed_class_ids {
            *c = m.class(*c).map_err(invalid)?;
        }
        v = materials_codec::record(&r);
    } else if let Some(suffix) = key.strip_prefix("business/material/v1/resources/") {
        let r = materials::read_inventory(&v)?;
        same(suffix, format!("{:020}", r.resource.version_id.get()))?;
        v["scope"] = materials_codec::scope(material(r.scope, m)?);
    } else if let Some(suffix) = key.strip_prefix("business/profile-content/v1/heads/") {
        codec::object_fields(
            &v,
            &[
                "format", "slot", "revision", "sequence", "view", "page", "current", "previous",
                "pending", "retired",
            ],
        )?;
        let old = profile_content_codec::read_slot(&v["slot"])?;
        same(suffix, slot_suffix(old))?;
        if !v["pending"].is_null() {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        let s = slot(old, m)?;
        v["slot"] = profile_content_codec::slot(s);
        mapped = format!("business/profile-content/v1/heads/{}", slot_suffix(s));
    } else if let Some(suffix) = key.strip_prefix("business/profile-content/v1/slots/") {
        let old = profile_content_codec::read_slot(&v)?;
        same(suffix, slot_suffix(old))?;
        let s = slot(old, m)?;
        v = profile_content_codec::slot(s);
        mapped = format!("business/profile-content/v1/slots/{}", slot_suffix(s));
    } else if let Some(suffix) = key.strip_prefix("business/profile-content/v1/versions/") {
        codec::object_fields(&v, &["format", "slot", "version"])?;
        if codec::number(&v, "format")? != 1 {
            return Err(StorageError::RecoveryInvalidPackage);
        }
        let old = profile_content_codec::read_slot(&v["slot"])?;
        let mut r = profile_content_codec::read_version(&v["version"], old)?;
        same(
            suffix,
            format!("{}/{:020}", slot_suffix(old), r.revision.get()),
        )?;
        let s = slot(old, m)?;
        version(&mut r, s, m)?;
        v["slot"] = profile_content_codec::slot(s);
        v["version"] = profile_content_codec::version(&r);
        mapped = format!(
            "business/profile-content/v1/versions/{}/{:020}",
            slot_suffix(s),
            r.revision.get()
        );
    } else if key.starts_with("business/profile-media/v1/entries/") {
        codec::object_fields(
            &v,
            &[
                "format", "id", "owner", "command", "bytes", "resource", "revision", "refs",
                "state",
            ],
        )?;
        v["owner"] =
            profile_content_codec::slot(slot(profile_content_codec::read_slot(&v["owner"])?, m)?);
        if !v["resource"].is_null() {
            let mut r = profile_content_codec::read_media(&v["resource"])?;
            media(&mut r, m)?;
            v["resource"] = profile_content_codec::media(&r);
        }
    }
    Ok((mapped, display::encode(&v, MAX_VALUE_BYTES)?))
}
