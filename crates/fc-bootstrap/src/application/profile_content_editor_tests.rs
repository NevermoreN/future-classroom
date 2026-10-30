//! Pure state/view tests: no SQLite, file IO, picker, KDF or actual publication.
use super::*;
use fc_domain::{
    layout::LayoutScope,
    time::{LocalDate, LocalDateTime},
};
fn slot(n: u64) -> ContentSlotKey {
    ContentSlotKey {
        scope: LayoutScope::Public,
        node_id: LayoutNodeId::new(n).unwrap(),
    }
}
fn image(s: ContentSlotKey, n: u64) -> MediaReference {
    MediaReference {
        version_id: MediaVersionId::new(n).unwrap(),
        owner: s,
        sha256: [n as u8; 32],
        bytes: 100,
        width: 10,
        height: 10,
        format: ImageFormat::Png,
    }
}
fn version(rev: u64, payload: ContentPayload) -> ContentVersion {
    let text_sha256 = matches!(payload, ContentPayload::Text(_)).then_some([8; 32]);
    ContentVersion {
        revision: Revision::new(rev),
        payload,
        digests: ContentDigests {
            payload_sha256: [9; 32],
            text_sha256,
        },
        last_actor_id: PrincipalId::new(1).unwrap(),
        updated_at: LocalDateTime::new(LocalDate::new(2026, 10, 31).unwrap(), 3600).unwrap(),
    }
}
fn document(s: ContentSlotKey, payload: ContentPayload) -> ProfileContentDocument {
    ProfileContentDocument {
        slot: s,
        revision: Revision::new(2),
        latest_publish_sequence: 2,
        previous: Some(version(
            1,
            match &payload {
                ContentPayload::Text(_) => ContentPayload::Text("previous".into()),
                _ => ContentPayload::Empty,
            },
        )),
        current: Some(version(2, payload)),
        view_revision: Revision::new(2),
        current_page: 0,
        pending: None,
    }
}
fn text_editor() -> Editor {
    let mut e = Editor::new(slot(1), ComponentKind::SubjectText).unwrap();
    e.loaded(
        document(slot(1), ContentPayload::Text("saved".into())),
        false,
    )
    .unwrap();
    e
}
fn form(t: &str) -> FormData {
    let mut d = FormData::new();
    d.insert(TEXT, t.into()).unwrap();
    d
}
fn act(e: &mut Editor, a: ActionId) -> Command {
    e.command(a, &FormData::new()).unwrap()
}
fn selected(name: &str) -> SelectedFile {
    SelectedFile::isolated_probe(
        std::path::Path::new("FutureClassroom-Test-Pure-Editor"),
        name,
    )
    .unwrap()
}
#[test]
fn text_full_unicode_limit_requires_review_preserves_captured_cas_and_keeps_draft() {
    let mut e = text_editor();
    let text = "😀".repeat(10_000);
    e.capture(&form(&text)).unwrap();
    e.view(false).validate().unwrap();
    assert_eq!(e.view(false).fields[0].max_utf16, 20000);
    assert!(matches!(act(&mut e, REVIEW), Command::Stay));
    assert!(matches!(act(&mut e, CANCEL_REVIEW), Command::Stay));
    assert_eq!(e.text, text);
    act(&mut e, REVIEW);
    let Command::Submit {
        slot: s,
        expected_revision,
        operation,
        input,
    } = act(&mut e, CONFIRM)
    else {
        panic!("explicit submit")
    };
    assert_eq!(s, slot(1));
    assert_eq!(expected_revision, Revision::new(2));
    assert_eq!(
        operation,
        ContentOperation::Publish(ContentPayloadKind::Text)
    );
    assert!(matches!(input,LocalContentInput::Text(t)if t==text));
    assert!(e.command(CONFIRM, &FormData::new()).is_err());
    assert!(e.is_dirty());
}
#[test]
fn text_over_scalar_limit_is_repairable_and_async_load_cannot_replace_unsaved_draft() {
    let mut e = text_editor();
    let bad = "a".repeat(10_001);
    e.capture(&form(&bad)).unwrap();
    assert!(e.command(REVIEW, &FormData::new()).is_err());
    assert_eq!(e.text, bad);
    assert!(e
        .loaded(
            document(slot(1), ContentPayload::Text("background".into())),
            false
        )
        .is_err());
    assert!(e.capture(&form(&"b".repeat(20_001))).is_err());
    assert_eq!(e.text, bad);
    e.capture(&form("fixed")).unwrap();
    act(&mut e, REVIEW);
    assert!(
        matches!(act(&mut e,CONFIRM),Command::Submit{expected_revision,..}if expected_revision==Revision::new(2))
    );
    e.cancel_review();
    assert!(matches!(act(&mut e, RELOAD), Command::Reload));
    e.loaded(
        document(slot(1), ContentPayload::Text("actual".into())),
        false,
    )
    .unwrap();
    assert_eq!(e.text, "fixed");
    assert!(e.is_dirty());
}
#[test]
fn clear_and_undo_require_separate_confirmation_and_cancel_does_not_publish() {
    let mut e = text_editor();
    act(&mut e, CLEAR);
    assert!(e.view(false).description.contains("清空"));
    assert!(matches!(act(&mut e, CANCEL_REVIEW), Command::Stay));
    assert_eq!(e.document.as_ref().unwrap().revision, Revision::new(2));
    act(&mut e, UNDO);
    assert!(
        matches!(act(&mut e,CONFIRM),Command::Submit{expected_revision,operation:ContentOperation::Undo{target_revision},input:LocalContentInput::Empty,..}if expected_revision==Revision::new(2)&&target_revision==Revision::new(1))
    );
}
#[test]
fn ordered_existing_images_move_remove_and_recover_consumed_submission_without_new_file_clone() {
    let s = slot(2);
    let mut e = Editor::new(s, ComponentKind::SubjectImage).unwrap();
    let original = document(
        s,
        ContentPayload::Images((1..=3).map(|n| image(s, n)).collect()),
    );
    e.loaded(original.clone(), true).unwrap();
    assert!(e.view(false).status.contains("配额"));
    e.select(1, 0).unwrap();
    act(&mut e, UP);
    e.select(2, 0).unwrap();
    act(&mut e, REMOVE);
    act(&mut e, REVIEW);
    let Command::Submit {
        input: LocalContentInput::ImagePages(pages),
        expected_revision,
        ..
    } = act(&mut e, CONFIRM)
    else {
        panic!("image submit")
    };
    assert_eq!(expected_revision, Revision::new(2));
    let ids: Vec<_> = pages
        .iter()
        .map(|p| match p {
            ImagePageInput::Existing(r) => r.version_id.get(),
            _ => panic!("no new file"),
        })
        .collect();
    assert_eq!(ids, vec![2, 1]);
    e.cancel_review();
    act(&mut e, RELOAD);
    e.loaded(original, false).unwrap();
    assert_eq!(e.pages.len(), 2);
    assert!(e.is_dirty());
    e.view(false).validate().unwrap();
    assert!(e.view(false).actions.len() < 12);
}
#[test]
fn image_append_replace_cancel_and_fifty_page_cap_preserve_previous_draft() {
    let s = slot(3);
    let mut e = Editor::new(s, ComponentKind::SubjectImage).unwrap();
    e.loaded(
        document(s, ContentPayload::Images(vec![image(s, 1)])),
        false,
    )
    .unwrap();
    assert!(matches!(act(&mut e, REPLACE_IMAGES), Command::PickImages));
    assert_eq!(e.pages.len(), 1, "cancelled picker makes no edit");
    act(&mut e, PICK_IMAGES);
    e.accept_images(vec![selected("new.png")]).unwrap();
    assert_eq!(e.pages.len(), 2);
    assert!(e
        .accept_images((0..49).map(|_| selected("extra.png")).collect())
        .is_err());
    assert_eq!(e.pages.len(), 2);
    act(&mut e, REPLACE_IMAGES);
    e.accept_images(vec![selected("replacement.bmp")]).unwrap();
    assert_eq!(e.pages.len(), 1);
    assert!(matches!(e.pages[0], ImagePageInput::Selected(_)));
    act(&mut e, REVIEW);
    assert!(
        matches!(act(&mut e,CONFIRM),Command::Submit{input:LocalContentInput::ImagePages(p),..}if p.len()==1)
    );
    e.cancel_review();
    act(&mut e, RELOAD);
    e.loaded(
        document(s, ContentPayload::Images(vec![image(s, 7)])),
        false,
    )
    .unwrap();
    assert!(matches!(&e.pages[0],ImagePageInput::Existing(r)if r.version_id.get()==7));
    assert!(!e.is_dirty());
}
#[test]
fn node_scoped_load_and_kind_guards_never_replace_another_nodes_body() {
    let mut a = text_editor();
    let mut b = Editor::new(slot(9), ComponentKind::SubjectText).unwrap();
    b.loaded(
        document(slot(9), ContentPayload::Text("other".into())),
        false,
    )
    .unwrap();
    assert!(a
        .loaded(
            document(slot(9), ContentPayload::Text("foreign".into())),
            false
        )
        .is_err());
    assert_eq!(a.text, "saved");
    assert_eq!(b.text, "other");
    assert!(a.accept_images(vec![selected("wrong.png")]).is_err());
    assert!(Editor::new(slot(1), ComponentKind::Homework).is_err());
    a.accept_txt(selected("new.txt")).unwrap();
    act(&mut a, REVIEW);
    assert!(matches!(
        act(&mut a, CONFIRM),
        Command::Submit {
            input: LocalContentInput::Txt(_),
            ..
        }
    ));
    a.cancel_review();
    act(&mut a, RELOAD);
    a.loaded(
        document(slot(1), ContentPayload::Text("actual imported".into())),
        false,
    )
    .unwrap();
    assert_eq!(a.text, "actual imported");
}
#[test]
fn close_and_reload_discard_are_explicit_and_lock_cancel_keeps_text() {
    let mut e = text_editor();
    e.capture(&form("half typed")).unwrap();
    e.cancel_review();
    assert_eq!(e.text, "half typed");
    assert!(matches!(act(&mut e, CLOSE), Command::Stay));
    e.view(false).validate().unwrap();
    assert!(matches!(act(&mut e, KEEP), Command::Stay));
    assert_eq!(e.text, "half typed");
    act(&mut e, RELOAD);
    assert!(matches!(act(&mut e, DISCARD), Command::Reload));
    e.loaded(
        document(slot(1), ContentPayload::Text("fresh".into())),
        false,
    )
    .unwrap();
    assert_eq!(e.text, "fresh");
    assert!(!e.is_dirty());
}

#[test]
fn clear_of_already_empty_content_cannot_erase_the_one_undo_version() {
    let mut e = text_editor();
    let mut doc = document(slot(1), ContentPayload::Empty);
    doc.previous = Some(version(1, ContentPayload::Text("recover me".into())));
    e.loaded(doc, false).unwrap();
    assert!(e.command(CLEAR, &FormData::new()).is_err());
    assert!(
        !e.view(false)
            .actions
            .iter()
            .find(|a| a.id == CLEAR)
            .unwrap()
            .enabled
    );
    assert!(matches!(act(&mut e, UNDO), Command::Stay));
    assert!(
        matches!(act(&mut e,CONFIRM),Command::Submit{operation:ContentOperation::Undo{target_revision},..}if target_revision==Revision::new(1))
    );
}
