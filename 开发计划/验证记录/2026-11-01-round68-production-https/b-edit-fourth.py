from pathlib import Path
r=Path('/tmp/fc-round68-production-https/worktree');e=r.parent/'evidence'
paths=['crates/fc-bootstrap/src/backend/web/runtime.rs','crates/fc-bootstrap/src/https_runtime.rs']
for rel in paths:
 p=e/'b-before-fourth'/rel;p.parent.mkdir(parents=True,exist_ok=True)
 with p.open('xb') as f:f.write((r/rel).read_bytes())
p=r/paths[0];s=p.read_text();old='Some(word.checked_add(2).unwrap_or(u64::MAX) & !1)';assert s.count(old)==1;s=s.replace(old,'Some(word.saturating_add(2) & !1)');p.write_text(s)
p=r/paths[1];s=p.read_text();old='''fn poll_cleanup<E: HttpExecutor>(
    work: CleanupWork<E::Ticket>,
    executor: &E,
) -> Result<(Option<CleanupWork<E::Ticket>>, bool), RuntimeError> {''';assert s.count(old)==1
s=s.replace(old,'''// Next original job/ticket owner plus a checked physical-drain observation.
type CleanupPollResult<T> = Result<(Option<CleanupWork<T>>, bool), RuntimeError>;
fn poll_cleanup<E: HttpExecutor>(
    work: CleanupWork<E::Ticket>,
    executor: &E,
) -> CleanupPollResult<E::Ticket> {''');p.write_text(s)
