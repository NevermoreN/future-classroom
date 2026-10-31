use super::*;
use fc_ports::http::{TokenIssuer, CHUNK_BYTES};
use fc_runtime::worker::{Task, WorkerPool};

fn token() -> RequestToken {
    TokenIssuer::new().unwrap().issue(0).unwrap()
}

#[test]
fn weak_network_cancel_needs_no_busy_stage_lock_and_normal_handle_drop_preserves_ready() {
    let (control, life) = TransferControl::new(token(), Instant::now()).unwrap();
    let signal = control.cancellation_handle();
    drop(signal.clone());
    assert!(life.is_active());
    let other_thread = std::thread::spawn(move || signal.cancel());
    other_thread.join().unwrap();
    assert_eq!(life.checkpoint(), Err(TransferError::Cancelled));
    drop(control);
}

#[test]
fn one_original_admission_bounds_every_cloned_phase() {
    let admitted = Instant::now() - Duration::from_secs(31);
    let (control, life) = TransferControl::new(token(), admitted).unwrap();
    assert_eq!(life.deadline(), admitted + TRANSFER_BUDGET);
    let voice = life.bounded(VOICE_BUDGET).unwrap();
    assert_eq!(voice.checkpoint(), Err(TransferError::Deadline));
    assert_eq!(
        voice.bounded(TRANSFER_BUDGET).unwrap().deadline(),
        voice.deadline()
    );
    assert!(life.checkpoint().is_ok());
    control.cancel();
    assert_eq!(life.checkpoint(), Err(TransferError::Cancelled));
    assert_eq!(voice.checkpoint(), Err(TransferError::Cancelled));
}

#[test]
fn dropping_control_cancels_all_stable_owners_without_renewal() {
    let (control, life) = TransferControl::new(token(), Instant::now()).unwrap();
    let clone = life.clone();
    let fence = life.execution_fence();
    assert!(fence());
    drop(control);
    assert_eq!(clone.checkpoint(), Err(TransferError::Cancelled));
    assert!(!fence());
    assert!(life.bounded(Duration::ZERO).is_err());
    assert!(life
        .bounded(TRANSFER_BUDGET + Duration::from_secs(1))
        .is_err());
    assert!(TransferControl::new(token(), Instant::now() + Duration::from_secs(10)).is_err());
}

struct Capture(TransferLifetime);
impl Task for Capture {
    type Output = ExecutionFence;
    fn allocation_bytes(&self) -> Option<usize> {
        Some(0)
    }
    fn run(self, context: &JobContext) -> ExecutionFence {
        self.0.job_fence(context)
    }
}

#[test]
fn per_job_ticket_retirement_does_not_cancel_stable_transfer() {
    let (control, life) = TransferControl::new(token(), Instant::now()).unwrap();
    let pool = WorkerPool::<Capture>::start().unwrap();
    let ticket = match pool
        .handle()
        .try_submit(Capture(life.clone()), life.deadline())
    {
        Ok(ticket) => ticket,
        Err(_) => panic!("isolated worker admission"),
    };
    // wait consumes the ticket and drops its per-job cancellation source. A
    // stage must never retain this now-invalid fence for its next chunk.
    let old_job_fence = ticket.wait().unwrap();
    assert!(!old_job_fence());
    assert!(life.is_active());
    let ticket = match pool
        .handle()
        .try_submit(Capture(life.clone()), life.deadline())
    {
        Ok(ticket) => ticket,
        Err(_) => panic!("next job has independent cancellation"),
    };
    assert!(!ticket.wait().unwrap()());
    assert!(life.is_active());
    control.cancel();
    assert!(!life.is_active());
    pool.request_stop();
    pool.join().unwrap();
}

