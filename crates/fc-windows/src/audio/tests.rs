use super::*;
use std::{cell::RefCell, collections::BTreeSet, io::Cursor};

#[derive(Default)]
struct FakeState {
    pointers: BTreeSet<usize>,
    queued: BTreeSet<usize>,
    returned: BTreeSet<usize>,
    prepared: BTreeSet<usize>,
    writes: usize,
    resets: usize,
    closes: usize,
    unprepares: usize,
    poisoned: bool,
    natural: bool,
    return_on_reset: bool,
    reset_failure: bool,
    unprepare_failure: bool,
    close_failure: bool,
    write_failure: bool,
}
thread_local! { static FAKE: RefCell<Rc<RefCell<FakeState>>> = RefCell::new(Rc::new(RefCell::new(FakeState::default()))); }
struct Fake(Rc<RefCell<FakeState>>);
impl Driver for Fake {
    fn open() -> Result<Self, Error> {
        Ok(Self(FAKE.with(|f| f.borrow().clone())))
    }
    fn prepare(&mut self, h: *mut WAVEHDR) -> u32 {
        let mut s = self.0.borrow_mut();
        s.pointers.insert(h as usize);
        s.prepared.insert(h as usize);
        0
    }
    fn write(&mut self, h: *mut WAVEHDR) -> u32 {
        let mut s = self.0.borrow_mut();
        assert!(s.prepared.contains(&(h as usize)));
        s.writes += 1;
        if s.write_failure {
            return 1;
        }
        s.returned.remove(&(h as usize));
        s.queued.insert(h as usize);
        0
    }
    fn returned(&self, h: *mut WAVEHDR) -> bool {
        self.0.borrow().returned.contains(&(h as usize))
    }
    fn reset(&mut self) -> u32 {
        let mut s = self.0.borrow_mut();
        s.resets += 1;
        if s.return_on_reset {
            s.returned = s.queued.clone();
            s.queued.clear();
        }
        if s.reset_failure {
            1
        } else {
            0
        }
    }
    fn unprepare(&mut self, h: *mut WAVEHDR) -> u32 {
        let mut s = self.0.borrow_mut();
        assert!(
            !s.queued.contains(&(h as usize)),
            "cannot unprepare driver-owned block"
        );
        if s.unprepare_failure {
            return WAVERR_STILLPLAYING;
        }
        assert!(s.prepared.remove(&(h as usize)));
        s.unprepares += 1;
        0
    }
    fn close(&mut self) -> u32 {
        let mut s = self.0.borrow_mut();
        assert!(s.prepared.is_empty());
        if s.close_failure {
            return WAVERR_STILLPLAYING;
        }
        s.closes += 1;
        0
    }
    fn wait(&mut self, _: u32) -> Result<(), Error> {
        let mut s = self.0.borrow_mut();
        if s.natural {
            s.returned = s.queued.clone();
            s.queued.clear();
        }
        Ok(())
    }
    fn poison(&self) {
        self.0.borrow_mut().poisoned = true;
    }
}
fn setup() -> (Engine<Fake>, Rc<RefCell<FakeState>>) {
    let state = Rc::new(RefCell::new(FakeState::default()));
    FAKE.with(|f| *f.borrow_mut() = state.clone());
    (Engine::new(), state)
}
#[test]
fn not_run_device_classification_does_not_hide_unexpected_driver_errors() {
    assert_eq!(open_error(MMSYSERR_NODRIVER), Error::DeviceUnavailable);
    assert_eq!(open_error(WAVERR_BADFORMAT), Error::DeviceUnavailable);
    assert_eq!(
        open_error(windows::Win32::Media::MMSYSERR_NOMEM),
        Error::Driver(7)
    );
    assert_eq!(open_error(987), Error::Driver(987));
}
fn begin(e: &mut Engine<Fake>, bytes: usize) {
    e.start(Box::new(Cursor::new(vec![0; bytes])), bytes, 7, &mut || {
        true
    })
    .unwrap();
}
#[test]
fn stream_reuses_three_fixed_addresses_and_releases_before_natural_completion() {
    let (mut e, s) = setup();
    s.borrow_mut().natural = true;
    begin(&mut e, BUFFER_BYTES * 8 + 100);
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::Playing { generation: 7 })
    );
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::Playing { generation: 7 })
    );
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::Playing { generation: 7 })
    );
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::NaturallyCompleted { generation: 7 })
    );
    assert_eq!(e.poll(0, &mut || true), Ok(Progress::Idle));
    let s = s.borrow();
    assert_eq!(s.writes, 9);
    assert_eq!(s.pointers.len(), 3);
    assert_eq!(s.unprepares, 3);
    assert_eq!(s.closes, 1);
    assert_eq!(s.resets, 0);
}
#[test]
fn reset_done_releases_without_counting_natural_and_stale_stop_cannot_cancel() {
    let (mut e, s) = setup();
    s.borrow_mut().return_on_reset = true;
    begin(&mut e, 64_000);
    assert_eq!(e.stop(6), Err(Error::Unauthorized));
    assert_eq!(s.borrow().resets, 0);
    assert_eq!(e.stop(7), Ok(Progress::Released { generation: 7 }));
    assert_eq!(s.borrow().resets, 1);
    assert_eq!(s.borrow().closes, 1);
    assert_eq!(e.poll(0, &mut || true), Ok(Progress::Idle));
}
#[test]
fn late_return_and_stillplaying_keep_allocations_and_admission_closed() {
    let (mut e, s) = setup();
    s.borrow_mut().reset_failure = true;
    begin(&mut e, 64_000);
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::Playing { generation: 7 })
    );
    assert_eq!(e.stop(7), Err(Error::Driver(1)));
    let addresses: Vec<_> = e
        .stream
        .as_ref()
        .unwrap()
        .buffers
        .iter()
        .map(|b| b.header() as usize)
        .collect();
    assert_eq!(s.borrow().unprepares, 0);
    e.stream.as_mut().unwrap().cleaning = Some(Instant::now() - CLEANUP_LIMIT);
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::Retained { generation: 7 })
    );
    assert_eq!(
        e.start(Box::new(Cursor::new([0; 2])), 2, 8, &mut || true),
        Err(Error::CleanupPending)
    );
    assert_eq!(
        addresses,
        e.stream
            .as_ref()
            .unwrap()
            .buffers
            .iter()
            .map(|b| b.header() as usize)
            .collect::<Vec<_>>()
    );
    {
        let mut s = s.borrow_mut();
        s.returned = s.queued.clone();
        s.queued.clear();
        s.unprepare_failure = true;
    }
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::Retained { generation: 7 })
    );
    assert_eq!(s.borrow().closes, 0);
    s.borrow_mut().unprepare_failure = false;
    s.borrow_mut().close_failure = true;
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::Retained { generation: 7 })
    );
    assert_eq!(s.borrow().unprepares, 3);
    assert!(e.stream.is_some());
    s.borrow_mut().close_failure = false;
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::Released { generation: 7 })
    );
    assert!(s.borrow().poisoned);
}
#[test]
fn failed_reset_even_when_all_buffers_return_releases_safely_but_forbids_reopening() {
    let (mut e, s) = setup();
    {
        let mut state = s.borrow_mut();
        state.reset_failure = true;
        state.return_on_reset = true;
    }
    begin(&mut e, 64_000);
    assert_eq!(e.stop(7), Err(Error::Driver(1)));
    assert!(e.stream.is_none());
    assert_eq!(s.borrow().closes, 1);
    assert!(s.borrow().poisoned);
    assert_eq!(
        e.start(Box::new(Cursor::new([0; 2])), 2, 8, &mut || true),
        Err(Error::CleanupPending)
    );
}
#[test]
fn revoked_authority_and_failed_write_use_cleanup_without_a_natural_event() {
    let (mut e, s) = setup();
    s.borrow_mut().return_on_reset = true;
    let mut calls = 0;
    let start = e.start(
        Box::new(Cursor::new(vec![0; 64_000])),
        64_000,
        7,
        &mut || {
            calls += 1;
            true
        },
    );
    assert_eq!(start, Ok(()));
    assert_eq!(calls, 2);
    assert_eq!(s.borrow().writes, 1);
    assert_eq!(
        e.poll(0, &mut || false),
        Ok(Progress::Released { generation: 7 })
    );
    s.borrow_mut().write_failure = true;
    assert_eq!(
        e.start(Box::new(Cursor::new([0; 2])), 2, 8, &mut || true),
        Err(Error::Driver(1))
    );
    assert_eq!(
        e.poll(0, &mut || true),
        Ok(Progress::Released { generation: 8 })
    );
}
