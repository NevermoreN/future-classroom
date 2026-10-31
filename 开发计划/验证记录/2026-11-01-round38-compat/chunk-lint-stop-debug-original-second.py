from pathlib import Path
import os,signal,time,json,shutil
base=Path('/tmp/fc-round37-ci-evidence');out=base/'chunk-lint-debug-original-stop-second.json';assert not out.exists()
expected={28635:['python3','/tmp/fc-round37-ci-evidence/chunk-lint-local-second.py'],28775:['/config/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin/cargo','test','--workspace','--all-targets','--locked','--target-dir','/tmp/fc-round38-host-target','--','--test-threads=2'],2814:['/tmp/fc-round38-host-target/debug/deps/fc_bootstrap-e5c2d97aab0c1386','--test-threads=2']}
r={'status':'RUNNING','scope':'Cancel only original owned local debug test/Cargo after exact UID, argv, parent chain and Linux process start-time checks, rechecked immediately before signals. Python build lacks pidfd APIs; no signal sent in first attempt. No process-name kill or unrelated process selection. Preserve original partial logs as NOT_COMPLETED; same full suite follows in release under original900s cap.','processes':[],'scratch':[]};tokens={}
def observed(pid):
 p=Path('/proc')/str(pid)
 try:
  fields=(p/'stat').read_text().rsplit(') ',1)[1].split();return fields[19],fields[0]
 except FileNotFoundError:return None
for pid,args in expected.items():
 p=Path('/proc')/str(pid);actual=[x.decode() for x in (p/'cmdline').read_bytes().split(b'\0') if x];assert actual==args and p.stat().st_uid==os.getuid();status=(p/'status').read_text();ppid=int(next(x.split()[1] for x in status.splitlines() if x.startswith('PPid:')));assert (pid!=28775 or ppid==28635) and (pid!=2814 or ppid==28775);tokens[pid]=observed(pid)[0];r['processes'].append({'pid':pid,'parentPid':ppid,'startTicks':tokens[pid],'arguments':actual})
for p in Path('/dev/shm').glob('FutureClassroom-*-2814-*'):
 assert p.is_dir() and not p.is_symlink() and p.stat().st_uid==os.getuid();r['scratch'].append({'path':str(p),'inode':p.stat().st_ino})
out.write_text(json.dumps(r,indent=2)+'\n')
for pid in [2814,28775]:
 state=observed(pid)
 if state and state[1]!='Z':assert state[0]==tokens[pid];os.kill(pid,signal.SIGINT)
until=time.monotonic()+10
while time.monotonic()<until:
 if all((s:=observed(pid)) is None or s[0]!=tokens[pid] or s[1]=='Z' for pid in tokens):break
 time.sleep(0.1)
else:raise RuntimeError('Original process exits not observed; no cleanup allowed')
for x in r['scratch']:
 p=Path(x['path'])
 if p.exists():assert p.stat().st_ino==x['inode'] and not p.is_symlink();shutil.rmtree(p)
 assert not p.exists()
d=json.loads((base/'chunk-lint-local-second.json').read_text());assert d['status']=='FAIL' and d['commands'][-1]['name']=='host-test'
r.update(status='PASS_CANCELLED_AND_CLEANED',originalTestOutcome='NOT_COMPLETED_NOT_PASS',originalRecordedExitCode=d['commands'][-1]['exitCode'],actualOriginalProcessesExited=True);out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:r[k] for k in ['status','originalTestOutcome','originalRecordedExitCode','actualOriginalProcessesExited']}))
