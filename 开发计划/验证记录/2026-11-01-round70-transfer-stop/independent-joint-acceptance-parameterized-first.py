#!/usr/bin/env python3
"""Finite R70 final-source evidence audit, prepared for root's later invocation.
Missing/RUNNING inputs yield WAIT/no report. Only local original bytes and pure
prior parsers are read; no executable, Windows/SSH/Cargo/network or source edit.
Recovery and reinitialization are separate original CLI operations by contract.
"""
from pathlib import Path
import argparse,ast,base64,datetime,hashlib,json,re,runpy,struct,sys
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parent.parent;E=ROOT/'evidence';W=ROOT/'worktree'
MAIN=Path('/workspace/共享/Eliora/未来教室计划')
PARSER=MAIN/'开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py'
PARSER_SHA='0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5'
ORDINARY_SHA='50195c3d7f7b26ad39055c91522e8a46f772fe4e1add35fc55527b9d48f9fd9e'
NORMAL_SHA='80fe27c788360b511e75efbaf451eadb7bd71a46657dd036db7b8a7af2622246'
class Waiting(Exception):pass
def need(ok,code):
 if not ok:raise AssertionError(code)
def sha(data):return hashlib.sha256(data).hexdigest()
def fact(path):
 p=Path(path)
 if not p.is_file():raise Waiting('MISSING_'+p.name)
 need(not p.is_symlink(),'nonregular original');h=hashlib.sha256();size=0
 with p.open('rb') as f:
  while b:=f.read(1024*1024):h.update(b);size+=len(b)
 return dict(path=str(p),bytes=size,sha256=h.hexdigest())
def read(path):
 p=Path(path)
 if not p.is_file():raise Waiting('MISSING_'+p.name)
 need(not p.is_symlink(),'nonregular original')
 try:v=json.loads(p.read_text(encoding='utf-8-sig'))
 except json.JSONDecodeError:raise Waiting('INCOMPLETE_'+p.name) from None
 if isinstance(v,dict) and v.get('status') in ('RUNNING','STARTED','OBSERVATION_INTERRUPTED_NOT_COMPLETION'):raise Waiting('NONTERMINAL_'+p.name)
 return v
def exact(row,path=None):
 got=fact(path or row['path']);need((got['bytes'],got['sha256'])==(row['bytes'],row['sha256']),'exact original identity');return got
def raw(row):
 b=base64.b64decode(row['base64'],validate=True);need(len(b)==row['bytes'] and sha(b)==row['sha256'],'exact encoded raw capture');return b
def source_current(f):
 for row in f['workingFiles']:
  got=fact(W/row['path']);need(got['bytes']==row['bytes'] and got['sha256']==row['sha256'],'frozen595 source changed')
def command_original(path,freeze_sha,normal,report_path,flag):
 op=read(path);need(op.get('status') in ('PASS','PASS_COMMAND') and type(op.get('exitCode')) is int and op['exitCode']==0 and op.get('sourceUnchanged') is True,'extra actual terminal controller')
 need(op.get('sourceFreezeSha256')==freeze_sha and op.get('workingFileCount')==595 and op.get('originalBudgetSeconds')==900 and op.get('elapsedSeconds',901)<=900,'extra actual source/budget')
 cmd=op['command'];need(cmd.count(flag)==1 and cmd.count('--exe')==cmd.count('--report')==1 and Path(cmd[cmd.index('--exe')+1])==Path(normal['path']) and Path(cmd[cmd.index('--report')+1]).resolve()==report_path.resolve(),'extra actual flags/exe/report')
 need(not('--recovery' in cmd and '--reinitialization' in cmd),'original mutually exclusive CLI contract')
 need(str(MAIN/'scripts/windows-probe.py') in cmd or str(W/'scripts/windows-probe.py') in cmd,'original windows probe controller')
 streams={}
 for key in ('stdout','stderr'):
  p=path.with_suffix('.'+key+'.log');streams[key]=exact(dict(path=str(p),bytes=op[key+'Bytes'],sha256=op[key+'Sha256']))
 return op,dict(report=fact(path),streams=streams)
