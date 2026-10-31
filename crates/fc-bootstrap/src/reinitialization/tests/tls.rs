use super::*;
use crate::voice_runtime::{
    AudioDomain, DomainProgress, RuntimeControl, RuntimeError, RuntimeFactory, StopReason,
    StopReasons,
};
use std::{
    cell::RefCell,
    sync::{atomic::AtomicUsize, mpsc},
};

struct TlsExit(Arc<Gate>);
impl Drop for TlsExit {
    fn drop(&mut self) {
        self.0.hold();
    }
}
thread_local! {
    static EXIT: RefCell<Option<TlsExit>> = const { RefCell::new(None) };
}
struct Factory {
    gate: Arc<Gate>,
    created: AtomicUsize,
    finished: AtomicBool,
}
struct Handle(Arc<Factory>);
struct Domain {
    factory: Arc<Factory>,
    stopping: bool,
}
impl RuntimeFactory for Handle {
    fn invalidate_runtime(&self) {}
    fn create(&self, _: RuntimeControl) -> std::result::Result<Box<dyn AudioDomain>, RuntimeError> {
        EXIT.with(|slot| *slot.borrow_mut() = Some(TlsExit(self.0.gate.clone())));
        self.0.created.fetch_add(1, Ordering::AcqRel);
        Ok(Box::new(Domain {
            factory: self.0.clone(),
            stopping: false,
        }))
    }
}
impl AudioDomain for Domain {
    fn interrupt(&mut self, reasons: StopReasons) -> std::result::Result<(), RuntimeError> {
        self.stopping |= reasons.contains(StopReason::Shutdown);
        Ok(())
    }
    fn step(&mut self) -> std::result::Result<DomainProgress, RuntimeError> {
        Ok(if self.stopping {
            DomainProgress::Idle
        } else {
            DomainProgress::Active
        })
    }
    fn has_native_ownership(&self) -> bool {
        false
    }
    fn resume_admission(&mut self) -> std::result::Result<(), RuntimeError> {
        Ok(())
    }
    fn finish(&mut self) -> std::result::Result<bool, RuntimeError> {
        self.factory.finished.store(true, Ordering::Release);
        Ok(true)
    }
}
#[test]
fn reset_permit_waits_for_real_audio_tls_exit_without_blocking_its_poller() {
    let scratch = Scratch::new();
    let proof = NeverOpenedProof::claim(&scratch.root, 11).unwrap();
    let workers = WorkerPool::<QuietTask>::start().unwrap();
    let gate = Arc::new(Gate::default());
    let _release = ReleaseOnDrop(gate.clone());
    let factory = Arc::new(Factory {
        gate: gate.clone(),
        created: AtomicUsize::new(0),
        finished: AtomicBool::new(false),
    });
    // Pre-start the caller: Windows may serialize NEW thread entry behind a
    // held TLS destructor's loader lock. That is unrelated to poll correctness.
    let (input_send, input_receive) = mpsc::channel::<ColdReinitialization<QuietTask>>();
    let (output_send, output_receive) = mpsc::channel();
    let (ready_send, ready_receive) = mpsc::channel();
    let caller = thread::spawn(move || {
        ready_send.send(()).unwrap();
        let mut core = input_receive.recv().unwrap();
        let result = core.poll();
        let _ = output_send.send((core, result));
    });
    ready_receive.recv_timeout(Duration::from_secs(15)).unwrap();
    let audio = VoiceRuntime::new(Arc::new(Handle(factory.clone())));
    audio.set_available(true).unwrap();
    eventually(|| factory.created.load(Ordering::Acquire) == 1);
    let core = ColdReinitialization::begin(
        scratch.root.clone(),
        11,
        ReinitializationBackend::NeverOpened(proof),
        workers,
        Some(audio),
    );
    eventually(|| gate.entered.load(Ordering::Acquire));
    assert!(factory.finished.load(Ordering::Acquire));
    input_send
        .send(core)
        .unwrap_or_else(|_| panic!("prestarted poller disappeared"));
    let (mut core, result) = match output_receive.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(error) => {
            gate.release();
            let (mut core, _) = output_receive
                .recv_timeout(Duration::from_secs(15))
                .expect("poller must return after TLS release");
            if !core.settled {
                let _ = finish(&mut core);
            }
            caller.join().unwrap();
            panic!("reset poll joined the still-live audio TLS owner: {error}");
        }
    };
    assert!(
        result.unwrap().is_none(),
        "TLS owner was replaced by a reset permit"
    );
    assert!(core.workers.is_some());
    let status = core.audio.as_ref().unwrap().status().unwrap();
    assert!(status.physical_thread_running);
    assert_ne!(status.phase, RuntimePhase::Closed);
    assert_eq!(status.physical_threads_started, 1);
    gate.release();
    caller.join().unwrap();
    let permit = finish(&mut core).unwrap();
    let status = core.audio.as_ref().unwrap().status().unwrap();
    assert_eq!(status.phase, RuntimePhase::Closed);
    assert!(!status.physical_thread_running);
    assert_eq!(factory.created.load(Ordering::Acquire), 1);
    assert_eq!(permit.sequence(), 11);
    drop(permit);
}
