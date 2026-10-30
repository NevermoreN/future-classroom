//! Actual SQLite/storage/owned clocks and exact guard; media preparation here is
//! synthetic metadata (no WIC/FS/DPAPI claim), SHA-256 is the actual implementation.
use super::*;
use fc_domain::profile_content::*;
use fc_ports::{
    prepare_profile_content, ProfileContentPreparation, ProfileContentRepository,
    ProfileContentRepositoryError as CE, ProfileContentStatus,
};
use sha2::{Digest, Sha256};
fn admin() -> ActorContext {
    actor(1, TrustedOrigin::Local)
}
fn content_node(f: &Fixture, image: bool) -> ContentSlotKey {
    let mut r = public_request(20_000, f.admin().public_layout(&admin()).unwrap());
    let id = r.layout.allocate_id().unwrap();
    let group = r.layout.allocate_id().unwrap();
    let outlet = r
        .layout
        .tree
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Outlet { .. }))
        .unwrap();
    let mut children = std::mem::replace(&mut outlet.children, vec![group]);
    children.push(id);
    r.layout.tree.nodes.push(LayoutNode {
        id: group,
        kind: NodeKind::Container(ContainerSettings {
            flow: Flow::Column,
            padding: 0,
            gap: 8,
            width: AxisSize::Fixed(300),
            height: AxisSize::Fixed(400),
            group_opacity_percent: 100,
            material: Some(GlassMaterial {
                surface_percent: 3,
                edge_percent: 50,
            }),
        }),
        children,
    });
    r.layout.tree.nodes.push(LayoutNode {
        id,
        kind: NodeKind::Widget(if image {
            WidgetKind::SubjectImage
        } else {
            WidgetKind::SubjectText
        }),
        children: vec![],
    });
    let mut component = r.display.components[0].clone();
    component.kind = if image {
        fc_domain::desktop::ComponentKind::SubjectImage
    } else {
        fc_domain::desktop::ComponentKind::SubjectText
    };
    r.layout.content_styles.push(ContentNodeStyle {
        node_id: id,
        component,
        image_fit: ImageFit::Contain,
    });
    f.admin()
        .execute_public_layout(r, &admin(), f.clock.as_ref())
        .unwrap();
    ContentSlotKey {
        scope: LayoutScope::Public,
        node_id: id,
    }
}
fn request(
    f: &Fixture,
    s: ContentSlotKey,
    c: u64,
    rev: u64,
    op: ContentOperation,
) -> BeginContentRequest {
    BeginContentRequest {
        command_id: CommandId::new(c).unwrap(),
        slot: s,
        expected_revision: Revision::new(rev),
        operation: op,
        payload_fingerprint: [c as u8; 32],
        expires_at: LocalDateTime::new(f.clock.now().date, 12 * 3600).unwrap(),
    }
}
struct Prepared(ContentPayload);
impl ProfileContentPreparation for Prepared {
    type Input = ();
    fn prepare(&mut self, _: &ContentAdmission, _: ()) -> Result<ContentPayload, CE> {
        Ok(self.0.clone())
    }
    fn sha256(&self, b: &[u8]) -> Result<[u8; 32], CE> {
        Ok(Sha256::digest(b).into())
    }
}
fn proof(ad: ContentAdmission, p: ContentPayload) -> fc_ports::PreparedProfileContent {
    prepare_profile_content(&mut Prepared(p), ad, ()).unwrap()
}
fn begin(
    f: &Fixture,
    s: ContentSlotKey,
    c: u64,
    rev: u64,
    op: ContentOperation,
) -> ContentAdmission {
    f.admin()
        .begin_profile_content(request(f, s, c, rev, op), &admin(), f.clock.as_ref())
        .unwrap()
}
fn publish(
    f: &Fixture,
    s: ContentSlotKey,
    c: u64,
    rev: u64,
    text: &str,
) -> PublishedContentSnapshot {
    let a = begin(
        f,
        s,
        c,
        rev,
        ContentOperation::Publish(ContentPayloadKind::Text),
    );
    f.admin()
        .commit_profile_content(
            proof(a, ContentPayload::Text(text.into())),
            &admin(),
            f.clock.as_ref(),
        )
        .unwrap()
}
#[test]
fn profile_content_versions_full_limit_clear_undo_restart_and_light_facts() {
    let mut f = Fixture::new();
    let s = content_node(&f, false);
    let a = "😀".repeat(10_000);
    let b = "𐀀".repeat(10_000);
    publish(&f, s, 20_001, 0, &a);
    publish(&f, s, 20_002, 1, &b);
    let doc = f.admin().profile_content(s, &admin()).unwrap();
    assert_eq!(
        doc.current.unwrap().payload,
        ContentPayload::Text(b.clone())
    );
    assert_eq!(doc.previous.unwrap().payload, ContentPayload::Text(a));
    let facts = f.admin().desktop_facts(0, f.clock.as_ref()).unwrap();
    assert_eq!(facts.profile_contents.len(), 1);
    assert!(matches!(
        facts.profile_contents[0].content,
        PublishedContentDescriptor::Text {
            characters: 10000,
            utf8_bytes: 40000,
            ..
        }
    ));
    assert!(
        serde_json::to_vec(&display_codec::facts(&facts))
            .unwrap()
            .len()
            < 64000
    );
    let global = f.revision();
    assert_eq!(
        f.admin()
            .read_current_profile_content_version(s, Revision::new(2), global)
            .unwrap()
            .unwrap()
            .payload,
        ContentPayload::Text(b.clone())
    );
    f.reopen();
    assert_eq!(
        f.admin().profile_content(s, &admin()).unwrap().revision,
        Revision::new(2)
    );
    let ad = begin(&f, s, 20_003, 2, ContentOperation::Clear);
    f.admin()
        .commit_profile_content(proof(ad, ContentPayload::Empty), &admin(), f.clock.as_ref())
        .unwrap();
    let undo = begin(
        &f,
        s,
        20_004,
        3,
        ContentOperation::Undo {
            target_revision: Revision::new(2),
        },
    );
    f.admin()
        .commit_profile_content(
            proof(undo, ContentPayload::Empty),
            &admin(),
            f.clock.as_ref(),
        )
        .unwrap();
    assert_eq!(
        f.admin()
            .profile_content(s, &admin())
            .unwrap()
            .current
            .unwrap()
            .payload,
        ContentPayload::Text(b)
    );
    assert!(f
        .admin()
        .read_profile_content_version(s, Revision::new(1), &admin())
        .unwrap()
        .is_none());
    f.reopen();
}
#[test]
fn profile_content_durable_latest_intent_survives_failed_newer_prepare() {
    let mut f = Fixture::new();
    let s = content_node(&f, false);
    let a = begin(
        &f,
        s,
        21_001,
        0,
        ContentOperation::Publish(ContentPayloadKind::Text),
    );
    let b = begin(
        &f,
        s,
        21_002,
        0,
        ContentOperation::Publish(ContentPayloadKind::Text),
    );
    assert_eq!(b.sequence, a.sequence + 1);
    f.admin()
        .fail_profile_content(b.clone(), &admin(), f.clock.as_ref())
        .unwrap();
    f.reopen();
    assert_eq!(
        f.admin().commit_profile_content(
            proof(a, ContentPayload::Text("old".into())),
            &admin(),
            f.clock.as_ref()
        ),
        Err(CE::Domain(ProfileContentError::Superseded))
    );
    assert_eq!(
        f.admin()
            .profile_content_outcome(CommandId::new(21_001).unwrap(), &admin(), f.clock.as_ref())
            .unwrap()
            .unwrap()
            .status,
        ProfileContentStatus::Superseded
    );
    assert_eq!(
        f.admin()
            .profile_content_outcome(b.request.command_id, &admin(), f.clock.as_ref())
            .unwrap()
            .unwrap()
            .status,
        ProfileContentStatus::Failed
    );
    assert_eq!(
        f.admin()
            .profile_content(s, &admin())
            .unwrap()
            .latest_publish_sequence,
        2
    );
}
#[test]
fn profile_content_receipts_bind_actor_intent_actual_bytes_and_never_double_publish() {
    let f = Fixture::new();
    let s = content_node(&f, false);
    let r = request(
        &f,
        s,
        22_001,
        0,
        ContentOperation::Publish(ContentPayloadKind::Text),
    );
    let ad = f
        .admin()
        .begin_profile_content(r.clone(), &admin(), f.clock.as_ref())
        .unwrap();
    assert_eq!(
        ad,
        f.admin()
            .begin_profile_content(r.clone(), &admin(), f.clock.as_ref())
            .unwrap()
    );
    let mut changed = r;
    changed.payload_fingerprint = [1; 32];
    assert_eq!(
        f.admin()
            .begin_profile_content(changed, &admin(), f.clock.as_ref()),
        Err(CE::IdempotencyConflict)
    );
    let prepared = proof(ad.clone(), ContentPayload::Text("correct".into()));
    let first = f
        .admin()
        .commit_profile_content(prepared, &admin(), f.clock.as_ref())
        .unwrap();
    let rev = f.revision();
    assert_eq!(
        f.admin()
            .commit_profile_content(
                proof(ad.clone(), ContentPayload::Text("correct".into())),
                &admin(),
                f.clock.as_ref()
            )
            .unwrap(),
        first
    );
    assert_eq!(f.revision(), rev);
    assert_eq!(
        f.admin().commit_profile_content(
            proof(ad, ContentPayload::Text("other".into())),
            &admin(),
            f.clock.as_ref()
        ),
        Err(CE::IdempotencyConflict)
    );
}
#[test]
fn profile_content_teacher_web_and_exact_precommit_lock_cannot_publish() {
    let f = Fixture::new();
    let s = content_node(&f, false);
    for a in [actor(2, TrustedOrigin::Local), actor(1, TrustedOrigin::Web)] {
        assert!(f
            .repo(a.clone())
            .begin_profile_content(
                request(
                    &f,
                    s,
                    23_001,
                    0,
                    ContentOperation::Publish(ContentPayloadKind::Text)
                ),
                &a,
                f.clock.as_ref()
            )
            .is_err());
    }
    let ad = begin(
        &f,
        s,
        23_002,
        0,
        ContentOperation::Publish(ContentPayloadKind::Text),
    );
    let rev = f.revision();
    f.lock_begin.store(true, Ordering::Release);
    assert!(f
        .admin()
        .commit_profile_content(
            proof(ad, ContentPayload::Text("locked".into())),
            &admin(),
            f.clock.as_ref()
        )
        .is_err());
    assert_eq!(f.revision(), rev);
    f.active.store(true, Ordering::Release);
    assert!(f
        .admin()
        .profile_content(s, &admin())
        .unwrap()
        .current
        .is_none());
}
fn images(
    f: &Fixture,
    s: ContentSlotKey,
    c: u64,
) -> (
    ContentAdmission,
    super::super::super::profile_content::MediaReservation,
    Vec<MediaReference>,
) {
    let ad = begin(
        f,
        s,
        c,
        0,
        ContentOperation::Publish(ContentPayloadKind::Images),
    );
    let r = f
        .admin()
        .reserve_profile_media(&ad, &[100, 200], &admin())
        .unwrap();
    let a = r
        .ids
        .iter()
        .enumerate()
        .map(|(n, id)| MediaReference {
            version_id: *id,
            owner: s,
            sha256: [n as u8; 32],
            bytes: r.bytes[n],
            width: 10,
            height: 10,
            format: ImageFormat::Png,
        })
        .collect();
    (ad, r, a)
}
#[test]
fn profile_content_media_page_default_deny_exact_guard_and_ref_safe_gc() {
    use super::super::super::profile_content::MediaState;
    let mut f = Fixture::new();
    let s = content_node(&f, true);
    let (ad, r, images) = images(&f, s, 24_001);
    let snap = f
        .admin()
        .commit_profile_content(
            proof(ad, ContentPayload::Images(images)),
            &admin(),
            f.clock.as_ref(),
        )
        .unwrap();
    let entry = f
        .admin()
        .inventory_profile_media(None, 128)
        .unwrap()
        .items
        .remove(0);
    assert_eq!(entry.state, MediaState::Referenced);
    assert!(f
        .admin()
        .retire_profile_media(entry.id, entry.revision)
        .is_err());
    f.admin().settle_profile_media(r).unwrap();
    let page = PageContentRequest {
        command_id: CommandId::new(24_002).unwrap(),
        slot: s,
        expected_content_revision: snap.revision,
        expected_view_revision: snap.view_revision,
        page: 1,
    };
    assert!(f
        .admin()
        .page_profile_content(page.clone(), f.clock.as_ref())
        .is_err());
    let allowed = f.active.clone();
    let mut repo =
        f.admin()
            .with_profile_content_scene_guard(Arc::new(move |slot, content, view, _| {
                if allowed.load(Ordering::Acquire)
                    && slot == s
                    && content == Revision::new(1)
                    && view == Revision::new(1)
                {
                    Ok(())
                } else {
                    Err(StorageError::AuthorityUnavailable)
                }
            }));
    let result = repo.page_profile_content(page, f.clock.as_ref()).unwrap();
    assert_eq!(result.current_page, 1);
    assert_eq!(result.revision, snap.revision);
    assert_eq!(result.view_revision, Revision::new(2));
    let active = f.active.clone();
    let mut revoke_repo =
        f.admin()
            .with_profile_content_scene_guard(Arc::new(move |_, _, _, _| {
                if active.load(Ordering::Acquire) {
                    Ok(())
                } else {
                    Err(StorageError::AuthorityUnavailable)
                }
            }));
    f.lock_begin.store(true, Ordering::Release);
    let before = f.revision();
    assert!(revoke_repo
        .page_profile_content(
            PageContentRequest {
                command_id: CommandId::new(24_003).unwrap(),
                slot: s,
                expected_content_revision: Revision::new(1),
                expected_view_revision: Revision::new(2),
                page: 0
            },
            f.clock.as_ref()
        )
        .is_err());
    assert_eq!(f.revision(), before);
    f.active.store(true, Ordering::Release);
    f.reopen();
    assert_eq!(
        f.admin().profile_content(s, &admin()).unwrap().current_page,
        1
    );
}
#[test]
fn profile_content_remove_node_atomically_cancels_and_drops_refs_before_physical_finalize() {
    use super::super::super::profile_content::MediaState;
    let mut f = Fixture::new();
    let s = content_node(&f, true);
    let (ad, res, pages) = images(&f, s, 25_001);
    f.admin()
        .commit_profile_content(
            proof(ad, ContentPayload::Images(pages)),
            &admin(),
            f.clock.as_ref(),
        )
        .unwrap();
    let pending = begin(
        &f,
        s,
        25_002,
        1,
        ContentOperation::Publish(ContentPayloadKind::Images),
    );
    let before = f.admin().inventory_profile_media(None, 128).unwrap();
    let charged: u64 = before.items.iter().map(|e| e.charged_bytes).sum();
    let mut req = public_request(25_003, f.admin().public_layout(&admin()).unwrap());
    req.layout.tree.nodes.retain(|n| n.id != s.node_id);
    for n in &mut req.layout.tree.nodes {
        n.children.retain(|id| *id != s.node_id);
    }
    req.layout.content_styles.clear();
    f.admin()
        .execute_public_layout(req, &admin(), f.clock.as_ref())
        .unwrap();
    assert_eq!(
        f.admin()
            .profile_content_outcome(pending.request.command_id, &admin(), f.clock.as_ref())
            .unwrap()
            .unwrap()
            .status,
        ProfileContentStatus::Cancelled
    );
    let inv = f.admin().inventory_profile_media(None, 128).unwrap();
    assert!(inv
        .items
        .iter()
        .all(|e| e.refs == 0 && e.state == MediaState::Unreferenced));
    assert_eq!(
        inv.items.iter().map(|e| e.charged_bytes).sum::<u64>(),
        charged
    );
    for e in inv.items {
        let t = f.admin().retire_profile_media(e.id, e.revision).unwrap();
        f.admin().finalize_profile_media_removal(t).unwrap();
    }
    f.admin().settle_profile_media(res).unwrap();
    f.reopen();
    assert!(f
        .admin()
        .desktop_facts(0, f.clock.as_ref())
        .unwrap()
        .profile_contents
        .is_empty());
}
#[test]
fn profile_content_corrupt_version_hash_and_declared_missing_head_fail_startup() {
    let mut f = Fixture::new();
    let s = content_node(&f, false);
    publish(&f, s, 26_001, 0, "immutable");
    f.stop();
    f.sql("UPDATE documents SET value=CAST(replace(CAST(value AS TEXT),'immutable','tampered') AS BLOB) WHERE key LIKE 'business/profile-content/v1/versions/%'");
    assert!(matches!(
        startup_state(f.directory.db()),
        Err(StorageError::Database)
    ));
}
#[test]
fn profile_content_hydration_rejects_foreign_scope_stale_global_and_previous_version() {
    let f = Fixture::new();
    let s = content_node(&f, false);
    publish(&f, s, 27_001, 0, "one");
    let g = f.revision();
    publish(&f, s, 27_002, 1, "two");
    assert!(f
        .admin()
        .read_current_profile_content_version(s, Revision::new(1), g)
        .is_err());
    let g = f.revision();
    assert!(f
        .admin()
        .read_current_profile_content_version(s, Revision::new(1), g)
        .is_err());
    assert!(f
        .admin()
        .read_current_profile_content_version(
            ContentSlotKey {
                scope: LayoutScope::Subject(key(1, 2)),
                node_id: s.node_id
            },
            Revision::new(2),
            g
        )
        .is_err());
    assert_eq!(
        f.admin()
            .read_current_profile_content_version(s, Revision::new(2), g)
            .unwrap()
            .unwrap()
            .payload,
        ContentPayload::Text("two".into())
    );
}