def products(op):
 need(len(op['tests'])==op['expectedHarnessCount']==21,'actual21 Cargo test producer rows')
 all_rows=[op['normal']]+op['tests'];need(len({Path(v['path']).name for v in all_rows})==22,'unique22 actual PE basenames')
 observed={}
 for kind in ('clippy','tests','product'):
  provenance=op['buildProvenance'][kind];exact(provenance['report']);report=read(provenance['report']['path'])
  need(report['status']=='PASS' and report['exitCode']==0 and report['sourceUnchanged'] is True and report['sourceFreezeSha256']==op['sourceFreezeSha256'] and report['workingFileCount']==595 and report['originalBudgetSeconds']==900 and report['elapsedSeconds']<=900,'actual frozen builds/900s')
  for stream in ('stdout','stderr'):
   exact(provenance[stream]);need(provenance[stream]['bytes']==report[stream+'Bytes'] and provenance[stream]['sha256']==report[stream+'Sha256'],'actual build raw stream identity')
 for kind,expected_rows in [('tests',op['tests']),('product',[op['normal']])]:
  rows={v['path']:v for v in expected_rows};log=Path(op['buildProvenance'][kind]['stdout']['path']);seen=set()
  for number,line in enumerate(log.read_bytes().splitlines(keepends=True),1):
   try:value=json.loads(line)
   except (ValueError,UnicodeError):continue
   if value.get('reason')!='compiler-artifact' or not value.get('executable'):continue
   selected=value.get('profile',{}).get('test') is True if kind=='tests' else value.get('target',{}).get('name')=='fc-bootstrap' and value.get('profile',{}).get('test') is False
   if not selected:continue
   path=value['executable'];need(path in rows and path not in seen,'complete unique actual Cargo executable set');seen.add(path);row=rows[path]
   need(row['cargoMetadataLine']==number and row['cargoMetadataLineSha256']==sha(line) and row['cargoTarget']==value['target']['name'] and row['cargoPackageId']==value.get('package_id') and row['cargoProfileTest']==value['profile']['test'] and path in value.get('filenames',[]),'exact original Cargo line/target/profile/package')
  need(seen==set(rows),'all actual Cargo producer rows mapped')
 for row in all_rows:
  p=Path(row['path']);need(p.resolve().is_relative_to((ROOT/'target').resolve()),'owned exact target artifact');exact(row)
  with p.open('rb') as stream:
   dos=stream.read(64);need(len(dos)==64 and dos[:2]==b'MZ','bounded DOS header');offset=struct.unpack_from('<I',dos,60)[0];stream.seek(offset);head=stream.read(96)
  subsystem=2 if row is op['normal'] else 3
  need(len(head)==96 and head[:4]==b'PE\0\0' and struct.unpack_from('<H',head,4)[0]==0x14c and struct.unpack_from('<H',head,24)[0]==0x10b and struct.unpack_from('<H',head,92)[0]==row['peSubsystem']==subsystem,'actual22 I386 PE32 GUI/CUI role headers')
  observed[p.name]=row['sha256']
 return observed

