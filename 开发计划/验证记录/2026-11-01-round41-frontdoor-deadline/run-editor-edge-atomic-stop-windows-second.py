from pathlib import Path
import subprocess,base64,json,hashlib,datetime,sys,time
E=Path('/tmp/fc-round41-frontdoor-deadline-evidence');PS=E/'editor-edge-atomic-stop-windows-fixture-second.ps1';OUT=E/'editor-edge-atomic-stop-windows-second.json';assert not OUT.exists()
sha=lambda b:hashlib.sha256(b).hexdigest();source=PS.read_bytes()
boot="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$s=[Console]::In.ReadToEnd();$t=$null;$e=$null;[Management.Automation.Language.Parser]::ParseInput($s,[ref]$t,[ref]$e)|Out-Null;if($e.Count){throw (($e|ForEach-Object{$_.Message})-join '; ')};[Console]::WriteLine('SYNTHETIC_STOP_PUBLISH_AST0');& ([scriptblock]::Create($s))"
cmd=['ssh','-o','BatchMode=yes','-o','ConnectTimeout=8','future-classroom-win11','powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand '+base64.b64encode(boot.encode('utf-16le')).decode()]
r={'status':'RUNNING','scope':'Actual Win11 PowerShell synthetic file-only fixture; no Edge/TLS/browser/control read/real audio/Cargo/Git. Exactly owned TEMP root and4owned child PowerShell processes.','source':{'path':str(PS),'bytes':len(source),'sha256':sha(source),'base64':base64.b64encode(source).decode()},'command':cmd,'stdinBoot':boot,'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()};started=time.monotonic()
try:
 actual=subprocess.run(cmd,input=source,capture_output=True,timeout=85)
 r.update(exitCode=actual.returncode,elapsedSeconds=time.monotonic()-started)
 for label,data in [('stdout',actual.stdout),('stderr',actual.stderr)]:
  p=E/('editor-edge-atomic-stop-windows-second.'+label+'.log');assert not p.exists();p.write_bytes(data);r[label]={'path':str(p),'bytes':len(data),'sha256':sha(data),'base64':base64.b64encode(data).decode()}
 assert actual.returncode==0,actual.stderr.decode(errors='replace')[-1200:]
 lines=actual.stdout.decode('utf-8-sig').strip().splitlines();assert len(lines)==2 and lines[0]=='SYNTHETIC_STOP_PUBLISH_AST0'
 result=json.loads(lines[1]);assert result['status']=='PASS' and result['rootRemoved'] is True and result['started']==result['actualExited']==result['disposed']==4
 r.update(status='PASS_ACTUAL_WINDOWS_SYNTHETIC_FILES',result=result,astErrors=0,completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
except BaseException as error:
 r.update(status='FAIL',errorType=type(error).__name__,error=str(error),elapsedSeconds=time.monotonic()-started);raise
finally:OUT.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'path':str(OUT),'sha256':sha(OUT.read_bytes()),'status':r['status'],'elapsedSeconds':r.get('elapsedSeconds'),'children':r.get('result',{}).get('actualExited')}))