#[test]
fn profile_content_expiry_restart_settlement_preserves_charge_and_never_releases_live_reservation()
{
    use super::super::super::profile_content::MediaState;
    let mut f = Fixture::new();
    let s = content_node(&f, true);
    let (ad, r, _) = images(&f, s, 28_001);
    *f.clock.0.lock().unwrap() = at(13, 0);
    let a = ActorContext {
        expires_at: at(23, 59),
        ..admin()
    };
    let outcome = f
        .repo(a.clone())
        .profile_content_outcome(ad.request.command_id, &a, f.clock.as_ref())
        .unwrap()
        .unwrap();
    assert_eq!(outcome.status, ProfileContentStatus::Expired);
    let inv = f.admin().inventory_profile_media(None, 128).unwrap();
    assert!(inv.items.iter().all(|e| e.state == MediaState::Reserved));
    for e in &inv.items {
        assert!(f.admin().retire_profile_media(e.id, e.revision).is_err());
    }
    f.reopen();
    for e in f.admin().inventory_profile_media(None, 128).unwrap().items {
        f.admin()
            .settle_profile_media_after_restart(e.id, e.revision)
            .unwrap();
    }
    let settled = f.admin().inventory_profile_media(None, 128).unwrap();
    assert_eq!(
        settled.items.iter().map(|e| e.charged_bytes).sum::<u64>(),
        r.bytes.iter().sum::<u64>()
    );
    assert!(settled
        .items
        .iter()
        .all(|e| e.state == MediaState::Unreferenced));
    f.reopen();
}

