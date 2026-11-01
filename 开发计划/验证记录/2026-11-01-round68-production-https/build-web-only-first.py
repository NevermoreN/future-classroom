#!/usr/bin/env python3
"""Build source-bound R68 web-only loopback fixture ZIP in its own root."""
from pathlib import Path
import datetime
import hashlib
import importlib.util
import json
import os
import runpy
import signal
import stat
import subprocess
import sys
import time
import zipfile
sys.dont_write_bytecode=True
ROOT=Path('/tmp/fc-round68-production-https')
WORK=ROOT/'worktree'
EVIDENCE=ROOT/'evidence'
BUILD=ROOT/'web-build'
PROJECT=BUILD/'web'
FREEZE=EVIDENCE/'joint-implementation-freeze-fourth.json'
REPORT=EVIDENCE/'web-build-first.json'
BUDGET=900

def sha(data):return hashlib.sha256(data).hexdigest()
def need(value,message):
 if not value:raise AssertionError(message)
def ident(path):
 need(stat.S_ISREG(path.lstat().st_mode),'regular file required '+str(path))
 data=path.read_bytes()
 return {'path':str(path),'bytes':len(data),'sha256':sha(data)}
def write_new(path,data):
 path.parent.mkdir(parents=True,exist_ok=True)
 with path.open('xb') as out:out.write(data)
def save(value):REPORT.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n')
def source_check(rows):
 for row in rows:
  actual=ident(WORK/row['path'])
  need((actual['bytes'],actual['sha256'])==(row['bytes'],row['sha256']),'frozen source differs '+row['path'])
 return {'status':'PASS_ALL_FROZEN_INPUT_BYTES_UNCHANGED','count':len(rows)}
def run(label,args,cwd,deadline,record,environment=None):
 need(time.monotonic()<deadline,'original900s web deadline')
 stdout=EVIDENCE/(label+'.stdout.log');stderr=EVIDENCE/(label+'.stderr.log')
 row={'label':label,'args':args,'cwd':str(cwd),'status':'RUNNING','originalBudgetSeconds':BUDGET}
 record['commands'].append(row);save(record)
 started=time.monotonic()
 with stdout.open('xb') as out,stderr.open('xb') as err:
  child=subprocess.Popen(args,cwd=cwd,stdout=out,stderr=err,env=environment,start_new_session=True)
  row['pid']=child.pid;save(record)
  try:code=child.wait(timeout=max(.001,deadline-time.monotonic()))
  except subprocess.TimeoutExpired:
   os.killpg(child.pid,signal.SIGTERM)
   try:child.wait(timeout=5)
   except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=5)
   row['status']='TIMEOUT';row['actualExit']=True;row['exitCode']=child.returncode
  else:row.update(status='PASS' if code==0 else 'FAIL',actualExit=True,exitCode=code)
 row['elapsedSeconds']=time.monotonic()-started
 row['stdout']=ident(stdout);row['stderr']=ident(stderr)
 save(record)
 need(row['status']=='PASS',label+' actual exit failure (original raw logs retained)')
 print(label+' ACTUAL_EXIT0',flush=True)
 return stdout.read_text().strip()

