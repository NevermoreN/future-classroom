"""Read-only audit of the one affected GNU libtest and normal binary; never run EXEs."""
from pathlib import Path
import hashlib,importlib.util,json,struct,subprocess,sys,time
sys.dont_write_bytecode=True
W=Path('/tmp/fc-round56-transactional-autorun-worktree');E=Path('/tmp/fc-round56-transactional-autorun-evidence');T=Path('/tmp/fc-round56-transactional-autorun-target');OUT=E/'programs-static-first.json';RAW=E/'pe-raw-first'
sha=lambda b:hashlib.sha256(b).hexdigest();real_run=subprocess.run
report={'status':'FAIL','scope':'Two GNU static PE files only; compilation is not runtime or formal MSVC/Win7 validation.','programs':[],'commands':[]}
def need(condition,msg):
 if not condition:raise ValueError(msg)
def fact(p):
 data=p.read_bytes();return {'path':str(p),'bytes':len(data),'sha256':sha(data)}
def artifacts(path,test):
 rows=[]
 for line in path.read_text().splitlines():
  if not line.strip():continue
  row=json.loads(line)
  if row.get('reason')=='compiler-artifact' and row.get('executable') and row['profile']['test'] is test:rows.append(row)
 return rows
def capture(args,**kwargs):
 check=kwargs.pop('check',False);text=kwargs.pop('text',False);t=time.monotonic();r=real_run(args,text=False,check=False,**kwargs);i=len(report['commands']);row={'argv':args,'exitCode':r.returncode,'actualExit':True,'elapsedSeconds':time.monotonic()-t}
 for kind,data in [('stdout',r.stdout),('stderr',r.stderr)]:
  need(len(data)<=16*1024*1024,'original objdump16MiB bound');p=RAW/f'{i:02d}.{kind}.log';p.open('xb').write(data);row[kind]=fact(p)
 report['commands'].append(row)
 if check:r.check_returncode()
 return subprocess.CompletedProcess(args,r.returncode,r.stdout.decode() if text else r.stdout,r.stderr.decode() if text else r.stderr)
need(not OUT.exists() and not RAW.exists(),'CreateNew outputs');RAW.mkdir();(RAW/'.owner').write_text('future-classroom-owned-r56-pe-raw/v1\n')
api_path=W/'scripts/audit-pe.py';spec=importlib.util.spec_from_file_location('primary_pe',api_path);api=importlib.util.module_from_spec(spec);spec.loader.exec_module(api)
try:
 build=json.loads((E/'checks-second.json').read_text());need(build['status']=='PASS_COMPILE_AND_STRICT_LINT_ONLY_PE_AND_RUNTIME_NOT_RUN' and not build['changedInputs'] and build['sourceBefore']==build['sourceAfter'],'completed frozen compile required')
 tests=artifacts(Path(next(c for c in build['commands'] if c['name']=='windows-fc-windows-compile')['stdout']['path']),True);normal=artifacts(Path(next(c for c in build['commands'] if c['name']=='windows-product')['stdout']['path']),False)
 need(len(tests)==1 and tests[0]['target']['name']=='fc_windows' and len(normal)==1 and normal[0]['target']['name']=='fc-bootstrap','one exact test and one product record')
 m=E/'windows-test-metadata-first.jsonl';m.open('x').write(json.dumps(tests[0],ensure_ascii=False)+'\n')
 for record in tests+normal:
  p=Path(record['executable']);need(p.is_relative_to(T/'stable') and not p.is_symlink(),'exact owned stable file');before=fact(p);data=p.read_bytes();off=struct.unpack_from('<I',data,60)[0]
  need(data[:2]==b'MZ' and data[off:off+4]==b'PE\0\0' and struct.unpack_from('<H',data,off+4)[0]==0x14c and struct.unpack_from('<H',data,off+24)[0]==0x10b and struct.unpack_from('<H',data,off+24+68)[0]==3,'PE32/i386/CUI')
  start=len(report['commands']);api.subprocess.run=capture
  try:audit=api.audit_pe(p)
  finally:api.subprocess.run=real_run
  need(not audit['structural_errors'] and audit['status']=='NOT_VALIDATED','primary structure/status')
  need(not ({'combase.dll','api-ms-win-core-winrt-error-l1-1-0.dll'} & set(audit['imports'])),'prior forbidden DLL reintroduced')
  extra=[dll for dll in audit['imports'] if dll.startswith(('vcruntime','msvcp','api-ms-win-crt-','libgcc_s_','libstdc++','libwinpthread')) or dll in {'ucrtbase.dll','ucrtbased.dll'}];need(not extra,'extra deployment CRT')
  ktm=set(audit['imports'].get('ktmw32.dll',[]));adv=set(audit['imports'].get('advapi32.dll',[]));required={'CreateTransaction','CommitTransaction','RollbackTransaction','GetTransactionInformation'};need(required<=ktm,'actual required KTM imports');need({'RegOpenKeyTransactedW','RegCreateKeyTransactedW'}<=adv,'actual transacted registry imports')
  need(fact(p)==before,'PE changed during audit');report['programs'].append({**before,'test':record['profile']['test'],'target':record['target'],'primaryAudit':audit,'objdumpCommands':report['commands'][start:],'KtmW32Imports':sorted(ktm),'Advapi32TransactedImports':sorted(x for x in adv if 'Transacted' in x),'systemMsvcrtRetained':'msvcrt.dll' in audit['imports'],'extraDeploymentRuntimeImports':extra})
 for relative,want in build['sourceAfter'].items():
  now=fact(W/relative);need(all(now[k]==want[k] for k in ['bytes','sha256']),'compiled source changed')
 report.update(status='PASS_TWO_GNU_PE32_KTM_TXR_STATIC_IMPORTS_RUNTIME_NOT_RUN',normalExecutable={k:report['programs'][1][k] for k in ['path','bytes','sha256']},affectedTestExecutable={k:report['programs'][0][k] for k in ['path','bytes','sha256']},sourceCount=len(build['sourceAfter']),sourceManifest=build['sourceAfter'],metadata=fact(m),primaryApi=fact(api_path),buildReport=fact(E/'checks-second.json'))
except BaseException as error:report['error']={'type':type(error).__name__,'message':str(error)[:500]}
finally:api.subprocess.run=real_run
report['limitations']=['No actual Windows, registry, EXE, SSH or CI action.','Both primary audits remain NOT_VALIDATED; GNU modern API risks and systemmsvcrt retained verbatim.','New KtmW32/Advapi32 static imports are documented Vista+ but require actual exactOS/MSVC and real scratch-interleaving evidence.','MSI identity/full-lifetime maintenance owner, SQLite/OS atomicity and universal TxR isolation are not implemented or established.']
OUT.open('x').write(json.dumps(report,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'status':report['status'],'report':str(OUT),'sha256':sha(OUT.read_bytes()),'normal':report.get('normalExecutable'),'test':report.get('affectedTestExecutable'),'error':report.get('error')},ensure_ascii=False));raise SystemExit(0 if report['status'].startswith('PASS') else 1)
