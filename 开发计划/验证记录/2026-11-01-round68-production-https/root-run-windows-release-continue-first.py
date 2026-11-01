"""Evidence-only full-suite collection. Nonzero harness exits remain overall FAIL.
Invoke only after root observes the parameterized second codegen terminal PASS.
"""
from pathlib import Path
import argparse,datetime,hashlib,importlib.util,json,re,subprocess,sys,time
sys.dont_write_bytecode=True
R=Path('/tmp/fc-round68-production-https');E=R/'evidence';W=R/'worktree';MAIN=Path('/workspace/共享/Eliora/未来教室计划')
PRIVATE=E/'windows-probe-continue-tests-private-first.py'
PRIVATE_SHA='3fcde8ac7b3408e151b5c2de41fabd8ea09e82fa3504fbef3c8f030a90151f1a'
ORIGINAL_HELPER=E/'root-run-windows-release-first.py'
HELPER_SHA='6def3870e12884d4aee726bc5db089eeea6e5d371cca17953714248440ecca69'
NORMAL=R/'target/i686-pc-windows-gnu/release/fc-bootstrap.exe'
ALLOWED_NON_RUST_DELTA={'scripts/windows-ordinary-https-probe.py'}

def require(value,message):
 if not value:raise RuntimeError(message)
def sha(path):
 h=hashlib.sha256()
 with path.open('rb') as f:
  while b:=f.read(1024*1024):h.update(b)
 return h.hexdigest()
