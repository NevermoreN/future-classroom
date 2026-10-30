//! Controller admission checks use actual scoped scene data. The cancellation
//! fixture deliberately holds two fixed-pool workers until physical release;
//! it is not an assertion that WIC or SQL can be preempted by cancellation.
use super::*;
use fc_domain::{
    ids::{AuthEpoch, CommandId, LayoutNodeId, MediaVersionId},
    layout::{LayoutScope, WidgetKind},
    profile_content::{ContentSlotKey, ImageFormat, MediaReference},
};
use fc_presentation::{
    layout::PaintGlass,
    scene::{
        SceneCard, SceneContentIdentity, SceneImageContent, SceneNodeKey, ScenePaintNode,
        ScenePaintTree,
    },
};
use fc_runtime::{
    authority::AuthorityStore,
    worker::{JobContext, Rejection, Task, WorkerHandle},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Condvar,
};
fn slot(id: u64) -> ContentSlotKey {
    ContentSlotKey {
        scope: LayoutScope::Public,
        node_id: LayoutNodeId::new(id).unwrap(),
    }
}
fn sample(second: u32) -> LocalDateTime {
    LocalDateTime::new(LocalDate::new(2026, 10, 30).unwrap(), second).unwrap()
}
fn image_scene() -> (SceneSnapshot, PublishedContentSnapshot) {
    let reference = MediaReference {
        owner: slot(50),
        version_id: MediaVersionId::new(1).unwrap(),
        sha256: [9; 32],
        bytes: 10,
        width: 400,
        height: 200,
        format: ImageFormat::Png,
    };
    let rect = Rect {
        x: 20.,
        y: 20.,
        width: 400.,
        height: 300.,
    };
    let entry = PublishedContentSnapshot {
        slot: slot(50),
        revision: Revision::new(3),
        view_revision: Revision::new(4),
        current_page: 0,
        content: PublishedContentDescriptor::Image {
            current: reference.clone(),
            total_pages: 3,
        },
    };
    let mut scene = SceneSnapshot {
        generation: SceneGeneration::new(1),
        source_revision: Revision::new(5),
        captured_at: sample(0),
        valid_until: Some(sample(60)),
        cards: Vec::new(),
        paint_tree: None,
        text_tone: fc_domain::desktop::TextTone::Light,
        clock_rollback: false,
        public_calendar_unverified: false,
        lesson_control: None,
    };
    scene.cards.push(SceneCard {
        kind: ComponentKind::SubjectImage,
        rect,
        font_dip: 18.,
        component_alpha: 1.,
        surface_alpha: 0.03,
        edge_alpha: 0.5,
        content: CardContent::Image(SceneImageContent {
            current: reference,
            current_page: 0,
            total_pages: 3,
        }),
        page: Default::default(),
        visible_leaf_limit: 1,
        content_identity: Some(SceneContentIdentity {
            slot: slot(50),
            revision: Revision::new(3),
            view_revision: Revision::new(4),
        }),
    });
    scene.paint_tree = Some(ScenePaintTree {
        nodes: vec![ScenePaintNode {
            key: SceneNodeKey {
                scope: LayoutScope::Public,
                id: slot(50).node_id,
            },
            parent: None,
            rect,
            clip: rect,
            local_opacity: 1.,
            effective_opacity: 1.,
            glass: Some(PaintGlass {
                surface_alpha: 0.03,
                edge_alpha: 0.5,
            }),
            card_index: Some(0),
            widget_kind: Some(WidgetKind::SubjectImage),
        }],
        public_layout_revision: Revision::new(1),
        subject_profile: None,
        selection_reason: fc_domain::profile::ProfileSelectionReason::Public,
    });
    (scene, entry)
}
fn event(scene: &SceneSnapshot) -> DesktopEvent {
    DesktopEvent::ImagePage {
        generation: scene.generation,
        slot: slot(50),
        content_revision: Revision::new(3),
        view_revision: Revision::new(4),
        page: 1,
    }
}
#[test]
fn image_page_requires_exact_scoped_revision_generation_adjacent_page_and_actual_clip() {
    let (scene, entry) = image_scene();
    assert!(image_page_matches(&scene, &event(&scene)));
    assert_eq!(
        image_entries(&scene, std::slice::from_ref(&entry)),
        vec![entry.clone()]
    );
    for which in 0..7 {
        let mut bad = event(&scene);
        if let DesktopEvent::ImagePage {
            generation,
            slot,
            content_revision,
            view_revision,
            page,
        } = &mut bad
        {
            match which {
                0 => *generation = SceneGeneration::new(2),
                1 => *slot = super::desktop_content_tests::slot(51),
                2 => *content_revision = Revision::new(2),
                3 => *view_revision = Revision::new(3),
                4 => *page = 2,
                5 => *page = 3,
                _ => *page = 0,
            }
        }
        assert!(!image_page_matches(&scene, &bad));
    }
    let mut invisible = scene.clone();
    invisible.paint_tree.as_mut().unwrap().nodes[0].effective_opacity = 0.;
    assert!(image_entries(&invisible, std::slice::from_ref(&entry)).is_empty());
    let mut clipped = scene.clone();
    clipped.paint_tree.as_mut().unwrap().nodes[0].clip.height = 40.;
    assert!(image_entries(&clipped, std::slice::from_ref(&entry)).is_empty());
    let mut stale = entry;
    stale.current_page = 1;
    assert!(image_entries(&scene, &[stale]).is_empty());
}
#[test]
fn image_completion_gate_rejects_wrong_scene_source_time_and_fault_without_relayout() {
    let (scene, _) = image_scene();
    let (mut publisher, reader) = AuthorityStore::new((), scene.source_revision, AuthEpoch::new(1));
    publisher.initialize().unwrap();
    let gate = reader.gate();
    let now = sample(30);
    assert!(content_scene_current(
        &scene,
        scene.generation,
        scene.source_revision,
        now,
        &gate
    ));
    assert!(!content_scene_current(
        &scene,
        SceneGeneration::new(99),
        scene.source_revision,
        now,
        &gate
    ));
    assert!(!content_scene_current(
        &scene,
        scene.generation,
        Revision::new(4),
        now,
        &gate
    ));
    assert!(!content_scene_current(
        &scene,
        scene.generation,
        scene.source_revision,
        sample(60),
        &gate
    ));
    gate.fault();
    assert!(!content_scene_current(
        &scene,
        scene.generation,
        scene.source_revision,
        now,
        &gate
    ));
}
struct Hold {
    started: Arc<AtomicUsize>,
    release: Arc<(Mutex<bool>, Condvar)>,
}
impl Task for Hold {
    type Output = super::super::Result<Outcome>;
    fn allocation_bytes(&self) -> Option<usize> {
        Some(0)
    }
    fn run(self, _: &JobContext) -> Self::Output {
        self.started.fetch_add(1, Ordering::Release);
        let (lock, cv) = &*self.release;
        let guard = lock.lock().unwrap();
        let _guard = cv
            .wait_timeout_while(guard, Duration::from_secs(5), |released| !*released)
            .unwrap();
        Err(super::super::Failure::Stopped)
    }
}
struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Drop for Release {
    fn drop(&mut self) {
        let (lock, cv) = &*self.0;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
}
fn submit_held(
    handle: &WorkerHandle<Hold>,
    mut task: Hold,
    deadline: Instant,
    label: &str,
) -> JobTicket<super::super::Result<Outcome>> {
    let admission_deadline = deadline.min(Instant::now() + Duration::from_secs(5));
    loop {
        assert!(
            Instant::now() < admission_deadline,
            "{label}: bounded admission retry window expired"
        );
        match handle.try_submit(task, deadline) {
            Ok(ticket) => return ticket,
            Err(error) => {
                // Rejection returns the same unaccepted owner. Once accepted,
                // return immediately: never submit a duplicate held worker.
                task = error.task;
                assert_eq!(
                    error.kind,
                    Rejection::Busy,
                    "{label}: unexpected admission rejection {:?}",
                    error.kind
                );
                assert!(
                    Instant::now() < admission_deadline,
                    "{label}: admission remained Busy for the bounded retry window"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}
#[test]
fn hidden_controller_cancels_both_tickets_without_freeing_live_workers_or_resetting_generation() {
    let mut controller = Controller::new(true).unwrap();
    controller.generation = SceneGeneration::new(500);
    controller.needs_scene = false;
    controller.images_verified = Some((controller.generation, Revision::new(5)));
    let pool = WorkerPool::<Hold>::start().unwrap();
    let started = Arc::new(AtomicUsize::new(0));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let guard = Release(release.clone());
    let handle = pool.handle();
    let job_deadline = Instant::now() + Duration::from_secs(10);
    let first = submit_held(
        &handle,
        Hold {
            started: started.clone(),
            release: release.clone(),
        },
        job_deadline,
        "first held worker",
    );
    let second = submit_held(
        &handle,
        Hold {
            started: started.clone(),
            release,
        },
        job_deadline,
        "second held worker",
    );
    controller.images = Some(PendingImages {
        deadline: Instant::now() + Duration::from_secs(10),
        ticket: first,
        source: Revision::new(5),
        generation: controller.generation,
        cancelled: false,
    });
    controller.page = Some(PendingPage {
        deadline: Instant::now() + Duration::from_secs(10),
        ticket: second,
        source: Revision::new(5),
        generation: controller.generation,
        request: PageContentRequest {
            command_id: CommandId::new(1).unwrap(),
            slot: slot(50),
            expected_content_revision: Revision::new(3),
            expected_view_revision: Revision::new(4),
            page: 1,
        },
        cancelled: false,
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while started.load(Ordering::Acquire) != 2 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    controller.images.as_mut().unwrap().deadline = Instant::now() - Duration::from_millis(1);
    controller.expire_content_jobs();
    assert!(controller.images.as_ref().unwrap().cancelled);
    assert!(controller.page.as_ref().unwrap().cancelled);
    controller.hide();
    assert!(controller.needs_scene);
    assert_eq!(controller.generation.get(), 500);
    assert_eq!(controller.generation.next().unwrap().get(), 501);
    assert!(controller.images_verified.is_none());
    assert!(!controller.images.as_ref().unwrap().ticket.is_finished());
    assert!(!controller.page.as_ref().unwrap().ticket.is_finished());
    assert_eq!(pool.handle().try_stats().unwrap().running, 2);
    drop(guard);
    fn await_cancelled(ticket: &mut JobTicket<super::super::Result<Outcome>>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match ticket.try_result() {
                Ok(None) => {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                Err(fc_runtime::worker::JobError::Cancelled) => break,
                _ => panic!("cancelled held worker must report its actual terminal cancellation"),
            }
        }
    }
    await_cancelled(&mut controller.images.as_mut().unwrap().ticket);
    await_cancelled(&mut controller.page.as_mut().unwrap().ticket);
    pool.join().unwrap();
}

#[test]
fn preview_release_drops_actual_native_card_scene_and_text_provider_owners_before_new_hydration() {
    use fc_presentation::scene::ContentTextSource;
    let (mut scene, _) = image_scene();
    let body = "真实缓存归还反例 · 场景、提供者和原生卡片都持有正文";
    let snapshot = PublishedContentSnapshot {
        slot: slot(50),
        revision: Revision::new(3),
        view_revision: Revision::new(4),
        current_page: 0,
        content: PublishedContentDescriptor::Text {
            characters: body.chars().count() as u32,
            utf8_bytes: body.len() as u32,
            sha256: fc_security::sha256(body.as_bytes()),
        },
    };
    let mut text = TextFrame::default();
    text.insert_verified(&snapshot, body.into()).unwrap();
    let arc = text
        .text(
            snapshot.slot,
            snapshot.revision,
            fc_security::sha256(body.as_bytes()),
        )
        .unwrap();
    let weak = Arc::downgrade(&arc);
    scene.cards[0].kind = ComponentKind::SubjectText;
    scene.cards[0].content = CardContent::StandaloneText(arc);
    scene.paint_tree.as_mut().unwrap().nodes[0].widget_kind = Some(WidgetKind::SubjectText);
    let (mut publisher, reader) = AuthorityStore::new((), scene.source_revision, AuthEpoch::new(1));
    publisher.initialize().unwrap();
    let gate = reader.gate();
    let mut controller = Controller::new(true).unwrap();
    controller.generation = scene.generation;
    let lease = gate
        .lease_for_authorized_scope(DESKTOP_SCOPE, Instant::now() + Duration::from_secs(30))
        .unwrap();
    assert_eq!(
        controller
            .renderer
            .present(
                &scene,
                sample(30),
                &PublicationGuard {
                    gate: &gate,
                    lease,
                    current_generation: scene.generation,
                    settings_open: false
                }
            )
            .unwrap(),
        PresentOutcome::Offscreen
    );
    controller.text = text;
    controller.scene = Some(scene);
    assert!(
        weak.strong_count() >= 3,
        "provider, scene, and actual native Card each own the original Arc"
    );
    controller.release_for_preview();
    assert!(controller.scene.is_none());
    assert!(controller.facts.is_none());
    assert!(weak.upgrade().is_none(),"hidden is insufficient; every old physical text owner must drop before the new cache admission");
    assert_eq!(controller.renderer.diagnostics().dib_bytes, 0);
    assert!(controller.needs_scene);
}
