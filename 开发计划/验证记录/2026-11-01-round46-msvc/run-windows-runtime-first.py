from pathlib import Path
import hashlib,json,subprocess,time,sys
root=Path('/workspace/共享/Eliora/未来教室计划')
ev=Path('/tmp/fc-round46-ci-evidence')
script=root/'scripts/windows-probe.py'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for tag in ('modern','win7'):
 audit=json.loads((ev/(tag+'-original-independent-first.json')).read_text())
 assert audit['status']=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN'
 exe=Path(audit['normalExecutable']['localPath']);meta=Path(audit['testMetadata']['path'])
 assert sha(exe)==audit['normalExecutable']['producer']['sha256']
 assert sha(meta)==audit['testMetadata']['sha256']
 out=ev/(tag+'-msvc-runtime-first.json');assert not out.exists()
 cmd=['python3','-B',str(script),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--test-metadata',str(meta),'--system-host','--report',str(out)]
 record=ev/(tag+'-msvc-runtime-first.command.json');assert not record.exists()
 r={'status':'RUNNING','args':cmd,'scriptSha256':sha(script),'sourceSha':audit['sourceSha'],'inputNormalSha256':sha(exe),'metadataSha256':sha(meta),'scope':'Actual serial Win11 WOW64 execution only; Win7 candidate does not establish Win7 OS compatibility. Original per-test60s,threads2 and hidden/read-only system/host checks retained.'}
 record.write_text(json.dumps(r,indent=2)+'\n')
 print('WINDOWS_RUNTIME_STARTED '+tag,flush=True);start=time.monotonic()
 with (ev/(tag+'-msvc-runtime-first.stdout.log')).open('xb') as a,(ev/(tag+'-msvc-runtime-first.stderr.log')).open('xb') as b:
  child=subprocess.Popen(cmd,cwd=root,stdout=a,stderr=b);child.wait()
 r.update(status='PASS_COMMAND' if child.returncode==0 else 'FAIL_COMMAND',exitCode=child.returncode,actualExit=True,pid=child.pid,elapsedSeconds=time.monotonic()-start,scriptUnchanged=sha(script)==r['scriptSha256'])
 if out.exists():
  result=json.loads(out.read_text());r.update(resultStatus=result.get('status'),cleanup=result.get('cleanup'))
 for key in ('stdout','stderr'):
  p=ev/(tag+'-msvc-runtime-first.'+key+'.log');r[key]={'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p)}
 record.write_text(json.dumps(r,indent=2)+'\n');print('WINDOWS_RUNTIME_DONE '+tag+' exit='+str(child.returncode),flush=True)
 if child.returncode or r.get('cleanup')!='PASS' or r.get('resultStatus')!='PASS' or not r['scriptUnchanged']:sys.exit(1)