def identity(p):return {'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p)}
def save(p,d,exclusive=False):
 with p.open('x' if exclusive else 'w',encoding='utf-8') as f:json.dump(d,f,ensure_ascii=False,indent=2);f.write('\n')
def owned(name):
 require(Path(name).name==name and name.endswith('.json'),'evidence report name must be a single JSON filename')
 return E/name
def same(f):return all((W/x['path']).is_file() and sha(W/x['path'])==x['sha256'] for x in f['workingFiles'])
def build(name):
 p=owned(name);d=json.loads(p.read_text());require(d.get('status')=='PASS' and d.get('exitCode')==0 and d.get('sourceUnchanged') is True,'build not actual terminal PASS: '+name)
 require(d.get('originalBudgetSeconds')==900 and d.get('elapsedSeconds',901)<=900,'original build budget failed: '+name)
 f=owned(d['sourceFreeze']);require(sha(f)==d['sourceFreezeSha256'],'build source manifest provenance failed')
 for kind in ['stdout','stderr']:
  log=p.with_name(p.stem+'.'+kind+'.log');require(log.is_file() and sha(log)==d[kind+'Sha256'] and log.stat().st_size==d[kind+'Bytes'],'complete original build output identity failed')
 return d,json.loads(f.read_text()),{'report':identity(p),'sourceFreeze':identity(f),'stdout':identity(p.with_name(p.stem+'.stdout.log')),'stderr':identity(p.with_name(p.stem+'.stderr.log'))}
def main():
 ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--execute',action='store_true');ap.add_argument('--freeze',required=True);ap.add_argument('--tests-report',required=True);ap.add_argument('--product-report',required=True);ap.add_argument('--label',required=True);ap.add_argument('--normal-identity-report',default='windows-ordinary-https-second.json');a=ap.parse_args()
 require(a.execute,'explicit --execute required; preparation must not connect Windows')
 require(re.fullmatch(r'[a-z0-9-]+',a.label) is not None,'fresh owned lowercase label required')
 paths={k:E/('windows-release-'+a.label+s) for k,s in [('operation','-operation.json'),('report','.json'),('stdout','.stdout.log'),('stderr','.stderr.log')]}
 require(not any(p.exists() for p in paths.values()),'original operation/result/raw files already exist; do not restart')
 freeze=owned(a.freeze);f=json.loads(freeze.read_text());require(len(f['workingFiles'])==595 and same(f),'exact current595 source mismatch')
 require(sha(PRIVATE)==PRIVATE_SHA and sha(ORIGINAL_HELPER)==HELPER_SHA,'private runner/helper changed')
 spec=importlib.util.spec_from_file_location('r68_original_evidence_guard',ORIGINAL_HELPER);guard=importlib.util.module_from_spec(spec);spec.loader.exec_module(guard)
 td,tf,tp=build(a.tests_report);pd,pf,pp=build(a.product_report)
 require(td['sourceFreezeSha256']==sha(freeze),'test codegen must bind exact selected new joint freeze')
 old={x['path']:x['sha256'] for x in pf['workingFiles']};new={x['path']:x['sha256'] for x in f['workingFiles']}
 require(old.keys()==new.keys(),'normal/new source input set changed')
 delta=[p for p in old if old[p]!=new[p]];require(set(delta)<=ALLOWED_NON_RUST_DELTA,'normal Rust/source5 differs from new source beyond explicitly allowed C7 Python-only runner change')
 meta=owned(a.tests_report).with_name(Path(a.tests_report).stem+'.stdout.log');products=owned(a.product_report).with_name(Path(a.product_report).stem+'.stdout.log')
 ts=[x for x in guard.compiler_artifacts(meta) if x[2].get('profile',{}).get('test') is True]
 ps=[x for x in guard.compiler_artifacts(products) if x[2].get('target',{}).get('name')=='fc-bootstrap' and x[2].get('profile',{}).get('test') is False]
 require(len(ts)==21 and len({x[2]['executable'] for x in ts})==21,'actual21 distinct Cargo harnesses required; no filtering/subsetting')
 require(len(ps)==1 and Path(ps[0][2]['executable'])==NORMAL,'actual unique normal product metadata required')
 normal=guard.artifact(*ps[0],expected_subsystem=2);tests=[guard.artifact(*x,expected_subsystem=3) for x in ts]
 previous=owned(a.normal_identity_report);prior=json.loads(previous.read_text());require(prior.get('exeSha256')==normal['sha256'],'normal bytes differ from original source5 actual uploaded executable identity')
 command=['python3','-B',str(PRIVATE),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(NORMAL),'--test-metadata',str(meta),'--test-timeout-seconds','900','--native-seconds','1','--system-host','--public-desktop','--report',str(paths['report'])]
 d={'status':'RUNNING','startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':command,'freeze':identity(freeze),'workingFileCount':595,'source5NormalToCurrentDifferences':delta,'allowedDeltaScope':'C7 Python runner only; compiled Rust exactly unchanged','normalIdentityPriorReport':identity(previous),'normalIdentityPriorReportStatus':prior.get('status'),'normalIdentityPriorScope':'Executable bytes only; prior FAIL is preserved and is not treated as runtime PASS','testBuild':tp,'productBuild':pp,'privateRunner':identity(PRIVATE),'artifactGuardHelper':identity(ORIGINAL_HELPER),'normal':normal,'tests':tests,'harnessesExpected':21,'testTotal':'derive only from actual terminal harness summaries','policy':'nonzero original test binary recorded and remaining harnesses continue; final overall FAIL if any failed; timeout/transport/digest/parse/unsafe exceptions remain failfast','testThreads':2,'testTimeoutSeconds':900,'scope':'evidence-only collection, no suite PASS claim from skipped/filtered tests; actual B/storage regressions and hidden public20 may pass while native IPv6 harness remains FAIL'}
 save(paths['operation'],d,True);start=time.monotonic()
 try:
  with paths['stdout'].open('xb') as out,paths['stderr'].open('xb') as err:
   child=subprocess.Popen(command,stdout=out,stderr=err,cwd=MAIN);d['controllerPid']=child.pid;save(paths['operation'],d);code=child.wait()
 except BaseException as exc:
  d.update(status='OBSERVATION_INTERRUPTED_NOT_COMPLETION',exceptionKind=type(exc).__name__,elapsedSeconds=time.monotonic()-start);save(paths['operation'],d);raise
 d.update(status='FAIL_COMMAND',exitCode=code,elapsedSeconds=time.monotonic()-start,stdout=identity(paths['stdout']),stderr=identity(paths['stderr']),sourceUnchanged=same(f),privateRunnerUnchanged=sha(PRIVATE)==PRIVATE_SHA,artifactsUnchanged=all(sha(Path(x['path']))==x['sha256'] for x in [normal]+tests))
 if paths['report'].is_file():
  result=json.loads(paths['report'].read_text());runs=result.get('tests',[])
  d.update(resultStatus=result.get('status'),cleanup=result.get('cleanup'),harnessesObserved=len(runs),failedTestBinaries=result.get('failedTestBinaries',[]),publicDesktopNativeStatus=result.get('publicDesktopNativeStatus'),publicDesktopNativeSteps=result.get('publicDesktopNativeSteps'),result=identity(paths['report']))
  expected={Path(x['path']).name:x['sha256'] for x in tests};d['allOriginalHarnessIdentitiesMatch']=(len(runs)==21 and len({x.get('name') for x in runs})==21 and all(expected.get(x.get('name'))==x.get('binarySha256') for x in runs))
  summaries=[]
  for run in runs:
   found=[]
   for line in run.get('output',[]):
    m=re.search(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',line)
    if m:found.append([int(n) for n in m.groups()])
   summaries.append({'name':run.get('name'),'exitCode':run.get('exitCode'),'terminalSummaries':found})
  d['actualHarnessSummaries']=summaries
  d['allHarnessesTerminalUnfiltered']=(len(summaries)==21 and all(len(x['terminalSummaries'])==1 and x['terminalSummaries'][0][2:]==[0,0,0] for x in summaries))
  d['fullResultsCollected']=(d['allOriginalHarnessIdentitiesMatch'] and d['allHarnessesTerminalUnfiltered'] and result.get('publicDesktopNativeStatus')=='PASS' and result.get('publicDesktopNativeSteps')==20 and result.get('cleanup')=='PASS')
  if d['fullResultsCollected'] and d['failedTestBinaries'] and code!=0 and result.get('status')=='FAIL':d['status']='COLLECTED_FULL_RESULTS_SUITE_FAIL'
  elif d['fullResultsCollected'] and not d['failedTestBinaries'] and code==0 and result.get('status')=='PASS':d['status']='PASS_COMMAND'
 if not all(d[k] for k in ['sourceUnchanged','privateRunnerUnchanged','artifactsUnchanged']):d['status']='FAIL_IDENTITY_OR_SOURCE_CHANGED'
 save(paths['operation'],d)
 print(json.dumps({k:d.get(k) for k in ['status','exitCode','resultStatus','cleanup','harnessesObserved','fullResultsCollected','publicDesktopNativeSteps']}),flush=True)
 return 0 if d['status']=='PASS_COMMAND' else 1
if __name__=='__main__':sys.exit(main())
