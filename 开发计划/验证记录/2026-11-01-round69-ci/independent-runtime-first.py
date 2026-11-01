#!/usr/bin/env python3
"""R69 finite modern runtime acceptance; Win7 failed CI explicitly retained.
Root invokes after all original controllers are terminal. Missing/RUNNING inputs
mean WAIT and no report. No network, SSH, executable, Cargo or Git mutation.
"""
from pathlib import Path
import argparse,ast,datetime,hashlib,json,re,runpy,subprocess,sys,zipfile
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parent
REPO=Path('/workspace/共享/Eliora/未来教室计划')
SOURCE='8bc1c9812df765865a54988a23ff3973d04884fc'
TEMPLATE=REPO/'开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py'
TEMPLATE_SHA='0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5'
AUDIT_SHA='c136efb0ecb5dc1dbffadadae31b933c43ac3d298c308921dd7c8e409fc75527'
FINAL_SHA='a0c79bdcc98ace31e160c31c5eeba42852babcc14d17ec5a54e5e05cd8044886'
SUCCESS_STATUS='PASS_R69_MODERN_WIN11_REGRESSION_WITH_WIN7_CI_FAILURE_RETAINED'
class Waiting(Exception):pass
def need(ok,message):
 if not ok:raise AssertionError(message)
def digest(data):return hashlib.sha256(data).hexdigest()
def read(path):
 p=Path(path)
 if not p.is_file():raise Waiting('MISSING_'+p.name)
 need(not p.is_symlink(),'nonregular original')
 try:v=json.loads(p.read_text(encoding='utf-8-sig'))
 except json.JSONDecodeError:raise Waiting('INCOMPLETE_'+p.name) from None
 if isinstance(v,dict) and v.get('status') in ('RUNNING','STARTED','queued','in_progress','pending','waiting','requested'):raise Waiting('RUNNING_'+p.name)
 return v
def fact(path):
 p=Path(path)
 if not p.is_file():raise Waiting('MISSING_'+p.name)
 need(not p.is_symlink(),'nonregular original')
 h=hashlib.sha256();count=0
 with p.open('rb') as stream:
  while chunk:=stream.read(1024*1024):h.update(chunk);count+=len(chunk)
 return dict(path=str(p),bytes=count,sha256=h.hexdigest())
def exact(row):
 p=Path(row['path']);p=p if p.is_absolute() else ROOT/p
 v=fact(p);need(v['bytes']==row['bytes'] and v['sha256']==row['sha256'],'original raw identity '+p.name);return v
def api(path):
 v=read(path);need(v.get('isError') is not True,'API error wrapper');v=v.get('structuredContent',v)
 if isinstance(v,dict) and isinstance(v.get('content'),str):v=json.loads(v['content'])
 return v
def operation(name,budget=None):
 v=read(ROOT/name);need(v.get('status') in ('PASS','PASS_COMMAND') and type(v.get('exitCode')) is int and v['exitCode']==0,'actual terminal operation '+name)
 if 'actualControllerReturned' in v:need(v['actualControllerReturned'] is True,'controller actually returned')
 if budget is not None:need(v['originalBudgetSeconds']==budget and v['elapsedSeconds']<=budget,'original operation budget')
 return dict(record=fact(ROOT/name),raw={key:exact(v[key]) for key in ('stdout','stderr')})
def download(tag,run_id,artifact_id,expected_size,expected_sha):
 v=read(ROOT/(tag+'-download-original-first.json'))
 need(v['status']=='PASS' and v['runId']==run_id and v['artifactId']==artifact_id
  and v['bytes']==v['expectedBytes']==expected_size and v['sha256']==v['expectedSha256']==expected_sha
  and v['overallBudgetSeconds']==1200 and v['socketTimeoutSeconds']==25,'terminal original download')
 op=operation(tag+'-download-operation-first.json',1200)
 need(read(ROOT/(tag+'-download-operation-first.json'))['actualControllerReturned'] is True,'original downloader parent returned')
 z=fact(ROOT/(tag+'-original.zip'));need(z['bytes']==expected_size and z['sha256']==expected_sha,'exact whole ZIP')
 return dict(receipt=fact(ROOT/(tag+'-download-original-first.json')),controller=op,zip=z)
