"""Audit all22 final GNU programs with the unchanged project audit_pe API."""
from pathlib import Path
import hashlib,importlib.util,json,re,struct,subprocess,sys,time
sys.dont_write_bytecode=True
W=Path('/tmp/fc-round54-win7-loader-worktree');E=Path('/tmp/fc-round54-win7-loader-evidence');T=Path('/tmp/fc-round54-win7-loader-target');OUT=E/'pe-audit-first.json';RAW=E/'pe-audit-first-raw';sha=lambda b:hashlib.sha256(b).hexdigest();REAL_RUN=subprocess.run
report={'status':'FAIL','scope':'GNU local staticPE only, noEXE execution or formalWin7/MSVC/realOS compatibility claim','programs':[],'commands':[]}
def need(x,msg):
 if not x:raise ValueError(msg)
def rawfact(path,data):
 need(len(data)<=64*1024*1024,'objdump output64MiB bound')
 with path.open('xb') as f:f.write(data)
 return {'path':str(path),'bytes':len(data),'sha256':sha(data)}
def capture(args,**kw):
 check=kw.pop('check',False);text=kw.pop('text',False);t=time.monotonic();r=REAL_RUN(args,check=False,text=False,**kw);i=len(report['commands']);row={'argv':args,'actualExit':True,'exitCode':r.returncode,'elapsedSeconds':time.monotonic()-t}
 for kind,data in [('stdout',r.stdout),('stderr',r.stderr)]:row[kind]=rawfact(RAW/f'{i:02d}.{kind}.log',data)
 report['commands'].append(row)
 if check:r.check_returncode()
 return subprocess.CompletedProcess(args,r.returncode,r.stdout.decode() if text else r.stdout,r.stderr.decode() if text else r.stderr)
def artifacts(path,test):
 result=[]
 for line in path.read_text().splitlines():
  if not line.strip():continue
  r=json.loads(line)
  if r.get('reason')=='compiler-artifact' and r.get('executable') and r['profile']['test'] is test and r['executable'].endswith('.exe'):result.append(r)
 return result
