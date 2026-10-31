import argparse, hashlib, json, runpy, uuid
from pathlib import Path
repo = Path('/workspace/共享/Eliora/未来教室计划')
m = runpy.run_path(str(repo/'scripts/windows-probe.py'))
Remote, q = m['Remote'], m['ps_literal']
p = argparse.ArgumentParser(); p.add_argument('--metadata', required=True); p.add_argument('--filter', required=True); p.add_argument('--report', required=True); a=p.parse_args()
items=[json.loads(l) for l in Path(a.metadata).read_text().splitlines()]
executables=[Path(x['executable']) for x in items if x.get('reason')=='compiler-artifact' and x.get('profile',{}).get('test') and x.get('executable') and x['target']['name']=='fc_bootstrap' and 'lib' in x['target']['kind']]
assert len(executables)==1, executables
exe=executables[0]; digest=hashlib.sha256(exe.read_bytes()).hexdigest()
r=Remote('elior@192.168.10.212',Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212'))
token=uuid.uuid4().hex
root=r.powershell('$p=Join-Path $env:TEMP '+q('FutureClassroom-RustProbe-'+token)+';New-Item -ItemType Directory -Path $p|Out-Null;Set-Content -LiteralPath (Join-Path $p '+q('.future-classroom-probe-root')+') -Value '+q(token)+';$p')
report={'scope':'actual Win11 real SQLite/native runtime desired regression; synthetic isolated deployment, no output device', 'filter':a.filter,'root':root,'binarySha256':digest,'cleanup':'NOT_RUN'}
try:
    r.upload(exe,root)
    script='$p=Join-Path '+q(root)+' '+q(exe.name)+';$hash=(Get-FileHash -Algorithm SHA256 -LiteralPath $p).Hash.ToLowerInvariant();if($hash -ne '+q(digest)+'){throw "binary hash mismatch"};$env:TEMP=Join-Path '+q(root)+' "temp";New-Item -ItemType Directory -Path $env:TEMP|Out-Null;$env:TMP=$env:TEMP;$out=@(& $p '+q(a.filter)+' --test-threads=2 2>&1|ForEach-Object{"$_"});$code=$LASTEXITCODE;@{exitCode=$code;output=$out;binarySha256=$hash}|ConvertTo-Json -Compress'
    report['actual']=json.loads(r.powershell(script,timeout=180).lstrip('\ufeff'))
finally:
    script='$root='+q(root)+';if((Get-Content -LiteralPath (Join-Path $root ".future-classroom-probe-root") -Raw).Trim() -ne '+q(token)+'){throw "ownership marker mismatch"};@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+"\\",[StringComparison]::OrdinalIgnoreCase)})|ForEach-Object{Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue};Remove-Item -LiteralPath $root -Recurse -Force;if(Test-Path -LiteralPath $root){throw "owned root remains"};"PASS"'
    report['cleanup']=r.powershell(script)
    Path(a.report).write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'exitCode':report.get('actual',{}).get('exitCode'),'cleanup':report['cleanup'],'filter':a.filter}))
