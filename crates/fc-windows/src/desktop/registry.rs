//! Built-in desktop implementations. Shared frame drawing and native ownership
//! remain here; each registration supplies its actual lifecycle and decoration.
use super::*;
use fc_presentation::registry::{Registration, Registry};
pub(super) struct Hooks {
    pub title: &'static str,
    pub bind: fn(&SceneCard) -> Result<()>,
    pub create: fn(&DesktopRenderer, Rect) -> Result<Surface>,
    pub measure: fn(&Graphics, &SceneCard, bool) -> Result<ContentLayout>,
    pub render: fn(&Graphics, &mut Card) -> Result<()>,
    pub decorate: fn(&ID2D1DCRenderTarget, &Card) -> Result<()>,
    pub dynamic: fn(&Graphics, &mut Card, LocalDateTime) -> Result<()>,
    pub input: fn(&Graphics, &mut Card) -> Result<()>,
    pub dispose: fn(&Card),
}
const fn frame(title: &'static str) -> Hooks {
    Hooks {
        title,
        bind: validate_card,
        create: display_surface,
        measure: prepare_content,
        render: render_frame,
        decorate: no_decoration,
        dynamic: no_dynamic,
        input: render_input_strip,
        dispose: Card::hide,
    }
}
const fn clock() -> Hooks {
    let mut hooks = frame("时钟");
    hooks.decorate = clock_dial;
    hooks.dynamic = render_dynamic_frame;
    hooks
}
const fn recess() -> Hooks {
    let mut hooks = frame("课间倒计时");
    hooks.dynamic = render_dynamic_frame;
    hooks
}
const fn schedule() -> Hooks {
    let mut hooks = frame("今日课表");
    hooks.render = schedule_frame;
    hooks
}
static COMPONENTS: &[Registration<ComponentKind, Hooks>] = &[
    Registration {
        key: ComponentKind::Clock,
        implementation: clock(),
    },
    Registration {
        key: ComponentKind::Date,
        implementation: frame("日期"),
    },
    Registration {
        key: ComponentKind::Schedule,
        implementation: schedule(),
    },
    Registration {
        key: ComponentKind::Homework,
        implementation: frame("今日作业"),
    },
    Registration {
        key: ComponentKind::Materials,
        implementation: frame("课堂资料"),
    },
    Registration {
        key: ComponentKind::Reminder,
        implementation: frame("今日提醒"),
    },
    Registration {
        key: ComponentKind::DateCountdown,
        implementation: frame("日期倒计时"),
    },
    Registration {
        key: ComponentKind::Recess,
        implementation: recess(),
    },
    Registration {
        key: ComponentKind::SubjectText,
        implementation: frame("独立文字"),
    },
    Registration {
        key: ComponentKind::SubjectImage,
        implementation: frame("独立图片"),
    },
];
const KINDS: [ComponentKind; 10] = [
    ComponentKind::Clock,
    ComponentKind::Date,
    ComponentKind::Schedule,
    ComponentKind::Homework,
    ComponentKind::Materials,
    ComponentKind::Reminder,
    ComponentKind::DateCountdown,
    ComponentKind::Recess,
    ComponentKind::SubjectText,
    ComponentKind::SubjectImage,
];
pub(super) fn hooks(kind: ComponentKind) -> Result<&'static Hooks> {
    let registry = Registry::new(COMPONENTS).map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
    registry
        .require(&KINDS)
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
    registry
        .get(kind)
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))
}
fn no_decoration(_: &ID2D1DCRenderTarget, _: &Card) -> Result<()> {
    Ok(())
}
fn no_dynamic(_: &Graphics, _: &mut Card, _: LocalDateTime) -> Result<()> {
    Ok(())
}
fn clock_dial(target: &ID2D1DCRenderTarget, card: &Card) -> Result<()> {
    if matches!(card.source.content, CardContent::Clock(clock) if clock.mode == ClockMode::Analog) {
        draw_dial(target, card)?;
    }
    Ok(())
}
fn schedule_frame(graphics: &Graphics, card: &mut Card) -> Result<()> {
    if let CardContent::LessonControl {
        caption,
        can_change,
    } = &card.source.content
    {
        render_lesson_control(graphics, card, caption.clone(), *can_change)
    } else {
        render_frame(graphics, card)
    }
}

// Display surfaces have a different native owner/class and may never be
// substituted by the management form constructor, even though both use roles.
type DisplayFactory = fn(&DesktopRenderer, Rect) -> Result<Surface>;
static WINDOWS: &[Registration<fc_presentation::workbench::WindowRole, DisplayFactory>] =
    &[Registration {
        key: fc_presentation::workbench::WindowRole::Display,
        implementation: DesktopRenderer::surface,
    }];
fn display_surface(owner: &DesktopRenderer, rect: Rect) -> Result<Surface> {
    let create = Registry::new(WINDOWS)
        .and_then(|registry| registry.get(fc_presentation::workbench::WindowRole::Display))
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
    create(owner, rect)
}
