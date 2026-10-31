from pathlib import Path
import base64,hashlib,json,subprocess,datetime,time
base=Path('/tmp/fc-round33-msvc-evidence');source=Path('/tmp/fc-round33-win7-worktree/scripts/windows-msvc-ci.ps1').read_bytes();check=(base/'win7-ci-pure-check.ps1').read_bytes();out=base/'win7-ci-powershell-first.json'
assert not out.exists()
sha=lambda d:hashlib.sha256(d).hexdigest()
assert sha(source)=='91e79de38f7a90e8d7976d6551b614f8fdc790590e226980971fa0f2303bc315'
wrapper="[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$s=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($s));"
stdin=("$source=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('"+base64.b64encode(source).decode()+"'));\n& {\n"+check.decode()+"\n} -TextSource $source\n").encode()
cmd=['ssh','-i','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','ConnectTimeout=8','elior@192.168.10.212','powershell.exe -NoProfile -NonInteractive -EncodedCommand '+base64.b64encode(wrapper.encode('utf-16-le')).decode()]
def raw(d):return {'bytes':len(d),'sha256':sha(d),'base64':base64.b64encode(d).decode(),'scope':'COMPLETE'}
r={'status':'RUNNING','startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'helperSha256':sha(source),'checkSha256':sha(check),'remoteScope':'Full source Parser AST; invoke only 3 extracted pure functions in memory, no top-level or hosted setup. No remote file/env/process mutation.','stdin':raw(stdin),'command':cmd}
with out.open('x') as f:
 start=time.monotonic()
 try:
  p=subprocess.run(cmd,input=stdin,capture_output=True,timeout=60);r.update(exitCode=p.returncode,stdout=raw(p.stdout),stderr=raw(p.stderr));assert p.returncode==0
  r['result']=json.loads(p.stdout.decode('utf-8-sig'));assert r['result']['status']=='PASS';r['status']='PASS'
 except subprocess.TimeoutExpired as e:r.update(status='FAIL',errorKind=type(e).__name__,stdout=raw(e.stdout or b''),stderr=raw(e.stderr or b''))
 except Exception as e:r.update(status='FAIL',errorKind=type(e).__name__)
 r['elapsedSeconds']=time.monotonic()-start;r['completedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat();f.write(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({k:r[k] for k in ['status','elapsedSeconds']},ensure_ascii=False));raise SystemExit(0 if r['status']=='PASS' else 1)