need(not OUT.exists() and not RAW.exists(),'CreateNew outputs required');RAW.mkdir();(RAW/'.owner').write_text('future-classroom-owned-r54-pe-raw/v1\n')
api_path=W/'scripts/audit-pe.py';api_raw=api_path.read_bytes();spec=importlib.util.spec_from_file_location('primary_pe',api_path);api=importlib.util.module_from_spec(spec);spec.loader.exec_module(api)
try:
 build=json.loads((E/'checks-first.json').read_bytes());need(build['status']=='PASS_COMMANDS_ONLY_PE_AUDIT_PENDING_WINDOWS_NOT_RUN' and build['sourceBefore']==build['sourceAfter'] and not build['changedInputs'],'completed originalbuild/source required');testcmd=next(x for x in build['commands'] if x['name']=='windows-compile');productcmd=next(x for x in build['commands'] if x['name']=='windows-product')
 tests=artifacts(Path(testcmd['stdout']['path']),True);normal=artifacts(Path(productcmd['stdout']['path']),False);need(len(tests)==21 and len(normal)==1 and normal[0]['target']['name']=='fc-bootstrap','exact21test+1normal compiler records');allitems=tests+normal;need(len({x['executable'] for x in allitems})==22,'distinct22programs');metadata=E/'windows-test-metadata-first.jsonl'
 with metadata.open('x') as f:
  for item in tests:f.write(json.dumps(item,ensure_ascii=False)+'\n')
 compiler=[]
 for item in allitems:
  p=Path(item['executable']);need(p.is_relative_to(T/'stable') and p.is_file() and not p.is_symlink(),'exactownedstableexecutable');data=p.read_bytes();before={'bytes':len(data),'sha256':sha(data)};off=struct.unpack_from('<I',data,60)[0];need(data[:2]==b'MZ' and data[off:off+4]==b'PE\0\0' and struct.unpack_from('<H',data,off+4)[0]==0x14c and struct.unpack_from('<H',data,off+24)[0]==0x10b and struct.unpack_from('<H',data,off+24+68)[0]==3,'PE32/i386/CUI required');start=len(report['commands']);api.subprocess.run=capture
  try:audit=api.audit_pe(p)
  finally:api.subprocess.run=REAL_RUN
  need(not audit['structural_errors'] and audit['status']=='NOT_VALIDATED','primary structure/status');need('api-ms-win-core-winrt-error-l1-1-0.dll' not in audit['imports'] and 'combase.dll' not in audit['imports'],'Win7unavailable staticDLL reintroduced');extra=[d for d in audit['imports'] if d.startswith(('vcruntime','msvcp','libgcc_s_','libstdc++','libwinpthread','api-ms-win-crt-')) or d in {'ucrtbase.dll','ucrtbased.dll'}];need(not extra,'extra deployable runtime');need(p.stat().st_size==before['bytes'] and sha(p.read_bytes())==before['sha256'],'EXEchanged duringaudit');row={'path':str(p),**before,'test':item['profile']['test'],'target':item['target'],'pe32i386CUI':True,'primaryAudit':audit,'objdump':report['commands'][start:],'forbiddenWinrtDlls':[],'extraDeploymentRuntimeImports':extra,'osMsvcrtImported':'msvcrt.dll' in audit['imports']};report['programs'].append(row);compiler.append({k:row[k] for k in ['path','bytes','sha256','test','target']})
 for rel,want in build['sourceAfter'].items():
  data=(W/rel).read_bytes();need(len(data)==want['bytes'] and sha(data)==want['sha256'],'compiledsourcechanged')
 oldpath=E/'original-win7-import-risk-before.json';oldraw=oldpath.read_bytes();old=json.loads(oldraw);oldimp=old['imports'];need(oldimp.get('api-ms-win-core-winrt-error-l1-1-0.dll')==['RoOriginateErrorW'] and oldimp.get('combase.dll')==['CoTaskMemFree'],'originalactualriskmetadatachanged')
 report.update(status='PASS_ALL22_GNU_PE32_NO_WINRT_ERROR_OR_COMBASE_STATIC_IMPORT_WINDOWS_NOT_RUN',normalExecutable=compiler[-1],metadata={'path':str(metadata),'bytes':metadata.stat().st_size,'sha256':sha(metadata.read_bytes())},sourceCount=len(build['sourceAfter']),sourceManifest=build['sourceAfter'],forbiddenDlls=['api-ms-win-core-winrt-error-l1-1-0.dll','combase.dll'],primaryApi={'path':str(api_path),'bytes':len(api_raw),'sha256':sha(api_raw)},originalFormalWin7Before={'report':str(oldpath),'reportSha256':sha(oldraw),'scope':'OriginalunmodifiedformalWin7MSVCartifact risk metadata; afterGNU is a different toolchain candidate, not retestedformalMSVC.','imports':{d:oldimp[d] for d in ['api-ms-win-core-winrt-error-l1-1-0.dll','combase.dll']}},counts={'tests':21,'programs':22,'actualObjdumpImports':22,'actualVersionCommands':22,'forbiddenImports':0,'additionalDeploymentRuntimeImports':0,'systemMsvcrt':sum(p['osMsvcrtImported'] for p in report['programs']),'programsWithOtherRisk':sum(bool(p['primaryAudit']['risks']) for p in report['programs'])})
 (E/'compiled-programs-first.json').write_text(json.dumps({'status':'PASS_GNU_STATIC_ARTIFACTS_ONLY_WINDOWS_NOT_RUN','sourceManifest':build['sourceAfter'],'metadata':report['metadata'],'artifacts':compiler},ensure_ascii=False,indent=2)+'\n')
except BaseException as error:report['error']={'type':type(error).__name__,'message':str(error)[:500]}
finally:api.subprocess.run=REAL_RUN
report.update(limitations=['NoEXEexecution, Windows, SSH or CI action by this audit.','AllprimaryauditNOT_VALIDATEDstatuses retained. Removing2knownWin7incompatible staticDLLs is not comprehensiveWin7API/load/runtime certification.','Systemmsvcrt.dll is explicitly retained forGNU; no extra deploymentCRT imports does not mean allCRTfunctions staticallyembedded.','OriginalWin7MSVCbefore imports retained asactualfailure risk, source/pinnedSDK/newformalMSVCPE+realWin7machine requireseparatevalidation.','Otherprimarymodern/update-sensitive risk lists retained verbatim.'])
with OUT.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'status':report['status'],'report':str(OUT),'sha256':sha(OUT.read_bytes()),'normalExecutable':report.get('normalExecutable'),'counts':report.get('counts'),'error':report.get('error')},ensure_ascii=False));raise SystemExit(0 if report['status'].startswith('PASS') else 1)
