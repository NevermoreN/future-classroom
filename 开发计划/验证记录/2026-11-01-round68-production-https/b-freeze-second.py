from pathlib import Path
import hashlib,json,subprocess,datetime
root=Path('/tmp/fc-round68-production-https/worktree');ev=root.parent/'evidence'
base='3fd34d5'
paths=[
'crates/fc-bootstrap/src/backend.rs','crates/fc-bootstrap/src/backend/https_settings.rs',
'crates/fc-bootstrap/src/backend/web.rs','crates/fc-bootstrap/src/backend/web/runtime.rs',
'crates/fc-bootstrap/src/backend/web/runtime_tests.rs','crates/fc-bootstrap/src/https_runtime.rs',
'crates/fc-bootstrap/src/lib.rs','crates/fc-bootstrap/src/web_application/voice_files.rs',
'crates/fc-bootstrap/src/web_transfer.rs','crates/fc-bootstrap/src/web_transfer/downloads.rs',
'crates/fc-bootstrap/src/web_transfer/executor.rs','crates/fc-bootstrap/src/web_transfer/network.rs',
'crates/fc-bootstrap/src/web_transfer/uploads.rs','crates/fc-bootstrap/src/web_transfer/uploads/tests.rs',
'crates/fc-bootstrap/src/web_transfer/live_cleanup.rs','crates/fc-storage/src/business/https_settings.rs',
'crates/fc-storage/src/business/https_settings/tests.rs']
cmd=['rustfmt','--edition','2021','--config','skip_children=true']+paths
r=subprocess.run(cmd,cwd=root,capture_output=True,timeout=60)
for suffix,data in [('stdout.log',r.stdout),('stderr.log',r.stderr)]:
 with (ev/f'b-rustfmt-second.{suffix}').open('xb') as f:f.write(data)
assert r.returncode==0,(r.returncode,r.stderr[-2000:])
cmd.insert(1,'--check');r=subprocess.run(cmd,cwd=root,capture_output=True,timeout=60)
for suffix,data in [('stdout.log',r.stdout),('stderr.log',r.stderr)]:
 with (ev/f'b-rustfmt-check-second.{suffix}').open('xb') as f:f.write(data)
assert r.returncode==0,(r.returncode,r.stderr[-2000:])
def identity(data):return {'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
rows=[]
for rel in paths:
 data=(root/rel).read_bytes();after=ev/'b-source-after-second'/rel;after.parent.mkdir(parents=True,exist_ok=True)
 with after.open('xb') as f:f.write(data)
 old=subprocess.run(['git','show',f'{base}:{rel}'],cwd=root,capture_output=True)
 before=None
 if old.returncode==0:
  out=ev/'b-source-base-second'/rel;out.parent.mkdir(parents=True,exist_ok=True)
  with out.open('xb') as f:f.write(old.stdout)
  before={'path':str(out.relative_to(ev)),**identity(old.stdout)}
 rows.append({'relativePath':rel,'before':before,'after':{'path':str(after.relative_to(ev)),**identity(data)}})
report={'schema':1,'status':'FROZEN_NOT_COMPILED_OR_EXECUTED','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
'base':subprocess.check_output(['git','rev-parse',base],cwd=root,text=True).strip(),
'source':rows,'rustfmtCheckExitCode':r.returncode,'cargoInvoked':False,'windowsInvoked':False,
'changes':['single configured HTTPS OS thread and existing shared Http worker adapter','atomic negative Web/session execution before and after ledger including queued Writer jobs','live Hub cleanup while same original workers remain available, MaterialReady/suppressed Begin/download pin regression','single original stop origin UI request_stop_at / HTTP timeout / after native Drop, sticky Faulted claim','machine format2 explicit CIDR persistence, legacy format1 source policy None, no automatic activation'],
'notProven':['GNU typecheck/link','Windows execution','ordinary authenticated wire flow','live owner native shutdown timing']}
with (ev/'b-implementation-freeze-second.json').open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'files':len(rows),'freezeSha256':identity((ev/'b-implementation-freeze-second.json').read_bytes())['sha256'],'status':report['status']}))
