#!/usr/bin/env python3
"""R72 terminal-download gate and source-bound static/runtime command preparation.
Default mode only reads local originals and prints a plan. No network, dispatch,
product/test EXE, SSH, Cargo or Git mutation. --execute-static explicitly invokes
only the retained independent static artifact auditor after every gate passes.
Missing/still-running inputs yield WAIT with no output report or ZIP access.
"""
from pathlib import Path
import argparse,datetime,hashlib,json,re,subprocess,sys
sys.dont_write_bytecode=True
EV=Path(__file__).resolve().parent
REPO=Path('/workspace/共享/Eliora/未来教室计划')
SOURCE=None  # Required --source-sha; no implicit baseline or old run.
OWNER=b'future-classroom-owned-r72-ci-evidence/v1\n'
AUDITOR_SHA='c136efb0ecb5dc1dbffadadae31b933c43ac3d298c308921dd7c8e409fc75527'
DOWNLOADER_SHA='328ddf669176a8101f8094ee783d8207921f7ef2bca44c993ea8c937a63e0e62'
PROBE_SHA='f176a022bf90565be7074f53a2d502d6519afd1748cdd2022f608a217d485c14'
BASELINES={
 'modern':dict(baseline='Modern',workflow='.github/workflows/windows-msvc.yml',job='compile-i686',prefix='future-classroom-msvc'),
 'win7':dict(baseline='Win7',workflow='.github/workflows/windows-win7-msvc.yml',job='compile-win7-i686',prefix='future-classroom-win7-msvc')}
class Waiting(Exception):pass
class Invalid(Exception):pass
def need(ok,code):
 if not ok:raise Invalid(code)
def sha(data):return hashlib.sha256(data).hexdigest()
def read(path):
 path=Path(path);need(path.resolve().is_relative_to(EV.resolve()),'INPUT_OUTSIDE_R72_ROOT')
 if not path.exists():raise Waiting('MISSING_'+path.name)
 need(path.is_file() and not path.is_symlink(),'NONREGULAR_INPUT')
 try:return json.loads(path.read_text(encoding='utf-8-sig'))
 except json.JSONDecodeError:raise Waiting('INCOMPLETE_JSON_'+path.name) from None
def api(path):
 v=read(path);need(v.get('isError') is not True,'API_WRAPPER_ERROR');v=v.get('structuredContent',v)
 if isinstance(v,dict) and isinstance(v.get('content'),str):
  try:v=json.loads(v['content'])
  except json.JSONDecodeError:raise Waiting('INCOMPLETE_API_CONTENT_'+Path(path).name) from None
 return v
def fact(path):
 path=Path(path);need(path.is_file() and not path.is_symlink(),'NONREGULAR_FACT')
 h=hashlib.sha256();size=0
 with path.open('rb') as stream:
  while data:=stream.read(1024*1024):h.update(data);size+=len(data)
 return dict(path=str(path),bytes=size,sha256=h.hexdigest())
def bootstrap():
 need((EV/'.owner').read_bytes()==OWNER,'R72_OWNER_MISMATCH')
 need(sha((EV/'verify-ci-artifact-first.py').read_bytes())==AUDITOR_SHA,'AUDITOR_CHANGED')
 need(sha((EV/'download-original.py').read_bytes())==DOWNLOADER_SHA,'DOWNLOADER_CHANGED')
 need(sha((REPO/'scripts/windows-probe.py').read_bytes())==PROBE_SHA,'WINDOWS_CONTROLLER_SOURCE_CHANGED')
def terminal(v,name):
 if v.get('status') in ('RUNNING','STARTED','queued','in_progress','waiting','pending','requested'):
  raise Waiting('STILL_RUNNING_'+name)
