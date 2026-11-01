"""Run exactly one selected dispatch and preserve its actual terminal streams."""
from pathlib import Path
import argparse,datetime,hashlib,json,os,signal,subprocess,time
p=argparse.ArgumentParser();p.add_argument('--workflow',choices=['windows-msvc.yml','windows-win7-msvc.yml'],required=True);a=p.parse_args()
E=Path('/tmp/fc-round74-ci-evidence');assert (E/'.owner').read_bytes()==b'future-classroom-owned-r74-ci-evidence/v1\n'
label='modern' if a.workflow=='windows-msvc.yml' else 'win7';report=E/(label+'-dispatch-operation-first.json');assert not report.exists()
v=dict(status='RUNNING',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),originalBudgetSeconds=180,scope='Single bounded workflow dispatch only; never retry POST on failure.')
def save():report.write_text(json.dumps(v,indent=2)+'\n')
command=['python3','-B',str(E/'dispatch-ci-once.py'),'--source-sha','2dfb2e7e07ace64c61abf66c9a423aa815de7110','--workflow',a.workflow];v['command']=command;save();start=time.monotonic()
with (E/(label+'-dispatch-operation-first.stdout.log')).open('xb') as out,(E/(label+'-dispatch-operation-first.stderr.log')).open('xb') as err:
 child=subprocess.Popen(command,stdout=out,stderr=err,start_new_session=True);v['pid']=child.pid;save()
 try:code=child.wait(timeout=180);v['timedOut']=False
 except subprocess.TimeoutExpired:
  v['timedOut']=True;os.killpg(child.pid,signal.SIGTERM)
  try:code=child.wait(timeout=10)
  except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);code=child.wait(timeout=10)
v.update(status='PASS' if code==0 and not v['timedOut'] else 'FAIL',actualExit=True,exitCode=code,elapsedSeconds=time.monotonic()-start)
for name in ('stdout','stderr'):
 q=E/(label+'-dispatch-operation-first.'+name+'.log');data=q.read_bytes();v[name]=dict(path=q.name,bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
save();print(json.dumps(v));raise SystemExit(0 if v['status']=='PASS' else 1)
