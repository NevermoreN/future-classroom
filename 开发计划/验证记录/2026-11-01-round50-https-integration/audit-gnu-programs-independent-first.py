"""Local PE import audit via the project's audit_pe API; never executes an EXE."""
from pathlib import Path
import datetime,hashlib,importlib.util,json,re,struct,subprocess,sys,time
sys.dont_write_bytecode=True
E=Path('/tmp/fc-round50-https-integration-evidence');ROOT=Path('/workspace/共享/Eliora/未来教室计划');SOURCE=Path('/tmp/fc-round47-https-settings-worktree');OUT=E/'gnu-programs-independent-first.json';RAW=E/'gnu-programs-independent-first-raw';BIND=E/'compiled-programs-preliminary-first.json';NORMAL='b6368400a58b87d4725cbd664b5bde88a4f51927a74656e5c566010e8bbf6930'
sha=lambda b:hashlib.sha256(b).hexdigest()
records=[];inputs={};report={'status':'FAIL','recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'runtimeExecutedByThisAudit':False,'executables':[],'commandOriginals':records}
def need(x,message):
 if not x:raise ValueError(message)
def fact(path):
 p=Path(path);need(p.is_file() and not p.is_symlink(),'missing ordinary original: '+p.name);b=p.read_bytes();row={'path':str(p),'bytes':len(b),'sha256':sha(b)}
 if str(p) in inputs:need(inputs[str(p)]==row,'input bytes changed: '+p.name)
 else:inputs[str(p)]=row
 return row
def raw_fact(p,data):
 need(len(data)<=64*1024*1024,'single objdump raw exceeds64MiB')
 with p.open('xb') as f:f.write(data)
 return {'path':str(p),'bytes':len(data),'sha256':sha(data)}
def pe_header(data):
 need(data[:2]==b'MZ' and len(data)>=64,'missing DOS header');off=struct.unpack_from('<I',data,60)[0];need(off+96<=len(data) and data[off:off+4]==b'PE\x00\x00','PE header bound')
 machine=struct.unpack_from('<H',data,off+4)[0];magic=struct.unpack_from('<H',data,off+24)[0];subsystem=struct.unpack_from('<H',data,off+24+68)[0];version=struct.unpack_from('<HH',data,off+24+48)
 need(machine==0x14c and magic==0x10b and subsystem==3,'expected PE32/i386/CUI')
 return {'machine':'0x014c i386','optionalMagic':'0x010b PE32','subsystem':'3 Windows CUI','subsystemVersion':list(version)}
def command_capture(cmd,**kw):
 # The primary API normally captures text. Capture original bytes first, then
 # hand it the same decoded text without inventing raw bytes from that text.
 args=list(cmd);check=kw.pop('check',False);text=kw.pop('text',False);kw.pop('universal_newlines',None);started=time.monotonic()
 try:r=REAL_RUN(args,check=False,text=False,**kw)
 except subprocess.TimeoutExpired as error:
  seq=len(records);row={'argv':args,'actualExit':False,'exitCode':None,'timedOut':True,'timeoutSeconds':kw.get('timeout'),'elapsedSeconds':time.monotonic()-started}
  for key,data in [('stdout',error.stdout or b''),('stderr',error.stderr or b'')]:row[key]=raw_fact(RAW/f'{seq:02d}-timeout.{key}.log',data)
  records.append(row);raise
 seq=len(records);label='imports' if '-p' in args else 'version';row={'argv':args,'actualExit':True,'exitCode':r.returncode,'timedOut':False,'timeoutSeconds':kw.get('timeout'),'elapsedSeconds':time.monotonic()-started}
 for key,data in [('stdout',r.stdout),('stderr',r.stderr)]:row[key]=raw_fact(RAW/f'{seq:02d}-{label}.{key}.log',data)
 records.append(row)
 if check:r.check_returncode()
 return subprocess.CompletedProcess(args,r.returncode,r.stdout.decode('utf-8') if text else r.stdout,r.stderr.decode('utf-8') if text else r.stderr)
def main():
 need(not OUT.exists() and not RAW.exists(),'CreateNew report/raw root required');RAW.mkdir();(RAW/'.owner').write_text('fc.r50.gnu.static-pe-audit-owned-raw/v1\n')
 helper=fact(Path(__file__));api_path=ROOT/'scripts/audit-pe.py';api_fact=fact(api_path)
 spec=importlib.util.spec_from_file_location('fc_pe_primary',api_path);api=importlib.util.module_from_spec(spec);spec.loader.exec_module(api)
 try:
  compiled=json.loads(BIND.read_bytes());fact(BIND);need(compiled['status']=='PASS_COMPLETED_GNU_COMPILE_PRODUCT_STABLE_STRICT_ONLY','original compiled report status')
  prior_path=E/'runtime-certificate-independent-second.json';prior=json.loads(prior_path.read_bytes());fact(prior_path);need(prior['status'].startswith('PASS') and prior['normalSha256']==NORMAL and prior['sourceCount']==429,'prior exact runtime/source binding')
  need(compiled['sourceBefore']==prior['source'] and len(compiled['sourceBefore'])==429,'same429 compiled input manifest')
  for rel,want in compiled['sourceBefore'].items():
   got=fact(SOURCE/rel);need(got['bytes']==want['bytes'] and got['sha256']==want['sha256'],'current source differs from actual compiled429: '+rel)
  artifacts=compiled['artifacts'];need(len(artifacts)==22 and sum(x['test'] is True for x in artifacts)==21,'exact22 programs21 tests')
  meta_path=Path(compiled['metadata']['path']);meta_raw=meta_path.read_bytes();fact(meta_path);need(sha(meta_raw)==compiled['metadata']['sha256'],'original21 metadata bytes')
  metadata=[json.loads(s) for s in meta_raw.decode().splitlines() if s.strip()];need(len(metadata)==21 and len({x['executable'] for x in metadata})==21,'metadata21 unique')
  need({x['executable'] for x in metadata}=={x['path'] for x in artifacts if x['test']},'metadata exact21 executable paths')
  for row in metadata:
   a=next(x for x in artifacts if x['path']==row['executable']);need(row['reason']=='compiler-artifact' and row['profile']['test'] is True and row['target']==a['target'],'metadata target/profile exact')
  need(len({Path(x['path']).name for x in artifacts})==22,'unique22 file basenames')
  for command in compiled['completedCommands']:
   need(command['exitCode']==0 and command['actualExit'] is True and command['timedOut'] is False,'original compilation command nonzero/unfinished')
   for kind in ['stdout','stderr']:
    got=fact(command[kind]['path']);need(got['bytes']==command[kind]['bytes'] and got['sha256']==command[kind]['sha256'],'original compilation raw changed')
  for a in artifacts:
   p=Path(a['path']);got=fact(p);original=fact(a['originalPath']);need(got['bytes']==original['bytes']==a['bytes'] and got['sha256']==original['sha256']==a['sha256'],'original/copied EXE SHA')
   header=pe_header(p.read_bytes());start=len(records)
   api.subprocess.run=command_capture
   try:audit=api.audit_pe(p)
   finally:api.subprocess.run=REAL_RUN
   need(len(records)==start+2 and records[start]['argv']==['i686-w64-mingw32-objdump','-p',str(p)] and all(v['exitCode']==0 and v['actualExit'] is True and v['timedOut'] is False for v in records[start:]),'actual primary API objdump commands')
   need(audit['status']=='NOT_VALIDATED' and not audit['structural_errors'] and audit['format']=='PE32-x86' and audit['subsystem']=='Windows CUI','primary PE32/CUI check')
   # GNU uses OS msvcrt.dll. Distinguish it from additional deployable runtimes;
   # do not claim that every CRT function is statically embedded in GNU output.
   deployment=[dll for dll in audit['imports'] if re.match(r'^(?:vcruntime|msvcp|libgcc_s_|libstdc\+\+|libwinpthread|ucrtbased?(?:\.|$)|api-ms-win-crt-)',dll) or (dll.startswith('msvcr') and dll!='msvcrt.dll')]
   need(not deployment,'additional dynamic deployment runtime import')
   report['executables'].append({**a,'peHeader':header,'audit':audit,'objdumpCommands':records[start:],'directDeploymentRuntimeImports':deployment,'osMsvcrtImported':'msvcrt.dll' in audit['imports'],'staticCrtScope':'No extra deployment CRT/libgcc/libstdc++/winpthread import. GNU OS msvcrt.dll remains an explicit system import, not claimed fully statically embedded.','runtimeExecutedByThisAudit':False})
  normal=[x for x in report['executables'] if not x['test']];need(len(normal)==1 and normal[0]['sha256']==NORMAL,'normal exact b636')
  # Re-read all bound inputs after inspection. Newly created raw records are
  # checked individually and do not enter the immutable source-input manifest.
  for row in list(inputs.values()):need(fact(row['path'])==row,'original source/artifact changed during audit')
  for row in records:
   for kind in ['stdout','stderr']:need(fact(row[kind]['path'])==row[kind],'raw objdump changed')
  report.update(status='PASS_STATIC_PE32_GNU_IDENTITY_NOT_OS_COMPATIBILITY',normalExecutable={k:normal[0][k] for k in ['path','bytes','sha256']},counts={'executables':22,'tests':21,'actualObjdumpImports':sum('-p' in x['argv'] for x in records),'originalInputs':429,'osMsvcrtImports':sum(x['osMsvcrtImported'] for x in report['executables']),'additionalDeploymentRuntimeImports':0,'programsWithModernOrUpdateRisk':sum(bool(x['audit']['risks']) for x in report['executables'])},sourceBefore=compiled['sourceBefore'],metadata={'path':str(meta_path),'bytes':len(meta_raw),'sha256':sha(meta_raw)},primaryApi=api_fact,helper=helper)
 except BaseException as error:
  report['error']={'type':type(error).__name__,'message':str(error)[:500]}
 finally:
  api.subprocess.run=REAL_RUN
 report.update(inputs=list(inputs.values()),limitations=['No executable run, Cargo, SSH, Windows, network, OS configuration or cleanup by this audit.','Actual audit_pe status NOT_VALIDATED is retained for every executable; structural PASS does not establish Win7/Win10/Seewo support.','Modern GNU directly imports OS msvcrt.dll. No additional deployment runtime imports is distinct from claiming every CRT function is statically embedded.','All actual post-baseline API-set/MODERN_API/WINRT/update-sensitive risks retained; dynamic/delayed loading and exact OS export availability are not established.','New certificate-query APIs need future formal MSVC and actual target-machine evidence; existing Win11 actual1481 runtime evidence is separate.','R50 full Host all-target matrix and original900s timeout/rerun are independent; this static audit does not promote them.'],helperUnchanged=sha(Path(__file__).read_bytes())==helper['sha256'])
 with OUT.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
 print(json.dumps({'status':report['status'],'report':str(OUT),'sha256':sha(OUT.read_bytes()),'normalSha256':report.get('normalExecutable',{}).get('sha256'),'counts':report.get('counts'),'error':report.get('error')},ensure_ascii=False));return 0 if report['status'].startswith('PASS') else 1
REAL_RUN=subprocess.run
if __name__=='__main__':raise SystemExit(main())