def download_gate(tag,paths):
 bootstrap();cfg=BASELINES[tag]
 dispatch=read(EV/('FutureClassroom-round72-'+('windows-msvc' if tag=='modern' else 'windows-win7-msvc')+'-dispatch.json'))
 need(dispatch.get('status')=='RUN_IDENTIFIED' and dispatch.get('sourceSha')==SOURCE and dispatch.get('runId')==cfg['run']
  and dispatch.get('workflowPath')==cfg['workflow'],'EXACT_SINGLE_DISPATCH_BINDING')
 run=api(paths['run']);terminal(run,'RUN')
 need(run.get('status')=='completed' and run.get('conclusion')=='success' and run.get('id')==cfg['run']
  and run.get('head_sha')==SOURCE and run.get('path')==cfg['workflow'] and run.get('event')=='workflow_dispatch','TERMINAL_RUN_IDENTITY')
 attempt=run.get('run_attempt');need(type(attempt) is int and attempt>0,'RUN_ATTEMPT')
 jobs=api(paths['jobs']);jobs=jobs.get('jobs',jobs) if isinstance(jobs,dict) else jobs
 need(isinstance(jobs,list),'JOBS_SHAPE')
 selected=[j for j in jobs if j.get('run_id')==cfg['run'] and j.get('name')==cfg['job']]
 need(len(selected)==1,'EXACT_JOB_UNIQUE');job=selected[0];terminal(job,'JOB')
 need(job.get('status')=='completed' and job.get('conclusion')=='success' and type(job.get('id')) is int,'TERMINAL_JOB_IDENTITY')
 artifacts=api(paths['artifacts']);artifacts=artifacts.get('artifacts',artifacts) if isinstance(artifacts,dict) else artifacts
 need(isinstance(artifacts,list),'ARTIFACTS_SHAPE')
 expected_name=f"{cfg['prefix']}-i686-{SOURCE}-{cfg['run']}-{attempt}"
 selected=[a for a in artifacts if a.get('name')==expected_name]
 need(len(selected)==1,'EXACT_ARTIFACT_UNIQUE');artifact=selected[0]
 need(artifact.get('expired') is False and artifact.get('workflow_run',{}).get('id')==cfg['run']
  and artifact['workflow_run'].get('head_sha')==SOURCE and type(artifact.get('id')) is int,'ARTIFACT_SOURCE_IDENTITY')
 size=artifact.get('size_in_bytes');digest=artifact.get('digest')
 need(type(size) is int and 0<size<=512*1024*1024 and isinstance(digest,str)
  and re.fullmatch(r'sha256:[0-9a-f]{64}',digest),'ARTIFACT_SIZE_DIGEST')
 downloaded=read(EV/(tag+'-download-original-first.json'));terminal(downloaded,'DOWNLOAD')
 need(downloaded.get('status')=='PASS' and downloaded.get('sourceSha')==SOURCE and downloaded.get('runId')==cfg['run'] and downloaded.get('artifactId')==artifact['id']
  and downloaded.get('expectedBytes')==downloaded.get('bytes')==size
  and downloaded.get('expectedSha256')==downloaded.get('sha256')==digest[7:],'TERMINAL_DOWNLOAD_EXACT')
 operation=read(paths['downloadOperation']);terminal(operation,'DOWNLOAD_CONTROLLER')
 need(operation.get('status') in ('PASS','PASS_COMMAND') and type(operation.get('exitCode')) is int
  and operation['exitCode']==0 and operation.get('actualExit',True) is True,'DOWNLOAD_CONTROLLER_TERMINAL_EXIT0')
 cmd=operation.get('command',operation.get('args'));need(isinstance(cmd,list),'DOWNLOAD_CONTROLLER_COMMAND')
 need(str(EV/'download-original.py') in cmd,'EXACT_DOWNLOADER_COMMAND')
 for key,value in [('--source-sha',SOURCE),('--baseline',tag),('--run-id',str(cfg['run'])),('--artifact-id',str(artifact['id'])),
  ('--expected-bytes',str(size)),('--expected-sha256',digest[7:])]:
  need(cmd.count(key)==1 and cmd.index(key)+1<len(cmd) and cmd[cmd.index(key)+1]==value,'DOWNLOAD_COMMAND_IDENTITY')
 # Never stat/hash/open the ZIP before BOTH original download and controller are terminal.
 zipfact=fact(EV/(tag+'-original.zip'))
 need(zipfact['bytes']==size and zipfact['sha256']==digest[7:],'WHOLE_TERMINAL_ZIP_IDENTITY')
 return dict(runId=cfg['run'],jobId=job['id'],artifactId=artifact['id'],runAttempt=attempt,zip=zipfact,
  metadata={k:fact(paths[k]) for k in ('run','jobs','artifacts')},downloadOperation=fact(paths['downloadOperation']))
def static_runtime_inputs(tag):
 bootstrap();cfg=BASELINES[tag];audit=read(EV/(tag+'-original-independent-first.json'));terminal(audit,'STATIC_AUDIT')
 need(audit.get('status')=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN' and audit.get('sourceSha')==SOURCE
  and audit.get('baseline')==cfg['baseline'] and audit.get('runId')==cfg['run']
  and all(p.get('status')=='PASS' for p in audit.get('phases',[])) and len(audit.get('phases',[]))==6,'EXACT_STATIC_AUDIT_PASS')
 need(audit.get('helper',{}).get('sha256')==AUDITOR_SHA,'STATIC_AUDITOR_IDENTITY')
 downloaded=read(EV/(tag+'-download-original-first.json'));terminal(downloaded,'DOWNLOAD')
 need(downloaded.get('status')=='PASS' and downloaded.get('sourceSha')==SOURCE and downloaded.get('runId')==cfg['run'] and downloaded.get('artifactId')==audit.get('artifactId')
  and downloaded.get('bytes')==audit['zip']['bytes'] and downloaded.get('sha256')==audit['zip']['sha256'],'STATIC_DOWNLOAD_BINDING')
 normal=audit['normalExecutable'];exe=Path(normal['localPath']);meta=Path(audit['testMetadata']['path'])
 need(exe.is_relative_to(EV) and meta.is_relative_to(EV),'R72_RUNTIME_INPUT_SCOPE')
 for expected in [dict(path=str(exe),bytes=normal['producer']['bytes'],sha256=normal['producer']['sha256']),audit['testMetadata']]:
  need(fact(expected['path'])==expected,'ACTUAL_RUNTIME_INPUT_BYTES')
 need(normal['producer']['role']=='product' and normal['peHeader']['subsystem']==2,'NORMAL_GUI_PRODUCT')
 need(audit.get('counts',{}).get('executables')==22 and audit['counts'].get('tests')==21,'COMPLETE_EXECUTABLE_SET')
 need(len(audit.get('executables',[]))==22,'COMPLETE_EXECUTABLE_ROWS')
 for item in audit['executables']:
  path=Path(item['localPath']);producer=item['producer']
  need(path.resolve().is_relative_to(EV.resolve()),'EXECUTABLE_OUTSIDE_R72_ROOT')
  need(fact(path)==dict(path=str(path),bytes=producer['bytes'],sha256=producer['sha256']),'ALL_RUNTIME_PE_BYTES_UNCHANGED')
 return audit,exe,meta

