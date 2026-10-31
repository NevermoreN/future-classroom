from pathlib import Path
import os,signal,select,time,json,shutil,hashlib
base=Path('/tmp/fc-round37-ci-evidence');out=base/'chunk-lint-debug-original-stop-first.json';assert not out.exists()
controller=28635;cargo=28775;test=2814
expected={controller:['python3','/tmp/fc-round37-ci-evidence/chunk-lint-local-second.py'],cargo:['/config/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin/cargo','test','--workspace','--all-targets','--locked','--target-dir','/tmp/fc-round38-host-target','--','--test-threads=2'],test:['/tmp/fc-round38-host-target/debug/deps/fc_bootstrap-e5c2d97aab0c1386','--test-threads=2']}
fds={};r={'status':'RUNNING','scope':'Explicitly cancel only the original owned slow debug full-suite run using kernel pidfds after exact UID/argv/parent-chain validation. Preserve partial logs and mark NOT_COMPLETED, never PASS. Follow-up will run same full workspace in release under original900s command budget, jobs2/tests2. No source, assertions, KDF cost or acceptance threshold relaxed.','originalRun':'chunk-lint-local-second.json','processes':[],'scratch':[]}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
try:
 for pid,args in expected.items():
  fd=os.pidfd_open(pid);fds[pid]=fd;p=Path('/proc')/str(pid);actual=[x.decode() for x in (p/'cmdline').read_bytes().split(b'\0') if x];assert actual==args and p.stat().st_uid==os.getuid();status=(p/'status').read_text();ppid=int(next(x.split()[1] for x in status.splitlines() if x.startswith('PPid:')));r['processes'].append({'pid':pid,'parentPid':ppid,'arguments':actual});assert (pid!=cargo or ppid==controller) and (pid!=test or ppid==cargo)
 for p in Path('/dev/shm').glob('FutureClassroom-*-2814-*'):
  assert p.is_dir() and not p.is_symlink() and p.stat().st_uid==os.getuid();r['scratch'].append({'path':str(p),'inode':p.stat().st_ino,'bytes':sum(x.stat().st_size for x in p.rglob('*') if x.is_file() and not x.is_symlink())})
 save()
 for pid in [test,cargo]:
  try:signal.pidfd_send_signal(fds[pid],signal.SIGINT)
  except ProcessLookupError:pass
 for pid in [test,cargo,controller]:
  poll=select.poll();poll.register(fds[pid],select.POLLIN);ready=poll.poll(10000)
  if not ready and pid in [test,cargo]:
   signal.pidfd_send_signal(fds[pid],signal.SIGKILL);ready=poll.poll(5000)
  assert ready,'Original owned process exit not observed'
 for x in r['scratch']:
  p=Path(x['path'])
  if p.exists():assert p.stat().st_ino==x['inode'] and not p.is_symlink();shutil.rmtree(p)
  assert not p.exists()
 report=json.loads((base/'chunk-lint-local-second.json').read_text());assert report['status']=='FAIL' and report['commands'][-1]['name']=='host-test';r['originalRecordedExitCode']=report['commands'][-1]['exitCode'];r.update(status='PASS_CANCELLED_AND_CLEANED',originalTestOutcome='NOT_COMPLETED_NOT_PASS',actualOriginalProcessesExited=True)
except Exception as e:r.update(status='FAIL',errorKind=type(e).__name__)
finally:
 for fd in fds.values():os.close(fd)
 save()
print(json.dumps({k:r.get(k) for k in ['status','originalTestOutcome','originalRecordedExitCode','actualOriginalProcessesExited','errorKind']}))
raise SystemExit(0 if r['status']=='PASS_CANCELLED_AND_CLEANED' else 1)