#[test]
fn profile_content_writer_rehashes_even_trusted_preparation_proof() {
    struct WrongHash;
    impl ProfileContentPreparation for WrongHash {
        type Input = ();
        fn prepare(&mut self, _: &ContentAdmission, _: ()) -> Result<ContentPayload, CE> {
            Ok(ContentPayload::Text("verified independently".into()))
        }
        fn sha256(&self, _: &[u8]) -> Result<[u8; 32], CE> {
            Ok([0; 32])
        }
    }
    let f = Fixture::new();
    let s = content_node(&f, false);
    let ad = begin(
        &f,
        s,
        29_001,
        0,
        ContentOperation::Publish(ContentPayloadKind::Text),
    );
    let prepared = prepare_profile_content(&mut WrongHash, ad, ()).unwrap();
    let before = f.revision();
    assert_eq!(
        f.admin()
            .commit_profile_content(prepared, &admin(), f.clock.as_ref()),
        Err(CE::Unavailable)
    );
    assert_eq!(f.revision(), before);
    assert!(f
        .admin()
        .profile_content(s, &admin())
        .unwrap()
        .current
        .is_none());
}

#[test]
fn profile_content_existing_profile_v1_exact_bytes_and_new_v2_strict_shape() {
    let mut f = Fixture::new();
    let original = f.admin().public_layout(&admin()).unwrap().layout;
    let old = profiles_codec::public(&original);
    assert_eq!(old["format"], json!(1));
    assert!(old.get("content_styles").is_none());
    assert_eq!(profiles_codec::read_public(&old).unwrap(), original);
    let s = content_node(&f, false);
    let layout = f.admin().public_layout(&admin()).unwrap().layout;
    let encoded = profiles_codec::public(&layout);
    assert_eq!(encoded["format"], json!(2));
    assert_eq!(profiles_codec::read_public(&encoded).unwrap(), layout);
    let mut missing = encoded.clone();
    missing.as_object_mut().unwrap().remove("content_styles");
    assert!(profiles_codec::read_public(&missing).is_err());
    let mut unknown = encoded;
    unknown["content_styles"][0]["path"] = json!("untrusted-path");
    assert!(profiles_codec::read_public(&unknown).is_err());
    publish(&f, s, 30_001, 0, "persisted");
    f.stop();
    f.sql("DELETE FROM documents WHERE key LIKE 'business/profile-content/v1/heads/%'");
    assert_eq!(startup_state(f.directory.db()), Err(StorageError::Database));
}