def main():
 global SOURCE
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--source-sha',required=True);p.add_argument('--baseline',choices=BASELINES,required=True)
 p.add_argument('--mode',choices=['static','runtime-inputs'],default='static');p.add_argument('--execute-static',action='store_true')
 for arg in ['run-metadata','job-metadata','artifact-metadata','download-operation']:p.add_argument('--'+arg,type=Path)
 a=p.parse_args();tag=a.baseline;cfg=BASELINES[tag]
 SOURCE=a.source_sha
 try:
  need(isinstance(SOURCE,str) and re.fullmatch(r'[0-9a-f]{40}',SOURCE),'INVALID_SOURCE_SHA')
  bootstrap()
  dispatch=read(EV/('FutureClassroom-round72-'+('windows-msvc' if tag=='modern' else 'windows-win7-msvc')+'-dispatch.json'))
  need(dispatch.get('status')=='RUN_IDENTIFIED' and dispatch.get('sourceSha')==SOURCE and dispatch.get('workflowPath')==cfg['workflow'] and type(dispatch.get('runId')) is int and dispatch['runId']>0,'EXACT_SINGLE_DISPATCH_BINDING')
  cfg['run']=dispatch['runId']
  if a.mode=='runtime-inputs':
   need(not a.execute_static,'STATIC_FLAG_WITH_RUNTIME_PLAN');audit,exe,meta=static_runtime_inputs(tag)
   print(json.dumps({'status':'PREPARED_RUNTIME_INPUTS_ONLY_NOT_EXECUTED','sourceSha':SOURCE,'baseline':tag,
    'runId':cfg['run'],'normal':fact(exe),'testMetadata':fact(meta),
    'runtimeControllersPrepared':False,
    'scope':'Root must separately prepare and execute source-bound Windows controllers serially. Win7-target on Win11 WOW64 is not Win7/10 OS proof.'}));return 0
  paths=dict(run=a.run_metadata or EV/(tag+'-run-final-first.json'),jobs=a.job_metadata or EV/(tag+'-jobs-first.json'),
   artifacts=a.artifact_metadata or EV/(tag+'-artifacts-first.json'),downloadOperation=a.download_operation or EV/(tag+'-download-operation-first.json'))
  bound=download_gate(tag,paths)
  out=EV/(tag+'-artifact-original');report=EV/(tag+'-original-independent-first.json')
  need(not out.exists() and not report.exists(),'FRESH_STATIC_OUTPUT_REQUIRED')
  cmd=['python3','-B',str(EV/'verify-ci-artifact-first.py'),'--source',SOURCE,'--baseline',cfg['baseline'],
   '--zip',bound['zip']['path'],'--zip-bytes',str(bound['zip']['bytes']),'--zip-sha256',bound['zip']['sha256'],
   '--run-id',str(bound['runId']),'--job-id',str(bound['jobId']),'--artifact-id',str(bound['artifactId']),
   '--run-metadata',str(paths['run']),'--job-metadata',str(paths['jobs']),'--artifact-metadata',str(paths['artifacts']),
   '--output-dir',str(out),'--report',str(report),'--repo',str(REPO)]
  if not a.execute_static:
   print(json.dumps({'status':'PREPARED_STATIC_COMMAND_ONLY_NOT_EXECUTED','sourceSha':SOURCE,'inputs':bound,'command':cmd}));return 0
  result=subprocess.run(cmd,cwd=REPO)
  return result.returncode
 except Waiting as error:
  print(json.dumps({'status':'WAIT','reasonCode':str(error),'reportCreated':False,'zipAccessed':False}));return 3
 except Invalid as error:
  print(json.dumps({'status':'BLOCKED_INVALID_INPUT','reasonCode':str(error),'reportCreated':False}));return 1
if __name__=='__main__':raise SystemExit(main())
