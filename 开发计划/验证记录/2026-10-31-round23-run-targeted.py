import pathlib,runpy,json,uuid,hashlib,sys
m=runpy.run_path('scripts/windows-probe.py');ps=m['ps_literal'];r=m['Remote']('elior@192.168.10.212',pathlib.Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212'))
exe=pathlib.Path(sys.argv[1]);filter_name=sys.argv[2];report_path=pathlib.Path(sys.argv[3]);token=uuid.uuid4().hex;sha=hashlib.sha256(exe.read_bytes()).hexdigest();report={'scope':'Targeted Windows noninteractive test; source and exe frozen for this component only, not whole workspace acceptance.','filter':filter_name,'exeSha256':sha,'status':'NOT_RUN','cleanup':'NOT_RUN'};root=None
try:
 root=r.powershell("$root=Join-Path $env:TEMP "+ps('FutureClassroom-RustProbe-round23-'+token)+";New-Item -ItemType Directory -Path $root|Out-Null;Set-Content -LiteralPath (Join-Path $root '.owner') -Value "+ps(token)+";$root")
 r.upload(exe,root)
 script="$root="+ps(root)+";$env:TEMP=Join-Path $root 'temp';$env:TMP=$env:TEMP;New-Item -ItemType Directory -Path $env:TEMP|Out-Null;$exe=Join-Path $root "+ps(exe.name)+";$sha=(Get-FileHash -Algorithm SHA256 -LiteralPath $exe).Hash.ToLowerInvariant();if($sha-ne "+ps(sha)+"){throw 'digest mismatch'};$ErrorActionPreference='Continue';$out=@(& $exe "+ps(filter_name)+" --test-threads=2 2>&1|ForEach-Object{[string]$_}|Tee-Object -FilePath (Join-Path $root 'test.log'));$code=$LASTEXITCODE;@{exitCode=$code;sha256=$sha;output=$out}|ConvertTo-Json -Depth 3 -Compress"
 report['result']=json.loads(r.powershell(script,timeout=600));report['status']='PASS' if report['result']['exitCode']==0 else 'FAIL'
except Exception as e:
 report['status']='FAIL';report['errorType']=type(e).__name__
 if root:
  try: report['partialOutput']=r.powershell("$p=Join-Path "+ps(root)+" 'test.log';if(Test-Path -LiteralPath $p){Get-Content -LiteralPath $p -Tail 100|ForEach-Object{[string]$_}}",timeout=15)
  except Exception:pass
finally:
 if root:
  try:
   result=r.powershell("$root="+ps(root)+";if((Get-Content -Raw -LiteralPath (Join-Path $root '.owner')).Trim()-ne "+ps(token)+"){throw 'ownership mismatch'};Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\\',[StringComparison]::OrdinalIgnoreCase)}|ForEach-Object{Stop-Process -Id $_.ProcessId -Force};Remove-Item -Recurse -Force -LiteralPath $root;@{rootAbsent=!(Test-Path -LiteralPath $root);sshd=(Get-Service sshd).Status.ToString()}|ConvertTo-Json -Compress",timeout=30)
   report['cleanup']=json.loads(result)
  except Exception as e:report['cleanup']={'errorType':type(e).__name__}
 report_path.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
 print(json.dumps({k:v for k,v in report.items() if k not in ['result','partialOutput']},ensure_ascii=False))
 if 'result' in report:
  print('\n'.join(x for x in report['result']['output'] if x.startswith('test result:')))
assert report['status']=='PASS' and report['cleanup'].get('rootAbsent') and report['cleanup'].get('sshd')=='Running'
