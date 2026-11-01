from pathlib import Path
import subprocess,json,urllib.request,urllib.error,urllib.parse,base64,hashlib,time,datetime
import argparse
parser=argparse.ArgumentParser();parser.add_argument('--source-sha',required=True);parser.add_argument('--workflow',choices=['windows-msvc.yml','windows-win7-msvc.yml'],required=True);args=parser.parse_args()
workflow_file=args.workflow
base=Path('/tmp/fc-round67-ci-evidence');path=base/('FutureClassroom-round67-'+workflow_file.removesuffix('.yml')+'-dispatch.json')
assert not path.exists(),'Do not repeat dispatch; inspect original report/run first'
head=subprocess.run(['git','rev-parse','HEAD'],capture_output=True,check=True).stdout.decode().strip();assert head==args.source_sha and len(head)==40
assert subprocess.check_output(['git','ls-remote','origin','refs/heads/main']).decode().split()[0]==head
assert not subprocess.check_output(['git','status','--porcelain'])
credentials=subprocess.run(['git','credential','fill'],input=b'protocol=https\nhost=github.com\n\n',capture_output=True,timeout=15)
values=dict(line.split('=',1) for line in credentials.stdout.decode().splitlines() if '=' in line);token=values.get('password');assert token
root='https://api.github.com/repos/NevermoreN/future-classroom'
report={'schemaVersion':1,'scope':'Single manual isolated MSVC compile-only dispatch for selected Modern or Win7 candidate baseline; no product/test EXE execution or formal three-OS runtime claim','sourceSha':head,'ref':'main','workflowPath':'.github/workflows/'+workflow_file,'apiVersion':'2026-03-10','calls':[],'status':'PREPARING','createdUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
def save():
 tmp=path.with_suffix('.partial.json');tmp.write_text(json.dumps(report,indent=2)+'\n');tmp.replace(path)
def call(method,route,body=None):
 data=json.dumps(body).encode() if body is not None else None;headers={'Authorization':'Bearer '+token,'Accept':'application/vnd.github+json','X-GitHub-Api-Version':'2026-03-10','User-Agent':'FutureClassroom-development-validation'}
 if data is not None:headers['Content-Type']='application/json'
 request=urllib.request.Request(root+route,data=data,headers=headers,method=method);entry={'method':method,'route':route}
 if body is not None:entry['body']=body
 report['calls'].append(entry);save()
 try:
  with urllib.request.urlopen(request,timeout=25) as response:raw=response.read(2*1024*1024+1);status=response.status
 except urllib.error.HTTPError as error:raw=error.read(2*1024*1024+1);status=error.code
 assert len(raw)<=2*1024*1024
 assert token.encode() not in raw
 entry.update({'httpStatus':status,'response':{'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'base64':base64.b64encode(raw).decode()}});save()
 return status,json.loads(raw) if raw else None
try:
 status,ref=call('GET','/git/ref/heads/main');assert status==200 and ref['object']['sha']==head
 status,workflow=call('GET','/actions/workflows/'+workflow_file+'');assert status==200 and workflow['path']==report['workflowPath'] and workflow['state']=='active'
 status,runs=call('GET','/actions/workflows/'+workflow_file+'/runs?event=workflow_dispatch&branch=main&head_sha='+head+'&per_page=20');assert status==200
 baseline={r['id'] for r in runs.get('workflow_runs',[])};report['baselineRunIds']=sorted(baseline);assert not baseline,'Existing run must be observed instead of duplicate dispatch'
 report['status']='DISPATCH_REQUEST_ABOUT_TO_SEND';save()
 status,dispatched=call('POST','/actions/workflows/'+workflow_file+'/dispatches',{'ref':'main','inputs':{'source_sha':head}})
 assert status in (200,204),status
 report['status']='DISPATCH_ACCEPTED';report['dispatchHttpStatus']=status
 if dispatched and dispatched.get('workflow_run_id'):report['runId']=dispatched['workflow_run_id'];report['runUrl']=dispatched.get('run_url');report['htmlUrl']=dispatched.get('html_url')
 save()
 until=time.monotonic()+60
 while time.monotonic()<until:
  if report.get('runId'):
   status,run=call('GET','/actions/runs/'+str(report['runId']))
   assert status==200
  else:
   status,listing=call('GET','/actions/workflows/'+workflow_file+'/runs?event=workflow_dispatch&branch=main&head_sha='+head+'&per_page=20');assert status==200
   fresh=[r for r in listing.get('workflow_runs',[]) if r['id'] not in baseline]
   assert len(fresh)<=1,'Ambiguous run; never redispatch'
   if not fresh:time.sleep(2);continue
   run=fresh[0];report['runId']=run['id']
  assert run['head_sha']==head and run['event']=='workflow_dispatch' and run['head_branch']=='main' and run['workflow_id']==workflow['id']
  report.update({'status':'RUN_IDENTIFIED','runId':run['id'],'htmlUrl':run['html_url'],'runStatus':run['status'],'conclusion':run.get('conclusion'),'headShaConfirmed':True});save();break
 else:raise RuntimeError('Accepted dispatch not yet identified; inspect original API state without repost')
except Exception as error:
 report['errorKind']=type(error).__name__;report['observationError']='Inspect original report; do not repeat POST';save();raise
print(json.dumps({k:report.get(k) for k in ['status','dispatchHttpStatus','sourceSha','runId','htmlUrl','runStatus','conclusion','headShaConfirmed']}),flush=True)