#[test]
fn only_exact_disk_ack_advances_body_and_framing_eof() {
    let token = token();
    let mut cursor =
        UploadCursor::new(token, UploadKind::Material, 42, CHUNK_BYTES as u64 + 3).unwrap();
    let p = cursor.plan_chunk(CHUNK_BYTES).unwrap();
    assert_eq!(
        p,
        ChunkPosition {
            sequence: 1,
            offset: 0
        }
    );
    assert_eq!(cursor.offset(), 0);
    assert!(!cursor.is_complete());
    cursor
        .acknowledge(UploadAck::written(
            token,
            UploadKind::Material,
            42,
            1,
            CHUNK_BYTES as u64,
        ))
        .unwrap();
    let p = cursor.plan_chunk(3).unwrap();
    assert_eq!(
        p,
        ChunkPosition {
            sequence: 2,
            offset: CHUNK_BYTES as u64
        }
    );
    cursor
        .acknowledge(UploadAck::written(
            token,
            UploadKind::Material,
            42,
            2,
            CHUNK_BYTES as u64 + 3,
        ))
        .unwrap();
    assert!(cursor.is_complete());
    assert!(cursor.finish().is_ok());
}

#[test]
fn foreign_stale_wrong_offset_kind_and_unplanned_acks_fail_closed() {
    let mut issuer = TokenIssuer::new().unwrap();
    let original = issuer.issue(0).unwrap();
    let reused_slot = issuer.issue(0).unwrap();
    let foreign_generation = token();
    for (token, kind, id, sequence, end) in [
        (reused_slot, UploadKind::Voice, 7, 1, 3),
        (foreign_generation, UploadKind::Voice, 7, 1, 3),
        (original, UploadKind::Material, 7, 1, 3),
        (original, UploadKind::Voice, 8, 1, 3),
        (original, UploadKind::Voice, 7, 2, 3),
        (original, UploadKind::Voice, 7, 1, 2),
        (original, UploadKind::Voice, 7, 1, 4),
    ] {
        let mut cursor = UploadCursor::new(original, UploadKind::Voice, 7, 6).unwrap();
        cursor.plan_chunk(3).unwrap();
        assert_eq!(
            cursor.acknowledge(UploadAck::written(token, kind, id, sequence, end)),
            Err(TransferError::InvalidAcknowledgement)
        );
        assert_eq!(cursor.offset(), 0);
        assert!(cursor
            .acknowledge(UploadAck::written(original, UploadKind::Voice, 7, 1, 3))
            .is_err());
        assert!(cursor.finish().is_err());
    }
    let mut cursor = UploadCursor::new(original, UploadKind::Voice, 7, 3).unwrap();
    assert!(cursor
        .acknowledge(UploadAck::written(original, UploadKind::Voice, 7, 1, 3))
        .is_err());
}

#[test]
fn no_second_chunk_or_eof_can_overtake_physical_write() {
    let token = token();
    let mut pending = UploadCursor::new(token, UploadKind::Material, 7, 6).unwrap();
    pending.plan_chunk(3).unwrap();
    assert!(pending.plan_chunk(3).is_err());
    assert!(pending
        .acknowledge(UploadAck::written(token, UploadKind::Material, 7, 1, 3))
        .is_err());
    let mut eof = UploadCursor::new(token, UploadKind::Material, 7, 3).unwrap();
    eof.plan_chunk(3).unwrap();
    assert!(eof.finish().is_err());
    assert!(eof
        .acknowledge(UploadAck::written(token, UploadKind::Material, 7, 1, 3))
        .is_err());
    for bytes in [0, CHUNK_BYTES + 1, usize::MAX, 4] {
        let mut cursor = UploadCursor::new(token, UploadKind::Material, 7, 3).unwrap();
        assert!(cursor.plan_chunk(bytes).is_err());
    }
    assert!(UploadCursor::new(token, UploadKind::Material, 0, 3).is_err());
    assert!(UploadCursor::new(token, UploadKind::Material, 7, 0).is_err());
    assert!(UploadCursor::new(
        token,
        UploadKind::Material,
        7,
        fc_ports::http::MAX_TRANSFER_BYTES + 1
    )
    .is_err());
}
