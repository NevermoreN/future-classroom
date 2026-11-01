from pathlib import Path
import json, hashlib
root=Path('/tmp/fc-round68-production-https/worktree')
ev=Path('/tmp/fc-round68-production-https/evidence')
paths=['crates/fc-bootstrap/src/backend/web/runtime.rs','crates/fc-bootstrap/src/https_runtime.rs','crates/fc-bootstrap/src/web_transfer/uploads/tests.rs','crates/fc-bootstrap/src/backend/web/runtime_tests.rs']
for rel in paths:
 p=ev/'b-before-second'/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes((root/rel).read_bytes())
p=root/paths[0];s=p.read_text();s=s.replace('''    pub(crate) fn settle_runtime_stop(&self) {
        self.negative_runtime_stop();
        self.close();
    }''','''    pub(crate) fn settle_runtime_stop(&self) -> bool {
        self.negative_runtime_stop();
        // HTTP must keep servicing cancellation/deadline/IOCP while another
        // worker or Writer holds metadata. This negative path never waits for
        // either lock and retries on its next bounded event-loop turn.
        let Ok(mut state) = self.inner.state.try_lock() else {
            return false;
        };
        let Ok(mut ledger) = self.inner.ledger.try_lock() else {
            return false;
        };
        state.enabled = false;
        state.clear_pairing();
        state.devices.clear();
        ledger.sessions.revoke_web();
        true
    }''');p.write_text(s)
p=root/paths[1];s=p.read_text().replace('''        let mut state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if state.stop_at.is_some() && !state.faulted && !self.quiescent {''','''        let mut state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if !self.quiescent && state.stop_at.is_some_and(|at| Instant::now() >= at + STOP_BUDGET) {
            state.faulted = true;
            state.retain_claim = true;
            state.status.phase = RuntimePhase::Faulted;
            state.status.error = Some(RuntimeError::StopTimeout);
        }
        if state.stop_at.is_some() && !state.faulted && !self.quiescent {''').replace('''    let mut stopping = false;
    let mut cleanup''','''    let mut stopping = false;
    let mut sessions_settled = false;
    let mut cleanup''').replace('''            plan.service.settle_runtime_stop();
            hub.request_domain_stop();''','''            hub.request_domain_stop();''').replace('''        if stopping {
            let original''','''        if stopping {
            if !sessions_settled {
                sessions_settled = plan.service.settle_runtime_stop();
            }
            let original''').replace('''            if net.is_idle()
                && cleaned''','''            if net.is_idle()
                && sessions_settled
                && cleaned''').replace('''    drop(plan);
    shared.lock().unwrap_or_else(|e| e.into_inner()).returned = true;''','''    drop(plan);
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    // Native/SSPI destruction is part of this same original budget too.
    if state.stop_at.is_some_and(|at| Instant::now() >= at + STOP_BUDGET) {
        state.faulted = true;
        state.retain_claim = true;
        state.status.phase = RuntimePhase::Faulted;
        state.status.error = Some(RuntimeError::StopTimeout);
    }
    state.returned = true;''');p.write_text(s)
