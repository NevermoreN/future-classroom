//! Explicit native window lifecycle implementations; all remain on their
//! original UI thread. Roles reuse one adapter, not additional hidden windows.
use super::*;
use fc_presentation::registry::{Registration, Registry};
pub(super) struct Hooks {
    pub create: fn() -> Result<Window>,
    pub bind: fn(&View) -> Result<()>,
    pub render: fn(&Window, &View) -> Result<bool>,
    pub preview: fn(&Window, Option<LayoutPreview>) -> Result<()>,
    pub clear_preview: fn(&Window),
    pub measure: fn(&Window) -> Result<()>,
    pub input: fn(&Window) -> Result<Vec<Event>>,
    pub dispose: fn(&mut Window),
}
const fn form() -> Hooks {
    Hooks {
        create: Window::create_native,
        bind: bind_view,
        render: Window::render_native,
        preview: Window::bind_preview_native,
        clear_preview: Window::clear_preview_native,
        measure: Window::measure_native,
        input: Window::input_native,
        dispose: Window::dispose_native,
    }
}
static WINDOWS: &[Registration<WindowRole, Hooks>] = &[
    Registration {
        key: WindowRole::Workbench,
        implementation: form(),
    },
    Registration {
        key: WindowRole::LayoutEditor,
        implementation: form(),
    },
    Registration {
        key: WindowRole::Preview,
        implementation: form(),
    },
    Registration {
        key: WindowRole::Status,
        implementation: form(),
    },
];
pub(super) fn hooks(role: WindowRole) -> Result<&'static Hooks> {
    let registry = Registry::new(WINDOWS).map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
    registry
        .require(&WindowRole::FORMS)
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))?;
    registry
        .get(role)
        .map_err(|_| Error::from_hresult(E_UNEXPECTED))
}

fn bind_view(view: &View) -> Result<()> {
    view.validate()
        .map_err(|_| Error::from_hresult(E_INVALIDARG))
}