struct LargeMaterial;
impl fc_ports::MaterialPreparation for LargeMaterial {
    type Input = ();
    fn prepare(
        &mut self,
        _: (),
    ) -> Result<fc_domain::materials::MaterialResource, fc_ports::MaterialRepositoryError> {
        Ok(fc_domain::materials::MaterialResource {
            version_id: ResourceVersionId::new(99000).unwrap(),
            sha256: [4; 32],
            kind: fc_domain::materials::MaterialKind::Pdf,
            bytes: 50 * 1024 * 1024,
        })
    }
}
#[test]
fn profile_content_reserved_media_and_materials_share_two_gib_without_ttl_refund() {
    use fc_ports::MaterialRepository;
    let mut f = Fixture::new();
    let s = content_node(&f, true);
    let mut reservations = Vec::new();
    for n in 0..4 {
        let ad = begin(
            &f,
            s,
            31_000 + n,
            0,
            ContentOperation::Publish(ContentPayloadKind::Images),
        );
        let r = f
            .admin()
            .reserve_profile_media(&ad, &vec![10 * 1024 * 1024; 50], &admin())
            .unwrap();
        f.admin()
            .fail_profile_content(ad, &admin(), f.clock.as_ref())
            .unwrap();
        reservations.push(r);
    }
    let prepared = || {
        fc_ports::prepare_material_append(
            &mut LargeMaterial,
            (),
            fc_ports::MaterialAppendMetadata {
                command_id: CommandId::new(31_100).unwrap(),
                scope: MaterialScope {
                    teacher_id: principal(2),
                    subject_id: SubjectId::new(1).unwrap(),
                },
                material_id: MaterialId::new(99000).unwrap(),
                display_name: "large.pdf".into(),
                allowed_class_ids: vec![ClassId::new(1).unwrap()],
            },
        )
        .unwrap()
    };
    assert_eq!(
        f.admin()
            .append_material(prepared(), &admin(), f.clock.as_ref()),
        Err(fc_ports::MaterialRepositoryError::QuotaExceeded)
    );
    f.admin()
        .settle_profile_media(reservations.remove(0))
        .unwrap();
    assert_eq!(
        f.admin()
            .append_material(prepared(), &admin(), f.clock.as_ref()),
        Err(fc_ports::MaterialRepositoryError::QuotaExceeded)
    );
    let first = f.admin().inventory_profile_media(None, 128).unwrap();
    for e in first.items.into_iter().take(1) {
        let t = f.admin().retire_profile_media(e.id, e.revision).unwrap();
        f.admin().finalize_profile_media_removal(t).unwrap();
    }
    f.admin()
        .append_material(prepared(), &admin(), f.clock.as_ref())
        .unwrap();
    f.reopen();
}

