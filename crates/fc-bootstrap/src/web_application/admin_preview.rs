//! Read-only bounded draft geometry, using the production DWrite/layout engine.
use super::*;
use crate::{profile_content_cache::TextFrame, web_transfer::TransferLifetime};
use fc_domain::{
    auth::Role,
    ids::*,
    layout::{LayoutScope, LayoutTree, NodeKind, WidgetKind},
    profile::SubjectProfileDocument,
    profile_content::*,
};
use fc_protocol::web::admin::{self as wire, preview as dto, Resource};
use fc_runtime::worker::JobContext;
use std::sync::{Arc, Mutex};
fn invalid() -> WebResponse {
    failure(400, responses::ErrorCode::InvalidRequest)
}
fn changed() -> WebResponse {
    failure(409, responses::ErrorCode::RevisionConflict)
}
fn unavailable() -> WebResponse {
    failure(503, responses::ErrorCode::ServiceUnavailable)
}
fn slots(tree: &LayoutTree, replace: bool, result: &mut Vec<(ContentSlotKey, WidgetKind)>) {
    fn visit(
        tree: &LayoutTree,
        id: LayoutNodeId,
        replace: bool,
        result: &mut Vec<(ContentSlotKey, WidgetKind)>,
    ) {
        let Some(node) = tree.nodes.iter().find(|node| node.id == id) else {
            return;
        };
        if replace && matches!(node.kind, NodeKind::Outlet { .. }) {
            return;
        }
        if let NodeKind::Widget(kind @ (WidgetKind::SubjectText | WidgetKind::SubjectImage)) =
            node.kind
        {
            result.push((
                ContentSlotKey {
                    scope: tree.scope,
                    node_id: id,
                },
                kind,
            ))
        }
        for child in &node.children {
            visit(tree, *child, replace, result)
        }
    }
    visit(tree, tree.root, replace, result)
}
impl WebApplication {
    pub(crate) fn is_admin_preview(&self, head: &RequestHead) -> bool {
        matches!(
            self.route_hint(head),
            Ok(Route::Admin {
                resource: Resource::PreviewSource | Resource::Preview { .. },
                ..
            })
        )
    }
    pub(crate) fn admin_preview_in_job(
        &self,
        backend: &Arc<Mutex<Backend>>,
        head: &RequestHead,
        body: &[u8],
        life: &TransferLifetime,
        context: &JobContext,
    ) -> WebResponse {
        let fence = life.job_fence(context);
        if head.token != life.token() || !fence() {
            return unavailable();
        }
        let app = self.for_execution(fence.clone());
        let captured = (|| {
            let Ok(mut backend) = backend.lock() else {
                return Err(unavailable());
            };
            let backend = backend.begin_execution_scope(fence.clone());
            if !app.service.same_instance(&backend.web_service()) {
                return Err(unavailable());
            }
            let route = app
                .boundary
                .preflight(head)
                .map_err(|e| app.request_boundary_error(head, e))?;
            let session = app
                .authenticate_route(head, route)?
                .ok_or_else(|| failure(401, responses::ErrorCode::Unauthorized))?;
            if session.account.principal.role != Role::GlobalAdmin {
                return Err(failure(403, responses::ErrorCode::Forbidden));
            }
            let lease = session.lease();
            let revision = backend.global_revision().map_err(admin::backend_error)?;
            let identity = backend
                .management_identity(lease)
                .map_err(admin::backend_error)?;
            let baseline = backend
                .read_public_layout(lease)
                .map_err(admin::backend_error)?;
            if matches!(
                route,
                Route::Admin {
                    resource: Resource::PreviewSource,
                    write: false
                }
            ) {
                return Err(admin::data_response(wire::encode(
                    revision,
                    &dto::Source {
                        publication_revision: revision.get().to_string(),
                        identity_revision: identity.revision.get().to_string(),
                        display_revision: baseline.display.revision.get().to_string(),
                        public_layout_revision: baseline.layout.revision.get().to_string(),
                    },
                )));
            }
            let Route::Admin {
                resource: Resource::Preview { scope },
                write: true,
            } = route
            else {
                return Err(invalid());
            };
            if body.len() as u64 != head.content_length.unwrap_or(0)
                || body.len() > wire::MAX_ADMIN_JSON_BYTES
            {
                return Err(invalid());
            }
            let saved = match scope {
                LayoutScope::Public => None,
                LayoutScope::Subject(key) => backend
                    .read_subject_profile(lease, key)
                    .map_err(admin::backend_error)?,
            };
            let layout_revision = match scope {
                LayoutScope::Public => baseline.layout.revision,
                LayoutScope::Subject(_) => saved
                    .as_ref()
                    .map_or(Revision::default(), |p| p.layout_revision),
            };
            let draft = dto::draft(body, scope, layout_revision).map_err(|_| invalid())?;
            let expected = match scope {
                LayoutScope::Public => baseline.layout.revision,
                LayoutScope::Subject(_) => {
                    saved.as_ref().map_or(Revision::default(), |p| p.revision)
                }
            };
            if draft.expected != expected
                || draft.publication != revision
                || draft.identity != identity.revision
                || draft.display_revision != baseline.display.revision
            {
                return Err(changed());
            }
            if draft.layout.node_id_high_water
                < match scope {
                    LayoutScope::Public => baseline.layout.node_id_high_water,
                    LayoutScope::Subject(_) => saved.as_ref().map_or(0, |p| p.node_id_high_water),
                }
            {
                return Err(invalid());
            }
            let mut public = baseline.clone();
            if let Some(display) = draft.display {
                public.display = display
            }
            let subject = match scope {
                LayoutScope::Public => {
                    public.layout = draft.layout;
                    None
                }
                LayoutScope::Subject(key) => {
                    let mut profile = saved.clone().unwrap_or_else(|| {
                        SubjectProfileDocument::new(ProfileId::new(1).expect("fixed id"), key)
                    });
                    profile.tree = draft.layout.tree;
                    profile.content_styles = draft.layout.content_styles;
                    profile.node_id_high_water = draft.layout.node_id_high_water;
                    if let Some(materials) = draft.materials {
                        profile.materials = materials
                    }
                    profile.validate_structure().map_err(|_| invalid())?;
                    Some(profile)
                }
            };
            public
                .layout
                .tree
                .validate_composition(subject.as_ref().map(|p| &p.tree))
                .map_err(|_| invalid())?;
            let mut active = Vec::new();
            slots(&public.layout.tree, subject.is_some(), &mut active);
            if let Some(profile) = &subject {
                slots(&profile.tree, false, &mut active)
            }
            if active.len() > fc_domain::layout::MAX_VISIBLE_LEAVES {
                return Err(invalid());
            }
            let mut documents = Vec::new();
            let mut contents = Vec::new();
            for (slot, kind) in active {
                let (tree, water) = match slot.scope {
                    LayoutScope::Public => {
                        (&baseline.layout.tree, baseline.layout.node_id_high_water)
                    }
                    LayoutScope::Subject(_) => match &saved {
                        Some(profile) => (&profile.tree, profile.node_id_high_water),
                        None => {
                            contents.push(PublishedContentSnapshot {
                                slot,
                                revision: Revision::default(),
                                view_revision: Revision::default(),
                                current_page: 0,
                                content: PublishedContentDescriptor::Empty,
                            });
                            continue;
                        }
                    },
                };
                match tree.nodes.iter().find(|node| node.id == slot.node_id) {
                    Some(node) if node.kind == NodeKind::Widget(kind) => {
                        let document = backend
                            .read_profile_content(lease, slot)
                            .map_err(admin::backend_error)?;
                        contents.push(document.snapshot().map_err(|_| invalid())?);
                        documents.push(document)
                    }
                    Some(_) => return Err(invalid()),
                    None if slot.node_id.get() > water => contents.push(PublishedContentSnapshot {
                        slot,
                        revision: Revision::default(),
                        view_revision: Revision::default(),
                        current_page: 0,
                        content: PublishedContentDescriptor::Empty,
                    }),
                    None => return Err(invalid()),
                }
            }
            if backend.global_revision().map_err(admin::backend_error)? != revision {
                return Err(changed());
            }
            Ok((
                lease, revision, identity, baseline, public, subject, saved, documents, contents,
            ))
        })();
        let (lease, revision, identity, baseline, public, subject, saved, documents, contents) =
            match captured {
                Ok(value) => value,
                Err(response) => return response,
            };
        let mut text = TextFrame::default();
        for document in documents {
            if let Some(version) = document.current {
                if let ContentPayload::Text(body) = version.payload {
                    let Some(snapshot) = contents
                        .iter()
                        .find(|snapshot| snapshot.slot == document.slot)
                    else {
                        return invalid();
                    };
                    if text.insert_verified(snapshot, body).is_err() {
                        return invalid();
                    }
                }
            }
        }
        if !fence() {
            return unavailable();
        }
        let now = fc_windows::system::CurrentClock::new().try_now();
        let now = match now {
            Ok(now) => now,
            Err(_) => return unavailable(),
        };
        let measured = match crate::profile_preview::geometry_with_content(
            &identity,
            &public,
            subject.as_ref(),
            1,
            now,
            &contents,
            &text,
        ) {
            Ok(nodes) => nodes,
            Err(_) => return invalid(),
        };
        let fresh = {
            let Ok(backend) = backend.lock() else {
                return unavailable();
            };
            backend
                .check_session(lease, fc_application::identity::SessionPurpose::Manage)
                .is_ok()
                && backend
                    .global_revision()
                    .is_ok_and(|current| current == revision)
        };
        if !fresh || !fence() {
            return changed();
        }
        let candidate = subject
            .as_ref()
            .map_or(&public.layout.tree, |profile| &profile.tree);
        let nodes = measured
            .into_iter()
            .map(|node| dto::Node {
                id: node.key.id.get().to_string(),
                parent_id: node.parent.map(|p| p.id.get().to_string()),
                kind: match candidate
                    .nodes
                    .iter()
                    .find(|candidate| candidate.id == node.key.id)
                    .map(|n| &n.kind)
                {
                    Some(NodeKind::Root { .. }) => "root",
                    Some(NodeKind::Container(_)) => "container",
                    Some(NodeKind::Outlet { .. }) => "outlet",
                    _ => "widget",
                },
                x: node.rect.x,
                y: node.rect.y,
                width: node.rect.width,
                height: node.rect.height,
                clip: dto::Rect {
                    x: node.clip.x,
                    y: node.clip.y,
                    width: node.clip.width,
                    height: node.clip.height,
                },
                text: node.text,
                font_dip: node.font_dip,
                text_truncated: node.text_truncated,
            })
            .collect();
        admin::data_response(wire::encode(
            revision,
            &dto::Geometry {
                width: 1920,
                height: 1080,
                nodes,
                source_publication_revision: revision.get().to_string(),
                identity_revision: identity.revision.get().to_string(),
                display_revision: baseline.display.revision.get().to_string(),
                public_layout_revision: baseline.layout.revision.get().to_string(),
                subject_profile_revision: subject.map(|_| {
                    saved
                        .map_or(Revision::default(), |p| p.revision)
                        .get()
                        .to_string()
                }),
            },
        ))
    }
}
