exec(__import__('pathlib').Path('/tmp/fc-round68-production-https/evidence/b-edit-first.py').read_text().split("edit('crates/fc-storage")[0])
def change(s):
 s=s.replace('use super::*;','use super::*;\nmod runtime;\npub(crate) use runtime::WebRuntimeReservation;',1)
 s=s.replace('    configuration_edit: AtomicBool,','    configuration_edit: AtomicBool,\n    runtime_owned: AtomicBool,\n    runtime_ready: AtomicBool,\n    runtime_stopping: AtomicBool,',1)
 s=s.replace('                configuration_edit: AtomicBool::new(false),','                configuration_edit: AtomicBool::new(false),\n                runtime_owned: AtomicBool::new(false),\n                runtime_ready: AtomicBool::new(false),\n                runtime_stopping: AtomicBool::new(false),',1)
 s=s.replace('        if let Err(error) = state.tick(tick) {','''        if self.inner.runtime_stopping.load(Ordering::Acquire) {
            state.enabled = false;
            state.clear_pairing();
            state.devices.clear();
            ledger.sessions.revoke_web();
        }
        if let Err(error) = state.tick(tick) {''',1)
 s=s.replace('        if state.enabled {\n            return Err(WebServiceError::Busy);\n        }','        if state.enabled || self.inner.runtime_owned.load(Ordering::Acquire) {\n            return Err(WebServiceError::Busy);\n        }',1)
 s=s.replace('        if enabled && self.inner.configuration_edit.load(Ordering::Acquire) {','''        if enabled && (self.inner.configuration_edit.load(Ordering::Acquire)
            || (self.inner.runtime_owned.load(Ordering::Acquire) && !self.inner.runtime_ready.load(Ordering::Acquire))) {''',1)
 s=s.replace('        state.enabled = enabled;','''        if enabled && !self.inner.runtime_owned.load(Ordering::Acquire) {
            // Existing explicit finite diagnostic composition. Normal product
            // composition exclusively uses the physical runtime reservation.
            self.inner.runtime_stopping.store(false, Ordering::Release);
        }
        state.enabled = enabled;''',1)
 return s
edit('crates/fc-bootstrap/src/backend/web.rs',change)
def backend(s):
 start=s.index('    fn session_guard(');end=s.index('    pub fn check_session',start);part=s[start:end]
 part=part.replace('        let ledger = self.ledger.clone();','        let web = self.web.clone();\n        let ledger = self.ledger.clone();',1)
 part=part.replace('        Arc::new(move |state, expected| {','''        Arc::new(move |state, expected| {
            if lease.origin() == TrustedOrigin::Web && !web.runtime_execution_current() {
                return Err(StorageError::AuthorityUnavailable);
            }''',1)
 part=part.replace('            if actual == *expected {','''            if lease.origin() == TrustedOrigin::Web && !web.runtime_execution_current() {
                return Err(StorageError::AuthorityUnavailable);
            }
            if actual == *expected {''',1)
 return s[:start]+part+s[end:]
edit('crates/fc-bootstrap/src/backend.rs',backend)
# Trusted worker metadata only; no native filesystem operation under Backend lock.
edit('crates/fc-bootstrap/src/backend/https_settings.rs',lambda s:s+'''
impl Backend {
    pub(crate) fn https_data_root(&self) -> Result<std::path::PathBuf> {
        self.database_path.parent().and_then(std::path::Path::parent)
            .map(std::path::Path::to_owned).ok_or(BackendError::Closed)
    }
}
''')