def load_template():
 need(digest(TEMPLATE.read_bytes())==TEMPLATE_SHA,'immutable R67 parser template')
 ns=runpy.run_path(str(TEMPLATE));g=ns['official_and_artifact'].__globals__
 original_api=g['api']
 def mapped_api(path):
  p=Path(path)
  if p.name=='modern-run-final-first.json':p=ROOT/'modern-run-third.json'
  return original_api(p)
 g.update(ROOT=ROOT,SOURCE=SOURCE,BASELINES={'modern':('first','Modern',41290550848,124800715119,13077068479,'.github/workflows/windows-msvc.yml')},api=mapped_api)
 return ns

def win7_failed(modern_audit,auditor):
 run=api(ROOT/'win7-run-third.json');jobs=api(ROOT/'win7-jobs-first.json')['jobs'];arts=api(ROOT/'win7-artifacts-first.json')['artifacts']
 need(run['id']==41290587954 and run['head_sha']==SOURCE and run['status']=='completed' and run['conclusion']=='failure'
  and run['path']=='.github/workflows/windows-win7-msvc.yml' and run['event']=='workflow_dispatch','Win7 exact original failure run')
 selected=[j for j in jobs if j['id']==124800824775];need(len(selected)==1,'Win7 exact job');job=selected[0]
 need(job['run_id']==run['id'] and job['status']=='completed' and job['conclusion']=='failure','Win7 job failure retained')
 failed_steps=[s for s in job['steps'] if s['conclusion']!='success'];need(len(failed_steps)==1 and failed_steps[0]['name']=='Compile Win7 MSVC candidate with rebuilt std/test; never execute','single original Win7 failed CI step')
 for label in ["Clean only this ephemeral run's build and tool roots",'Upload original logs, metadata and compiled test executables']:
  rows=[s for s in job['steps'] if s['name']==label];need(len(rows)==1 and rows[0]['status']=='completed' and rows[0]['conclusion']=='success','Win7 cleanup/upload actually passed')
 selected=[a for a in arts if a['id']==13077796437];need(len(selected)==1,'Win7 exact artifact');art=selected[0]
 sha='5c12b693a84151a18ba63131dfa2991638bc5c9bc647d925ec0595e1ec4696aa'
 need(art['expired'] is False and art['workflow_run']['id']==run['id'] and art['workflow_run']['head_sha']==SOURCE
  and art['size_in_bytes']==44654009 and art['digest']=='sha256:'+sha
  and art['name']==f'future-classroom-win7-msvc-i686-{SOURCE}-{run["id"]}-{run["run_attempt"]}','Win7 official whole artifact identity')
 dl=download('win7',run['id'],art['id'],44654009,sha)
 receipt=read(ROOT/'win7-failed-original-raw-first.json');need(receipt['status']=='FAILED_CI_NONBINARY_ORIGINALS_RETAINED_NOT_RUNTIME_APPROVED'
  and receipt['sourceSha']==SOURCE and receipt['runId']==run['id'] and receipt['zipSha256']==sha,'failed raw receipt never promoted')
 raw=ROOT/'win7-failed-original-raw-first';retained={row['path']:row for row in receipt['files']}
 need(len(retained)==len(receipt['files'])==210,'complete210 nonbinary originals')
 with zipfile.ZipFile(dl['zip']['path']) as z:
  infos=z.infolist();names=auditor['zip_inventory'](infos)
  ledger=auditor['decode_json'](z.read('artifact-sha256.json'));by_name={v['path']:v for v in ledger}
  need(len(by_name)==len(ledger) and set(names)==set(by_name)|{'artifact-sha256.json'},'failed original complete producer ledger')
  exe_names={n for n in names if n.lower().endswith('.exe')};need(len(exe_names)==22 and set(retained)==set(names)-exe_names,'failed22 binaries excluded from210 raw members')
  for info in infos:
   data=z.read(info);need(len(data)==info.file_size,'failed ZIP member length')
   if info.filename in by_name:need(len(data)==by_name[info.filename]['bytes'] and digest(data)==by_name[info.filename]['sha256'],'failed original member producer SHA')
   if info.filename in retained:
    row=retained[info.filename];p=raw/info.filename
    need(p.read_bytes()==data and len(data)==row['bytes'] and digest(data)==row['sha256'],'retained nonbinary exact ZIP member')
  producers=auditor['decode_json'](z.read('executables.json'))
  need(len(producers)==22 and {p['relative_path'] for p in producers}==exe_names
   and sum(p['role']=='tests' for p in producers)==21 and sum(p['role']=='product' for p in producers)==1
   and all(p['executed'] is False for p in producers),'Win7 original compiled-only producer set')
  for p in producers:need(by_name[p['relative_path']]['bytes']==p['bytes'] and by_name[p['relative_path']]['sha256']==p['sha256'],'Win7 failed producer byte binding')
 need({p.relative_to(raw).as_posix() for p in raw.rglob('*') if p.is_file()}==set(retained),'failed raw exact membership')
 result=read(raw/'result.json');need(result['source_sha']==result['dispatch_source_sha']==SOURCE and result['baseline']=='Win7'
  and result['status']=='FAILED_OR_INCOMPLETE' and result['target']=='i686-win7-windows-msvc'
  and result['toolchain']=='nightly-2026-10-29-x86_64-pc-windows-msvc','original failed CI result source')
 need(all(result[k]=='NOT_RUN' for k in ('windows7','windows10','windows11','seewo_device')),'failed CI runtime flags remain NOT_RUN')
 commands={v['name']:v for v in result['commands']};need(len(commands)==len(result['commands']),'unique failed original commands')
 failures=[v['name'] for v in result['commands'] if not(v['passed'] is True and v['exit_code']==0 and v['parent_exited'] is True and not v['timed_out_or_pipe_stalled'])]
 need(failures==['clippy'],'only original command failure clippy')
 summary=[]
 for name,budget,code in [('workspace-test-compile',2400,0),('product-compile',1200,0),('clippy',1500,101)]:
  v=commands[name];need(v['parent_exited'] is True and not v['timed_out_or_pipe_stalled'] and v['exit_code']==code
   and v['duration_seconds']<=budget and v['passed'] is (code==0),'fixed original command state/budget')
  summary.append(dict(name=name,exitCode=code,elapsedSeconds=v['duration_seconds'],budgetSeconds=budget))
 stderr=(raw/'logs/clippy.stderr.log').read_text(encoding='utf-8-sig');plain=re.sub(r'\x1b\[[0-9;]*m','',stderr)
 errors=[s for s in plain.splitlines() if re.match(r'^error(?:\[|:)',s)]
 need(errors==['error: items after a test module','error: could not compile `fc-windows` (lib test) due to 1 previous error']
  and 'items_after_test_module' in plain and 'ConfiguredTlsError' in plain and 'tls_frontdoor/mod.rs' in plain.replace('\\','/'),'exact firstparty item-order diagnostic')
 # Independently evaluate original source partition metadata with the unchanged
 # auditor predicate, then tie actual Win7 checkout SHA rows to already audited modern rows.
 git=subprocess.run(['git','ls-tree','-r','-z','-l','--full-tree',SOURCE],cwd=REPO,capture_output=True,check=True).stdout
 expected=auditor['parse_git_tree'](git)
 oid=subprocess.run(['git','rev-parse',SOURCE+'^{tree}'],cwd=REPO,capture_output=True,check=True).stdout.decode().strip()
 triples=[[read(raw/(stem+suffix+'.json')) for stem in ('input-sha256','input-git-tree','input-domains')] for suffix in ('','-after')]
 counts,hashed=auditor['validate_domains'](SOURCE,oid,expected,*triples)
 need(counts['actualShaSubset']==595 and counts['historicalMetadataOnly']==14496 and counts['fullGitMetadata']==15091,'actual595 and historical14496 domains')
 modern_rows={v['path']:{k:v[k] for k in ('path','bytes','sha256')} for v in modern_audit['sourceMatches']}
 need(hashed==modern_rows,'Win7 exact original checkout SHA subset equals independently source-bound modern original')
 cleanup=read(raw/'cleanup.json');need(cleanup['status']=='OWNED_BUILD_ROOTS_REMOVED' and len(cleanup['roots'])==4 and all(v['removed'] is True for v in cleanup['roots']),'failed CI actual original four-root cleanup')
 need(not list(ROOT.glob('win7-msvc-*.json')) and not (ROOT/'win7-original-independent-first.json').exists(),'no failed-candidate static PASS or runtime report fabricated')
 return dict(status='ACTUAL_CI_FAIL_RETAINED_NOT_RUNTIME_APPROVED',runId=run['id'],jobId=job['id'],artifactId=art['id'],download=dl,
  rawReceipt=fact(ROOT/'win7-failed-original-raw-first.json'),retainedNonbinaryCount=210,compiledNotExecutedCount=22,
  commands=summary,firstpartyDiagnostic='clippy::items_after_test_module in fc-windows tls_frontdoor/mod.rs',sourceDomains=counts,
  originalResult=fact(raw/'result.json'),diagnosticOriginal=fact(raw/'logs/clippy.stderr.log'),cleanup=fact(raw/'cleanup.json'),
  runtime='NOT_RUN_NOT_APPROVED',qualification='R71 reorder work belongs to a separate later source; it does not change source8bc or this failed run. No aggregate two-CI PASS.')

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--report',type=Path,default=ROOT/'independent-runtime-first.json');args=p.parse_args()
 need(args.report.parent.resolve()==ROOT and not args.report.exists(),'fresh exclusive report required')
 report=dict(schemaVersion=1,status='FAIL_INDEPENDENT_EVIDENCE_CHECK',sourceSha=SOURCE,recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
 try:
  need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r69-ci-evidence/v1\n','exact R69 owned root')
  required=['modern-original-independent-first.json','modern-msvc-runtime-first.json','modern-msvc-runtime-first.command.json',
   'modern-public-visual-operation-first.json','modern-msvc-public-native-first.json','modern-msvc-visual-native-first.json',
   'modern-static-operation-first.json','modern-download-original-first.json','modern-download-operation-first.json',
   'win7-run-third.json','win7-jobs-first.json','win7-artifacts-first.json','win7-download-original-first.json','win7-download-operation-first.json',
   'win7-failed-original-raw-first.json','windows-readonly-final-first.json']
  values={name:read(ROOT/name) for name in required}
  template=load_template();g=template['official_and_artifact'].__globals__
  auditor_path=ROOT/'verify-ci-artifact-first.py';need(digest(auditor_path.read_bytes())==AUDIT_SHA,'unchanged auditor');auditor=runpy.run_path(str(auditor_path))
  audit=values['modern-original-independent-first.json'];bound=template['official_and_artifact']('modern',audit)
  need(audit['helper']['sha256']==AUDIT_SHA and audit['counts']['actualShaSubset']==595 and audit['counts']['historicalMetadataOnly']==14496,'modern source domain/auditor scope')
  dl=download('modern',41290550848,13077068479,43928970,'b63edb03cd83ca23ea01736796d38cf48cf60c76042823d2eb65b894652c7dff')
  static_op=operation('modern-static-operation-first.json',900)
  probe=subprocess.run(['git','show',SOURCE+':scripts/windows-probe.py'],cwd=REPO,capture_output=True,check=True).stdout;probe_hash=digest(probe)
  source_ast=ast.parse(probe);native_steps={}
  for node in source_ast.body:
   if isinstance(node,ast.Assign):
    for target in node.targets:
     if isinstance(target,ast.Name) and target.id in ('PUBLIC_DESKTOP_STEPS','SUBJECT_VISUAL_STEPS'):native_steps[target.id]=ast.literal_eval(node.value)
  need(len(native_steps['PUBLIC_DESKTOP_STEPS'])==20 and len(native_steps['SUBJECT_VISUAL_STEPS'])==22,'source-pinned20/22 steps')
  runtime=values['modern-msvc-runtime-first.json'];controller=values['modern-msvc-runtime-first.command.json'];template['check_platform'](runtime)
  normal=audit['normalExecutable']['localPath'];need(controller['sourceSha']==SOURCE and controller['scriptUnchanged'] is True
   and controller['inputNormalSha256']==runtime['exeSha256']==bound['normalSha256'] and controller['metadataSha256']==bound['metadataSha256'],'exact runtime source/normal/metadata')
  controller_raw=template['command'](controller,normal,probe_hash)
  need(controller['args'][controller['args'].index('--test-timeout-seconds')+1]=='900'
   and controller['args'][controller['args'].index('--test-metadata')+1]==audit['testMetadata']['path'],'original runtime metadata900 budget')
  uploaded={v['name']:v['sha256'] for v in runtime['uploadedArtifacts']};need(len(uploaded)==len(runtime['uploadedArtifacts'])==22
   and uploaded=={n:p['sha256'] for n,p in bound['producers'].items()},'actual uploaded all22 producer SHA')
  tests=runtime['tests'];need(len(tests)==21 and {v['name'] for v in tests}=={n for n,p in bound['producers'].items() if p['role']=='tests'},'actual complete21 harness inventory')
  harnesses=[]
  for item in tests:
   need(item['binarySha256']==uploaded[item['name']],'actual test uploaded hash');parsed=template['harness'](item)
   parsed['caseNames']=[g['CASE'].fullmatch(line)[1] for line in item['output'] if g['CASE'].fullmatch(line)];harnesses.append(parsed)
  need(runtime['system']['exitCode']==0 and runtime['native']['hostExitCode']==0,'actual system and hidden host exit0')
  operations=values['modern-public-visual-operation-first.json'];need(operations['status']=='PASS_SELECTED2_ORIGINAL_MSVC_PUBLIC_VISUAL_FLOWS'
   and operations['scriptSha256']==probe_hash and len(operations['operations'])==2,'actual dedicated flow controller')
  flows={}
  for kind,key in [('public','PUBLIC_DESKTOP_STEPS'),('visual','SUBJECT_VISUAL_STEPS')]:
   label='modern-msvc-'+kind+'-native-first';rows=[v for v in operations['operations'] if v['label']==label]
   need(len(rows)==1 and rows[0]['status']=='PASS' and rows[0]['exeSha256']==bound['normalSha256'],'actual flow operation source')
   data=values[label+'.json'];need(data['exeSha256']==bound['normalSha256'] and data['tests']==[]
    and data['uploadedArtifacts']==[dict(name=template['basename'](normal),sha256=bound['normalSha256'])],'actual flow sole normal upload')
   raw=template['command'](rows[0],normal,probe_hash);flows[kind]=dict(template['flow'](data,kind,native_steps[key]),controllerRaw=raw)
  win7=win7_failed(audit,auditor)
  final=values['windows-readonly-final-first.json'];need(digest((ROOT/'windows-readonly-final-first.json').read_bytes())==FINAL_SHA,'exact final readonly audit')
  need(final['status']=='PASS' and final.get('completedAtUtc') and final['ssh']['exitCode']==0 and final['authentication']=='PASS'
   and final['publicAuthorizationUnchanged'] is True and final['projectRunUnchanged'] is True and final['integrityStatus']=='PASS'
   and final['ownedProcesses']==final['ownedTasks']==final['scratchDirectories']==[] and final['sshd']=='Running'
   and final['persistentKeyMode']=='0o600' and final['persistentKeyMetadata']['contentsRead'] is False,'final cleanup/persistent invariants')
  baseline=REPO/final['baseline']['path'];need(digest(baseline.read_bytes())==final['baseline']['sha256'],'readonly exact prior baseline')
  for name,stream in final['ssh'].items():
   if isinstance(stream,dict) and 'base64' in stream:
    import base64
    raw=base64.b64decode(stream['base64'],validate=True);need(len(raw)==stream['bytes'] and digest(raw)==stream['sha256'],'readonly exact captured stream')
  report.update(status=SUCCESS_STATUS,
   modern=dict(status='PASS_ACTUAL_MODERN_CI_AND_FINITE_WIN11_RUNTIME_ONLY',runId=41290550848,jobId=124800715119,artifactId=13077068479,
    staticAudit=fact(ROOT/'modern-original-independent-first.json'),staticController=static_op,download=dl,executables=bound['executables'],
    normalSha256=bound['normalSha256'],metadataSha256=bound['metadataSha256'],sourceDomains=audit['counts'],actualHost=runtime['platform'],runtimeArchitecture='x86 PE32 on x64 Windows11 WOW64',
    harnessCount=len(harnesses),harnesses=harnesses,totals={k:sum(v[k] for v in harnesses) for k in ('passed','failed','ignored','measured','filteredOut')},
    controllerRaw=controller_raw,nativeFlows=flows,cleanup='PASS'),win7=win7,windowsFinalAudit=fact(ROOT/'windows-readonly-final-first.json'),
   parserTemplate=dict(identity=fact(TEMPLATE),reused='original PE/producer/Cargo association, strict per-case/unique-summary harness and source20/22 flow parsers'),
   auditor=fact(auditor_path),sourceProbeSha256=probe_hash,helper=fact(Path(__file__)),inputs=[fact(ROOT/name) for name in required],
   scope='R69 source8bc: modern official CI and actual finite Win11 regression pass; original Win7-target CI FAIL retained, candidate not approved or run. No aggregate both-CI PASS, Win7/10 OS, campus, R70/R71 new-source or final product certification.',
   captureQualification='Per-harness/native output is original normalized merged lines, not independent raw stdout/stderr; original controller raw files are separately byte-verified. Historical14496 archive files use exact Git metadata only, not checkout content SHA.')
 except (Waiting,g.get('Waiting',Waiting) if 'g' in locals() else Waiting) as error:
  print(json.dumps(dict(status='WAIT',reasonCode=str(error),reportCreated=False)));return 3
 except Exception as error:
  report['error']=dict(kind=type(error).__name__,message=str(error))
 data=(json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode()
 with args.report.open('xb') as output:output.write(data)
 print(json.dumps(dict(status=report['status'],report=str(args.report),sha256=digest(data))))
 return 0 if report['status']==SUCCESS_STATUS else 1
if __name__=='__main__':raise SystemExit(main())