def main():
 p=argparse.ArgumentParser(description=__doc__)
 for name in ('freeze','full-report','full-operation','ordinary-audit','recovery-report','recovery-operation','reinitialization-report','reinitialization-operation','readonly-audit','readonly-operation'):p.add_argument('--'+name,type=Path,required=True)
 p.add_argument('--freeze-sha256',required=True);p.add_argument('--readonly-sha256',required=True)
 p.add_argument('--output',type=Path,default=E/'independent-joint-acceptance-first.json');a=p.parse_args()
 need(a.output.parent.resolve()==E and not a.output.exists(),'fresh exclusive output required')
 result=dict(schemaVersion=1,status='FAIL_FINITE_R70_FINAL_EVIDENCE_CHECK',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
 try:
  # Preflight ALL final inputs before any PASS decision or report creation.
  values={name:read(getattr(a,name.replace('-','_'))) for name in ('freeze','full-report','full-operation','ordinary-audit','recovery-report','recovery-operation','reinitialization-report','reinitialization-operation','readonly-audit','readonly-operation')}
  need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r70-ordinary-transfer-stop/v1\n','exact R70 owner')
  freeze=values['freeze'];need(fact(a.freeze)['sha256']==a.freeze_sha256 and len(freeze['files'])==4 and len(freeze['workingFiles'])==595,'supplied final4 source/595 freeze')
  source_current(freeze)
  need('ORDINARY_MATERIAL_PREPARATION' not in (W/'crates/fc-bootstrap/src/web_application/material_files.rs').read_text(),'temporary preparation diagnostic withdrawn')
  need(fact(PARSER)['sha256']==PARSER_SHA,'original strict parser identity');old=runpy.run_path(str(PARSER));g=old['harness'].__globals__
  op=values['full-operation'];full=values['full-report'];need(op['status']=='PASS_COMMAND' and type(op['exitCode']) is int and op['exitCode']==0 and op['resultStatus']==op['cleanup']=='PASS','original actual full controller/result terminal')
  need(op['sourceFreezeSha256']==a.freeze_sha256 and op['workingFileCount']==595 and all(op[k] is True for k in ('sourceUnchanged','scriptUnchanged','metadataUnchanged','artifactsUnchanged')),'full exact source/runner/artifact ownership')
  need(op['normal']['sha256']==NORMAL_SHA,'same final normal80fe identity');uploaded=products(op)
  exact(op['script']);exact(op['metadata']);need(op['script']['sha256']==fact(W/'scripts/windows-probe.py')['sha256'],'frozen original Windows runner')
  need(op['result']==fact(a.full_report),'actual controller exact result identity')
  full_raw={key:exact(op[key]) for key in ('stdout','stderr')}
  cmd=op['command'];need(cmd.count('--test-timeout-seconds')==1 and cmd[cmd.index('--test-timeout-seconds')+1]=='900' and cmd[cmd.index('--native-seconds')+1]=='1' and '--system-host' in cmd and '--public-desktop' in cmd and Path(cmd[cmd.index('--exe')+1])==Path(op['normal']['path']) and Path(cmd[cmd.index('--test-metadata')+1])==Path(op['metadata']['path']) and Path(cmd[cmd.index('--report')+1]).resolve()==a.full_report.resolve(),'original900/native1/system/public/full input contract')
  old['check_platform'](full);need(full['system']['exitCode']==0 and full['native']['hostExitCode']==0,'actual system/hidden host exits')
  need(full['exeSha256']==NORMAL_SHA and len(full['uploadedArtifacts'])==22 and {v['name']:v['sha256'] for v in full['uploadedArtifacts']}==uploaded,'all22 actual uploaded bytes')
  need(len(full['tests'])==21 and {v['name'] for v in full['tests']}=={Path(v['path']).name for v in op['tests']},'complete21 harness set')
  cases=[]
  for test in full['tests']:
   need(test['binarySha256']==uploaded[test['name']],'actual per-harness PE identity');checked=old['harness'](test)
   checked['caseNames']=[g['CASE'].fullmatch(line)[1] for line in test['output'] if g['CASE'].fullmatch(line)];cases.append(checked)
  native_cases=next(row['caseNames'] for row in cases if row['name'].startswith('fc_windows-'))
  material_source=(W/'crates/fc-windows/src/materials/files.rs').read_text()
  material_tests=re.findall(r'#\[test\]\s*fn\s+([a-zA-Z0-9_]+)\s*\(',material_source)
  need(material_tests and all(any(name.endswith('::'+test) for name in native_cases) for test in material_tests),'actual material native regression cases include current frozen long-path/nooverwrite assertions')
  constants={}
  for n in ast.parse((W/'scripts/windows-probe.py').read_bytes()).body:
   if isinstance(n,ast.Assign):
    for t in n.targets:
     if isinstance(t,ast.Name) and t.id=='PUBLIC_DESKTOP_STEPS':constants[t.id]=ast.literal_eval(n.value)
  public=old['flow'](full,'public',constants['PUBLIC_DESKTOP_STEPS'])
  # The already actually executed ordinary31 independent report is immutable;
  # rebind every retained original identity, without rerunning product/validators.
  ordinary=values['ordinary-audit'];need(fact(a.ordinary_audit)['sha256']==ORDINARY_SHA and ordinary['status']=='PASS_FINAL_FROZEN_ORDINARY31_FINITE_WIN11_ONLY' and ordinary['freeze']['sha256']==a.freeze_sha256 and ordinary['normal']['sha256']==NORMAL_SHA and ordinary['cleanup']=='PASS' and ordinary['sourceUnchanged'] is True,'actual source8 ordinary31 independent admission')
  for row in [ordinary['freeze'],ordinary['actualReport'],ordinary['runner'],ordinary['normal']]+ordinary['rawOriginals']:exact(row)
  for key in ('productProvenance','operationProvenance'):
   exact(ordinary[key]['report'])
   for row in ordinary[key]['streams'].values():exact(row)
  need(ordinary['transcript']['steps']==31 and ordinary['teacher']['principalId']=='2' and ordinary['teacher']['nativeApprovals']==2 and ordinary['teacher']['oldCookie']=='401' and ordinary['journey']['reusedSlots']==2 and ordinary['materialReuse']['commandId']=='69001' and ordinary['materialReuse']['exactRetry'] is True,'actual31 teacher/twoReady/receipt/old cookie contracts')
  # Respect the existing CLI mutual exclusion: two separately terminal flows.
  extras={};extra_ops=[]
  for kind,report_path,operation_path,flag in [('recovery',a.recovery_report,a.recovery_operation,'--recovery'),('reinitialization',a.reinitialization_report,a.reinitialization_operation,'--reinitialization')]:
   report=values[kind+'-report'];old['check_platform'](report);need(report['exeSha256']==NORMAL_SHA and report['tests']==[] and report['uploadedArtifacts']==[dict(name=Path(op['normal']['path']).name,sha256=NORMAL_SHA)],'extra same sole normal source')
   operation,identity=command_original(operation_path,a.freeze_sha256,op['normal'],report_path,flag);extra_ops.append(operation)
   need(report['native'][kind+'ExitCode']==0 and report[kind+'NativeStatus']=='PASS','extra actual native flow terminal')
   lines=report['native'][kind+'Output'];need(isinstance(lines,list) and all(isinstance(v,str) for v in lines),'actual normalized extra output')
   if kind=='recovery':
    source=(W/'crates/fc-bootstrap/src/application/backup_probe.rs').read_text();expected=re.findall(r'mark\(\s*(?:&mut\s+)?steps,\s*"([^"]+)"',source)
    steps=[re.fullmatch(r'RECOVERY_NATIVE_STEP index=([0-9]+) name=([^\r\n]+)',s) for s in lines if s.startswith('RECOVERY_NATIVE_STEP')]
    need(len(expected)==len(steps)==23 and all(steps) and [(int(m[1]),m[2]) for m in steps]==list(enumerate(expected)),'complete source-pinned23 recovery steps')
    footer=[s for s in lines if s.startswith('backup_export_save_preflight_cancel_lock_stale_explicit_cold_restore_native_worker=PASS ')];need(len(footer)==1 and all(s in footer[0] for s in ['recovery_native_steps=23','reopen=fresh_App_AND_independent_executable_production_Open','old_lease=fresh_App_only','picker=ISOLATED_CAPABILITY_ONLY','visible_dialog=NOT_TESTED','foreground=UNTOUCHED','real_run=READ_ONLY','isolated_cleanup=PASS']),'original complete recovery footer')
    elapsed=re.search(r' elapsed_ms=([0-9]+)$',footer[0]);need(elapsed is not None and int(elapsed[1])<=250000,'original recovery250s bound')
    extras[kind]=dict(report=fact(report_path),controller=identity,steps=expected,footer=footer[0],scope='Original parent validates actual independent child reopen internally; child captured output is not separately emitted by this probe. No visible picker/power-loss claim.')
   else:
    children=[re.fullmatch(r'REINITIALIZATION_CHILD_PASS stage=(cancel|commit|reopen) nonce=([0-9a-f]{32})',s) for s in lines if s.startswith('REINITIALIZATION_CHILD_PASS')]
    need(all(children) and [v[1] for v in children]==['cancel','commit','reopen'] and len({v[2] for v in children})==1,'three exact actual original child stages/nonce')
    footer=[s for s in lines if s.startswith('reinitialization_native=PASS child_processes=3 ')];need(len(footer)==1 and all(s in footer[0] for s in ['cancel_exact_preservation=PASS','default_and_partial_confirmation_denied=PASS','full_reset=PASS','new_dpapi=PASS','new_installation=PASS','native_setup=PASS','old_account_absent=PASS','old_backup_direct_restore_denied=PASS','external_saved_copy=UNCHANGED','foreground=UNTOUCHED','real_run=UNTOUCHED','isolated_cleanup=PASS']),'complete actual reinitialization footer')
    extras[kind]=dict(report=fact(report_path),controller=identity,childStages=[v[1] for v in children],footer=footer[0])
  audit_op=values['readonly-operation'];need(audit_op.get('status')=='PASS' and type(audit_op.get('exitCode')) is int and audit_op['exitCode']==0 and audit_op.get('sourceUnchanged') is True and audit_op.get('sourceFreezeSha256')==a.freeze_sha256 and audit_op.get('workingFileCount')==595 and audit_op.get('originalBudgetSeconds')==900 and audit_op.get('elapsedSeconds',901)<=900,'actual final readonly controller terminal/source/budget')
  audit_cmd=audit_op['command'];need(audit_cmd.count('--report')==1 and Path(audit_cmd[audit_cmd.index('--report')+1]).resolve()==a.readonly_audit.resolve() and str(E/'audit-windows-readonly-first.py') in audit_cmd,'actual original readonly script/report')
  audit_raw={key:exact(dict(path=str(a.readonly_operation.with_suffix('.'+key+'.log')),bytes=audit_op[key+'Bytes'],sha256=audit_op[key+'Sha256'])) for key in ('stdout','stderr')}
  final=values['readonly-audit'];need(fact(a.readonly_audit)['sha256']==a.readonly_sha256 and final['status']=='PASS' and final.get('completedAtUtc') and final['ssh']['exitCode']==0 and final['authentication']=='PASS' and final['integrityStatus']=='PASS','exact final terminal readonly audit')
  need(final['ownedProcesses']==final['ownedTasks']==final['scratchDirectories']==[] and final['publicAuthorizationUnchanged'] is True and final['projectRunUnchanged'] is True and final['sshd']=='Running' and final['persistentKeyMode']=='0o600' and final['persistentKeyMetadata']['contentsRead'] is False,'clean fixtures and persistent SSH/Run boundaries')
  need(fact(MAIN/final['baseline']['path'])['sha256']==final['baseline']['sha256'],'exact preceding readonly baseline')
  for entry in final['ssh'].values():
   if isinstance(entry,dict) and 'base64' in entry:raw(entry)
  observed=datetime.datetime.fromisoformat(final['observedAtUtc'])
  for operation in [op]+extra_ops:
   ended=datetime.datetime.fromisoformat(operation['startedUtc'])+datetime.timedelta(seconds=operation['elapsedSeconds'])
   need(observed>=ended,'readonly observation after actual terminal Windows controllers')
  source_current(freeze)
  historical=[]
  for path in sorted(E.glob('windows-ordinary-https-*.json')):
   value=read(path)
   if value.get('status')=='FAIL':historical.append(dict(identity=fact(path),status='FAIL',cleanup=value.get('cleanup'),scope='Original failure preserved; no promotion by final scoped acceptance.'))
  result.update(status='PASS_R70_SOURCE8_FINITE_GNU_WIN11_REGRESSION_ORDINARY31_AND_RECOVERY_ONLY',freeze=fact(a.freeze),modifiedSources=freeze['files'],workingFileCount=595,
   normal=op['normal'],fullRegression=dict(report=fact(a.full_report),controller=fact(a.full_operation),controllerRaw=full_raw,harnessCount=len(cases),harnesses=cases,totals={k:sum(v[k] for v in cases) for k in ('passed','failed','ignored','measured','filteredOut')},actualMaterialNativeCases=material_tests,publicDesktop=public),
   ordinary31=dict(independentAudit=fact(a.ordinary_audit),actualReport=ordinary['actualReport'],generations=ordinary['twoActualGenerations'],stops=ordinary['stops'],journey=ordinary['journey'],teacher=ordinary['teacher']),extras=extras,
   readonly=dict(audit=fact(a.readonly_audit),controller=fact(a.readonly_operation),controllerRaw=audit_raw),historicalFailures=historical,parser=fact(PARSER),helper=fact(Path(__file__)),sourceUnchanged=True,
   scope='Supplied final source8/normal80fe and actual finite GNU Win11 build26300/WOW64 regression, ordinary31, independent recovery/reinitialization and terminal cleanup only. No Win7/10 OS, new MSVC/campus/visible Seewo/PPT/touch/audio, installer broker/hooks/upgrade/uninstall, hardware power-loss or72h acceptance.',
   capture='Harness/native lines are original normalized merged output; controller/raw original streams independently hashed. Counts derive from each actual case and unique summary; no fixed1542 total.',
   statusQualification='All historical source/publication/diagnostic failures remain original. Temporary stage diagnostics withdrawn; new native206 evidence narrows the real process failure but successful independent C# path controls are retained as differing host conditions, not relabeled or claimed as a sole cause.')
 except Waiting as error:
  print(json.dumps(dict(status='WAIT',reasonCode=str(error),reportCreated=False)));return 3
 except Exception as error:
  result['error']=dict(kind=type(error).__name__,failureCode=str(error) if isinstance(error,AssertionError) else 'INPUT_OR_ORIGINAL_PARSER_FAILURE')
 data=(json.dumps(result,ensure_ascii=False,indent=2)+'\n').encode()
 with a.output.open('xb') as stream:stream.write(data)
 print(json.dumps(dict(status=result['status'],report=str(a.output),sha256=sha(data))))
 return 0 if result['status'].startswith('PASS_R70_SOURCE8_') else 1
if __name__=='__main__':raise SystemExit(main())
