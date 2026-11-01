from pathlib import Path
r=Path('/tmp/fc-round68-production-https/worktree');e=r.parent/'evidence'
paths=['crates/fc-bootstrap/src/backend/web.rs','crates/fc-bootstrap/src/backend/web/runtime.rs','crates/fc-bootstrap/src/backend/web/runtime_tests.rs','crates/fc-bootstrap/src/https_runtime.rs','crates/fc-bootstrap/src/web_application/voice_files.rs','crates/fc-bootstrap/src/web_transfer/work/tests.rs']
for rel in paths:
 p=e/'b-before-third'/rel;p.parent.mkdir(parents=True,exist_ok=True)
 with p.open('xb') as f:f.write((r/rel).read_bytes())
p=r/paths[0];s=p.read_text().replace('runtime_owned: AtomicBool','runtime_claim: AtomicU64').replace('runtime_owned: AtomicBool::new(false)','runtime_claim: AtomicU64::new(0)')
# Previous replacement leaves initializer expression to adjust explicitly.
s=s.replace('runtime_claim: AtomicU64::new(false)','runtime_claim: AtomicU64::new(0)')
s=s.replace('!self.inner.runtime_owned.load(Ordering::Acquire)','self.inner.runtime_claim.load(Ordering::Acquire) == 0')
s=s.replace('self.inner.runtime_owned.load(Ordering::Acquire)','self.inner.runtime_claim.load(Ordering::Acquire) != 0');p.write_text(s)
p=r/paths[1];s=p.read_text();start=s.index('        self.inner\n            .runtime_owned\n            .compare_exchange');end=s.index('        self.inner.runtime_ready.store(false',start)
s=s[:start]+'''        if self.inner.runtime_claim.load(Ordering::Acquire) != 0 {
            return Err(WebServiceError::Busy);
        }
        // This monotonically increasing value is the exact physical claim,
        // independently retained while admission's epoch can become negative.
        let epoch = self.inner.runtime_epoch.try_update(
            Ordering::AcqRel, Ordering::Acquire,
            |word| word.checked_add(2).map(|word| word & !1),
        ).map(|previous| (previous + 2) & !1)
            .map_err(|_| WebServiceError::Overflow)?;
        self.inner.runtime_claim.compare_exchange(
            0, epoch, Ordering::AcqRel, Ordering::Acquire,
        ).map_err(|_| WebServiceError::Busy)?;
'''+s[end:]
s=s.replace('''        if !self.service.inner.runtime_owned.load(Ordering::Acquire) {''','''        if !self.owns_claim() {''')
start=s.index('    /// Caller must have observed the original HTTP thread');s=s[:start]+'''    pub(crate) fn owns_claim(&self) -> bool {
        self.service.inner.runtime_claim.load(Ordering::Acquire) == self.epoch
            && !self.returned.load(Ordering::Acquire)
    }
    /// Exact old runtime owners may revoke only their own admission epoch.
    /// A delayed request_stop can never close a replacement generation.
    pub(crate) fn negative_stop(&self) -> bool {
        if !self.owns_claim() {
            return false;
        }
        let _ = self.service.inner.runtime_epoch.try_update(
            Ordering::AcqRel, Ordering::Acquire,
            |word| ((word == self.epoch) || (word == self.epoch | 1))
                .then(|| self.epoch.checked_add(2)).flatten(),
        );
        self.service.inner.runtime_ready.store(false, Ordering::Release);
        self.service.inner.runtime_stopping.store(true, Ordering::Release);
        true
    }
    /// Caller must have observed the original HTTP thread's physical exit,
    /// after its Net/native owner and final worker cleanup already returned.
    pub(crate) fn physical_returned(&self) {
        if self.returned.swap(true, Ordering::AcqRel) {
            return;
        }
        // Clear readiness while this claim still excludes a new reservation.
        // No metadata store is permitted after relinquishing the exact claim.
        if self.service.inner.runtime_claim.load(Ordering::Acquire) == self.epoch {
            self.service.inner.runtime_ready.store(false, Ordering::Release);
            let _ = self.service.inner.runtime_claim.compare_exchange(
                self.epoch, 0, Ordering::AcqRel, Ordering::Acquire,
            );
        }
    }
}
impl Drop for WebRuntimeReservation {
    fn drop(&mut self) {
        if !self.started.load(Ordering::Acquire) && !self.returned.load(Ordering::Acquire) {
            // An unstarted plan may release only its own claim. A returned
            // reservation was already released and must never clear it again.
            let _ = self.service.inner.runtime_claim.compare_exchange(
                self.epoch, 0, Ordering::AcqRel, Ordering::Acquire,
            );
        }
        // Unknown native/worker retirement intentionally leaves its exact
        // runtime_claim set until process recovery; no replacement is admitted.
    }
}
''';p.write_text(s)
p=r/paths[2];s=p.read_text().replace('//!','//');p.write_text(s)
p=r/paths[3];s=p.read_text().replace('        Backend, SessionLease,','        Backend,').replace('use fc_domain::https_settings::HttpsConfiguration;','use fc_application::identity::SessionLease;\nuse fc_domain::https_settings::HttpsConfiguration;')
s=s.replace('fn stop(shared: &Arc<Mutex<Shared>>, service: &WebService, at: Instant) {\n    service.negative_runtime_stop();','fn stop(shared: &Arc<Mutex<Shared>>, reservation: &WebRuntimeReservation, at: Instant) {\n    if !reservation.negative_stop() {\n        return;\n    }')
s=s.replace('''    pub fn request_stop_at(&self, at: Instant) {
        let at''','''    pub fn request_stop_at(&self, at: Instant) {
        if self.quiescent || !self.reservation.owns_claim() {
            return;
        }
        let at''')
s=s.replace('stop(&self.shared, &self.service, at);','stop(&self.shared, &self.reservation, at);').replace('stop(&shared, &plan.service, Instant::now());','stop(&shared, &plan.reservation, Instant::now());');p.write_text(s)
p=r/paths[4];s=p.read_text().replace('voice: crate::backend::voice::VoiceService,','voice: crate::backend::VoiceService,');p.write_text(s)
p=r/paths[5];s=p.read_text().replace('''        Ok(HttpCompletion::Download(_)) => panic!("wrong job correlation"),''','''        Ok(HttpCompletion::Download(_)) => panic!("wrong job correlation"),
        #[cfg(windows)]
        Ok(HttpCompletion::Cleanup(_)) => panic!("unexpected cleanup job correlation"),''');p.write_text(s)
