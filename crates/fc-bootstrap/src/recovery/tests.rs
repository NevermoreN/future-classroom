use super::test_support::Fixture;
use super::*;
use crate::voice_runtime::{
    AudioDomain, DomainProgress, RuntimeControl, RuntimeError, RuntimeFactory, StopReasons,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Condvar,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct Hold {
    entered: AtomicBool,
    released: Mutex<bool>,
    signal: Condvar,
}
impl Hold {
    fn wait(&self) {
        self.entered.store(true, Ordering::Release);
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.signal.wait(released).unwrap();
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.signal.notify_all();
    }
    fn entered(&self) {
        let until = Instant::now() + Duration::from_secs(10);
        while !self.entered.load(Ordering::Acquire) {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
    }
}
struct HeldTask(Arc<Hold>);
impl Task for HeldTask {
    type Output = ();
    fn allocation_bytes(&self) -> Option<usize> {
        Some(0)
    }
    fn run(self, _: &fc_runtime::worker::JobContext) {
        self.0.wait();
    }
}
struct AudioFactory(Arc<Hold>);
struct Audio(Arc<Hold>);
impl RuntimeFactory for AudioFactory {
    fn invalidate_runtime(&self) {}
    fn create(&self, _: RuntimeControl) -> Result<Box<dyn AudioDomain>, RuntimeError> {
        Ok(Box::new(Audio(self.0.clone())))
    }
}
impl AudioDomain for Audio {
    fn interrupt(&mut self, _: StopReasons) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn step(&mut self) -> Result<DomainProgress, RuntimeError> {
        Ok(DomainProgress::Idle)
    }
    fn has_native_ownership(&self) -> bool {
        false
    }
    fn resume_admission(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn finish(&mut self) -> Result<bool, RuntimeError> {
        self.0.wait();
        Ok(true)
    }
}
#[test]
fn cold_switch_permit_waits_for_physical_worker_audio_destructors_and_writer_close() {
    let f = Fixture::new();
    let (_, confirmed) = f.confirm();
    let candidate = f.root.join("candidate.sqlite3");
    let pool = WorkerPool::<HeldTask>::start().unwrap();
    let hold = Arc::new(Hold::default());
    let _ticket = pool
        .handle()
        .try_submit(
            HeldTask(hold.clone()),
            Instant::now() + Duration::from_secs(60),
        )
        .map_err(|_| ())
        .unwrap();
    hold.entered();
    let audio_hold = Arc::new(Hold::default());
    let audio = VoiceRuntime::new(Arc::new(AudioFactory(audio_hold.clone())));
    let mut shutdown = ColdShutdown::begin(confirmed, f.backend.clone(), pool, audio).unwrap();
    assert!(shutdown.poll().unwrap().is_none());
    audio_hold.entered();
    // A cancelled worker owns its real slot; audio completion alone is not proof.
    audio_hold.release();
    for _ in 0..16 {
        assert!(shutdown.poll().unwrap().is_none());
        std::thread::yield_now();
    }
    hold.release();
    let until = Instant::now() + Duration::from_secs(15);
    let drained = loop {
        if let Some(drained) = shutdown.poll().unwrap() {
            break drained;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    };
    assert!(!candidate.exists());
    let permit = drained
        .prepare_database(candidate.clone(), Arc::new(|| true))
        .unwrap();
    assert_eq!(
        permit.live_database(),
        f.root.join("state/classroom.sqlite3")
    );
    assert_eq!(permit.candidate_database(), candidate);
    assert!(permit.generation() >= 2);
    assert!(permit.revision().get() > 0);
    assert_ne!(permit.manifest_sha256(), [0; 32]);
    assert!(matches!(
        shutdown.poll(),
        Err(ShutdownError::AlreadySettled)
    ));
    // Opening the old and new databases after this proof does not race a writer.
    fc_storage::business::startup_state(permit.live_database()).unwrap();
    fc_storage::business::startup_state(permit.candidate_database()).unwrap();
}
