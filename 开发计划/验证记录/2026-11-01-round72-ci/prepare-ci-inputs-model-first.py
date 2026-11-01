#!/usr/bin/env python3
"""Pure synthetic R72 gate models. No artifact, ZIP, remote or executable access."""
from pathlib import Path
import copy,hashlib,json,runpy,datetime
EV=Path(__file__).resolve().parent

def main():
 ns=runpy.run_path(str(EV/'prepare-ci-inputs-first.py'));ns=ns['download_gate'].__globals__
 ns['SOURCE']='a'*40
 for i,tag in enumerate(('modern','win7'),start=1):ns['BASELINES'][tag]['run']=1000+i
 # Original immutable helper functions are used with synthetic I/O adapters only.
 original={k:ns[k] for k in ('bootstrap','read','api','fact')};checks=[]
 def exercise(tag,change=None,expected=None,zip_allowed=False):
  cfg=ns['BASELINES'][tag];attempt=1;jobid=101;artifactid=201;size=100;digest='a'*64
  paths={k:EV/('synthetic-'+tag+'-'+k+'.json') for k in ('run','jobs','artifacts','downloadOperation')}
  dispatch=dict(status='RUN_IDENTIFIED',sourceSha=ns['SOURCE'],runId=cfg['run'],workflowPath=cfg['workflow'])
  run=dict(status='completed',conclusion='success',id=cfg['run'],head_sha=ns['SOURCE'],path=cfg['workflow'],event='workflow_dispatch',run_attempt=attempt)
  job=dict(id=jobid,run_id=cfg['run'],name=cfg['job'],status='completed',conclusion='success')
  artifact=dict(id=artifactid,name=f"{cfg['prefix']}-i686-{ns['SOURCE']}-{cfg['run']}-{attempt}",expired=False,
   workflow_run=dict(id=cfg['run'],head_sha=ns['SOURCE']),size_in_bytes=size,digest='sha256:'+digest)
  downloaded=dict(status='PASS',sourceSha=ns['SOURCE'],runId=cfg['run'],artifactId=artifactid,expectedBytes=size,bytes=size,expectedSha256=digest,sha256=digest)
  command=['python3','-B',str(EV/'download-original.py'),'--baseline',tag,'--source-sha',ns['SOURCE'],'--run-id',str(cfg['run']),
   '--artifact-id',str(artifactid),'--expected-bytes',str(size),'--expected-sha256',digest]
  operation=dict(status='PASS',exitCode=0,actualExit=True,command=command)
  fixture=dict(dispatch=dispatch,run=run,jobs={'jobs':[job]},artifacts={'artifacts':[artifact]},downloaded=downloaded,downloadOperation=operation)
  if change:change(fixture)
  calls=[]
  def read(path):
   name=Path(path).name
   if name.startswith('FutureClassroom-round72-'):return fixture['dispatch']
   if name==tag+'-download-original-first.json':return fixture['downloaded']
   for key,path_value in paths.items():
    if Path(path)==path_value:
     if fixture[key] is None:raise ns['Waiting']('SYNTHETIC_MISSING')
     return fixture[key]
   raise AssertionError('unexpected synthetic path')
  def fact(path):
   calls.append(str(path));return dict(path=str(path),bytes=size,sha256=digest)
  ns.update(bootstrap=lambda:None,read=read,api=read,fact=fact)
  outcome=None
  try:ns['download_gate'](tag,paths)
  except (ns['Waiting'],ns['Invalid']) as error:outcome=type(error).__name__
  finally:ns.update(original)
  assert outcome==expected,(tag,outcome,expected)
  zip_calls=[p for p in calls if p.endswith('-original.zip')]
  assert bool(zip_calls)==zip_allowed,'ZIP was accessed before terminal gates'
 for tag in ('modern','win7'):
  exercise(tag,zip_allowed=True);checks.append(tag+' synthetic complete gate permits one exact ZIP fact only after terminal receipt')
 cases=[
  ('wrong dispatch source',lambda f:f['dispatch'].update(sourceSha='0'*40),'Invalid'),
  ('wrong dispatch run',lambda f:f['dispatch'].update(runId=1),'Invalid'),
  ('wrong downloaded source',lambda f:f['downloaded'].update(sourceSha='0'*40),'Invalid'),
  ('queued run',lambda f:f['run'].update(status='queued',conclusion=None),'Waiting'),
  ('running job',lambda f:f['jobs']['jobs'][0].update(status='in_progress',conclusion=None),'Waiting'),
  ('wrong run',lambda f:f['run'].update(id=1),'Invalid'),
  ('stale source',lambda f:f['run'].update(head_sha='0'*40),'Invalid'),
  ('failed CI',lambda f:f['run'].update(conclusion='failure'),'Invalid'),
  ('wrong workflow',lambda f:f['run'].update(path='.github/workflows/other.yml'),'Invalid'),
  ('duplicate artifact',lambda f:f['artifacts']['artifacts'].append(copy.deepcopy(f['artifacts']['artifacts'][0])),'Invalid'),
  ('expired artifact',lambda f:f['artifacts']['artifacts'][0].update(expired=True),'Invalid'),
  ('wrong artifact source',lambda f:f['artifacts']['artifacts'][0]['workflow_run'].update(head_sha='0'*40),'Invalid'),
  ('malformed digest',lambda f:f['artifacts']['artifacts'][0].update(digest='sha256:bad'),'Invalid'),
  ('running download',lambda f:f['downloaded'].update(status='RUNNING'),'Waiting'),
  ('failed download',lambda f:f['downloaded'].update(status='FAIL'),'Invalid'),
  ('short downloaded bytes',lambda f:f['downloaded'].update(bytes=99),'Invalid'),
  ('wrong download digest',lambda f:f['downloaded'].update(sha256='0'*64),'Invalid'),
  ('missing parent receipt',lambda f:f.update(downloadOperation=None),'Waiting'),
  ('parent still running',lambda f:f['downloadOperation'].update(status='RUNNING'),'Waiting'),
  ('parent exit failure',lambda f:f['downloadOperation'].update(exitCode=1),'Invalid'),
  ('parent not actually exited',lambda f:f['downloadOperation'].update(actualExit=False),'Invalid'),
  ('stale command baseline',lambda f:f['downloadOperation']['command'].__setitem__(5,'other'),'Invalid'),
 ]
 for name,change,expected in cases:
  exercise('modern',change,expected);checks.append(name+' blocks all ZIP access')
 # Real local default invocations were separately observed WAIT by preparer;
 # this model report is explicitly not a real run/artifact/static PASS.
 helper=EV/'prepare-ci-inputs-first.py';data=helper.read_bytes()
 report=dict(schemaVersion=1,status='PASS_PURE_GATE_MODELS_ONLY_NO_ARTIFACT',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
  helper=dict(path=helper.name,bytes=len(data),sha256=hashlib.sha256(data).hexdigest()),checkCount=len(checks),checks=checks,
  scope='Synthetic in-memory I/O only. No actual ZIP read, API request, download, static artifact audit, Windows, SSH, Cargo, Git mutation or product execution. Missing original terminal inputs are WAIT, not a product FAIL or PASS.')
 path=EV/'prepare-ci-inputs-model-first.json'
 with path.open('x') as output:json.dump(report,output,indent=2);output.write('\n')
 print(json.dumps(dict(status=report['status'],checkCount=len(checks),reportSha256=hashlib.sha256(path.read_bytes()).hexdigest(),helperSha256=report['helper']['sha256'])))
if __name__=='__main__':main()