#[test]
fn profile_content_committed_publication_failure_preserves_immutable_version_and_durable_outcome() {
    let mut f = Fixture::new();
    let s = content_node(&f, false);
    let ad = begin(
        &f,
        s,
        32_001,
        0,
        ContentOperation::Publish(ContentPayloadKind::Text),
    );
    f.publish_fail.store(true, Ordering::Release);
    assert_eq!(
        f.admin().commit_profile_content(
            proof(
                ad.clone(),
                ContentPayload::Text("committed before fault".into())
            ),
            &admin(),
            f.clock.as_ref()
        ),
        Err(CE::Faulted)
    );
    assert!(f.admin().profile_content(s, &admin()).is_err());
    f.stop();
    startup_state(f.directory.db()).unwrap();
    f.publish_fail.store(false, Ordering::Release);
    f.open();
    let outcome = f
        .admin()
        .profile_content_outcome(ad.request.command_id, &admin(), f.clock.as_ref())
        .unwrap()
        .unwrap();
    assert_eq!(outcome.status, ProfileContentStatus::Committed);
    assert_eq!(outcome.committed_revision, Some(Revision::new(1)));
    assert_eq!(
        f.admin()
            .profile_content(s, &admin())
            .unwrap()
            .current
            .unwrap()
            .payload,
        ContentPayload::Text("committed before fault".into())
    );
}