def main():
 need(not BUILD.exists() and not REPORT.exists(),'fresh owned build/report required')
 freeze_bytes=FREEZE.read_bytes();freeze=json.loads(freeze_bytes);rows=freeze['workingFiles']
 source_check(rows)
 web=[r for r in rows if r['path'].startswith('web/')]
 need(len(web)==65,'exact current frozen web membership')
 need(all(not any(part in ('node_modules','dist','__pycache__','.npm-cache','.vite') for part in Path(r['path']).parts) for r in web),'no cache/build input copied')
 package=json.loads((WORK/'web/package.json').read_text())
 need(package['packageManager']=='npm@10.9.3' and package['scripts']['build']=='tsc --noEmit && node scripts/licenses.mjs && vite build && node scripts/budget.mjs','unchanged locked build/license/budget script')
 BUILD.mkdir();PROJECT.mkdir();write_new(BUILD/'.owner',b'future-classroom-owned-r68-web-build/v1\n')
 record={'schemaVersion':1,'status':'RUNNING','sourceFreeze':{'path':str(FREEZE),'bytes':len(freeze_bytes),'sha256':sha(freeze_bytes)},
  'sourceBaseCommit':freeze['baseCommit'],'scope':'Source-bound static web fixture for ordinary HTTPS probe; no EXE packaging, upload, Windows, TLS, teacher auth, campus or OS-runtime claim.',
  'originalBudgetSeconds':BUDGET,'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'commands':[],
  'frozenInputsBefore':source_check(rows),'frozenWebSources':web,'copiedSourceCount':len(web),
  'cachePolicy':'No source node_modules/dist/cache copied; npm cache explicitly scoped under this owned web-build root.',
  'controlledEnvironment':{'FC_WEB_BASE':'/fc/'},'credentials':'No credential file/config/environment dump or secret material read by controller.'}
 with REPORT.open('x') as out:json.dump(record,out,ensure_ascii=False,indent=2);out.write('\n')
 started=time.monotonic();deadline=started+BUDGET
 try:
  for row in web:
   data=(WORK/row['path']).read_bytes();dest=BUILD/row['path'];write_new(dest,data)
   actual=ident(dest);need((actual['bytes'],actual['sha256'])==(row['bytes'],row['sha256']),'exact controlled source copy')
  record['nodeVersion']=run('web-node-version-first',['node','--version'],PROJECT,deadline,record)
  record['npmVersion']=run('web-npm-version-first',['npm','--version'],PROJECT,deadline,record)
  need(record['nodeVersion'].startswith('v22.') and record['npmVersion']=='10.9.3','declared current Node22/npm10.9.3 toolchain')
  env=os.environ.copy();env['FC_WEB_BASE']='/fc/'
  run('web-npm-ci-first',['npm','ci','--cache',str(BUILD/'.npm-cache')],PROJECT,deadline,record,env)
  run('web-npm-build-first',['npm','run','build'],PROJECT,deadline,record,env)
  budget=json.loads((PROJECT/'build-budget.json').read_text())
  need(budget['status']=='PASS' and budget['totalLimitBytes']==5*1024*1024 and budget['gzipJsCssLimitBytes']==512*1024
   and budget['totalBytes']<=budget['totalLimitBytes'] and budget['gzipJsCssBytes']<=budget['gzipJsCssLimitBytes'],'unchanged real build budget')
  record['buildBudget']={'identity':ident(PROJECT/'build-budget.json'),'value':budget}
  packager=WORK/'scripts/package-probe.py';reader=WORK/'scripts/windows-tls-business-probe.py'
  record['packagingRuleSource']=ident(packager);record['readerRuleSource']=ident(reader)
  # Reuse the existing manifest/licence validators unchanged, pointing only its
  # documented source data root at our isolated copied web project.
  pack=runpy.run_path(str(packager));payload_fn=pack['frontend_payloads'];payload_fn.__globals__['ROOT']=BUILD
  payloads,summary=payload_fn(PROJECT/'dist')
  need(all(name.startswith('web/') for name in payloads),'web-only ZIP membership')
  zip_path=BUILD/'ordinary-https-web-first.zip'
  with zipfile.ZipFile(zip_path,'x',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as archive:
   for name,data in sorted(payloads.items()):pack['archive_bytes'](archive,name,data)
  spec=importlib.util.spec_from_file_location('fc_r68_web_reader',reader)
  module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
  manifest,payload,validated=module.read_web_package(zip_path)
  need(manifest==payloads['web/fc-assets.json'] and {name:data for name,data in payload}=={name[4:]:data for name,data in payloads.items() if name not in ('web/fc-assets.json','web/THIRD-PARTY-NOTICES.txt')},'existing runner exact read_web_package proof')
  with zipfile.ZipFile(zip_path) as archive:
   need(set(archive.namelist())==set(payloads) and len(archive.namelist())==len(payloads),'exact only declared web ZIP members')
   need(archive.testzip() is None,'actual ZIP CRC')
   for name,data in payloads.items():need(archive.read(name)==data,'actual ZIP bytes '+name)
  record['outputs']=[{'path':name,'bytes':len(data),'sha256':sha(data)} for name,data in sorted(payloads.items())]
  record['distOutputs']=[ident(p) for p in sorted((PROJECT/'dist').rglob('*')) if p.is_file()]
  record['thirdPartyNotices']=ident(PROJECT/'THIRD-PARTY-NOTICES.txt')
  record['staticZip']=ident(zip_path);record['existingRunnerPackageValidation']={'status':'PASS','value':validated,'payloadCount':len(payload)}
  copy_changes=[]
  for row in web:
   got=ident(BUILD/row['path'])
   if (got['bytes'],got['sha256'])!=(row['bytes'],row['sha256']):copy_changes.append({'path':row['path'],'after':got})
  need(all(r['path']=='web/THIRD-PARTY-NOTICES.txt' for r in copy_changes),'build may only regenerate its declared notices input')
  record['isolatedGeneratedNoticeChanges']=copy_changes
  record['frozenInputsAfter']=source_check(rows)
  need(FREEZE.read_bytes()==freeze_bytes,'joint freeze immutable during web build')
  record['status']='PASS_STATIC_BUILD_AND_WEB_ONLY_ZIP_SOURCE_BOUND'
 except Exception as error:
  record['status']='FAIL';record['error']={'kind':type(error).__name__,'message':str(error)}
 finally:
  record['elapsedSeconds']=time.monotonic()-started;record['completedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
  record['helper']=ident(Path(__file__))
  try:record['frozenInputsFinal']=source_check(rows)
  except Exception as error:record['status']='FAIL';record['finalSourceGuardError']=str(error)
  save(record)
 print(json.dumps({'status':record['status'],'report':str(REPORT),'reportSha256':sha(REPORT.read_bytes()),'zip':record.get('staticZip'),'elapsedSeconds':record['elapsedSeconds'],'sourceUnchanged':record.get('frozenInputsFinal')},ensure_ascii=False),flush=True)
 return 0 if record['status'].startswith('PASS_') else 1
if __name__=='__main__':raise SystemExit(main())