#[test]
fn profile_content_subject_owners_are_exact_and_disabled_history_stays_manageable() {
    use fc_ports::ScheduleRepository;
    let mut f = Fixture::new();
    let mut slots = Vec::new();
    for (n, owner) in [(33_001, 2), (33_002, 3)] {
        let mut r = f.request(n, key(1, owner));
        let leaf = r
            .candidate
            .tree
            .nodes
            .iter_mut()
            .find(|n| matches!(n.kind, NodeKind::Widget(_)))
            .unwrap();
        leaf.kind = NodeKind::Widget(WidgetKind::SubjectText);
        let node_id = leaf.id;
        r.candidate.content_styles = vec![ContentNodeStyle::new(node_id, ContentPayloadKind::Text)];
        let p = f.save(r).unwrap();
        let slot = ContentSlotKey {
            scope: LayoutScope::Subject(p.key),
            node_id,
        };
        publish(&f, slot, n + 10, 0, &format!("teacher-{owner}"));
        slots.push((slot, p));
    }
    f.admin()
        .execute_schedule(
            fc_ports::ScheduleRequest {
                command_id: CommandId::new(33_030).unwrap(),
                expected_revision: Revision::default(),
                candidate: schedule(),
            },
            &admin(),
            f.clock.as_ref(),
        )
        .unwrap();
    let facts = f.admin().desktop_facts(0, f.clock.as_ref()).unwrap();
    assert_eq!(facts.profile_contents.len(), 1);
    assert_eq!(facts.profile_contents[0].slot, slots[0].0);
    assert!(f
        .admin()
        .read_current_profile_content_version(slots[1].0, Revision::new(1), facts.global_revision)
        .is_err());
    assert_eq!(
        f.admin()
            .read_current_profile_content_version(
                slots[0].0,
                Revision::new(1),
                facts.global_revision
            )
            .unwrap()
            .unwrap()
            .payload,
        ContentPayload::Text("teacher-2".into())
    );
    let (slot, mut disabled) = slots.remove(0);
    let expected = disabled.revision;
    disabled.enabled = false;
    f.save(ProfileRequest {
        command_id: CommandId::new(33_031).unwrap(),
        key: disabled.key,
        expected_revision: expected,
        candidate: disabled,
    })
    .unwrap();
    assert!(f
        .admin()
        .desktop_facts(0, f.clock.as_ref())
        .unwrap()
        .profile_contents
        .is_empty());
    assert_eq!(
        f.admin()
            .profile_content(slot, &admin())
            .unwrap()
            .current
            .unwrap()
            .payload,
        ContentPayload::Text("teacher-2".into())
    );
    assert!(f
        .admin()
        .begin_profile_content(
            request(&f, slot, 33_032, 1, ContentOperation::Clear),
            &admin(),
            f.clock.as_ref()
        )
        .is_err());
    f.reopen();
}
