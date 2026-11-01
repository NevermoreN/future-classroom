from pathlib import Path
import subprocess,base64,json,hashlib,time,sys,uuid

P=Path(__file__).parent
MODE=sys.argv[1]
EXE_SHA='42082ab87cf9825bd4d370b31c3834b2e18910e64fc763970bf6704de251b515'
ORIGINAL_SHA='b9851d1ce96297375d62a0c8d959123471c4a1436d7cf120973ec8e1c035d7f5'
state=P/'root-transport-first.json'
if not state.exists():
    assert MODE=='preflight'
    source=(P/'source-final/run-windows-probe.ps1').read_bytes()
    assert hashlib.sha256(source).hexdigest()==ORIGINAL_SHA
    source=source.replace(b"$report.errorKind=$_.Exception.GetType().Name;$report.errorMessage=",b"$report.errorKind=$_.Exception.GetType().Name;$report.errorDetail=$_.Exception.Message;$report.errorLine=$_.InvocationInfo.ScriptLineNumber;$report.errorMessage=")
    f=P/'root-diagnostic-runner-first.ps1';f.write_bytes(source)
    state.write_text(json.dumps({'directoryNonce':uuid.uuid4().hex,'sourceSha256':hashlib.sha256(source).hexdigest(),'originalSourceSha256':ORIGINAL_SHA,'delta':'Only actual exception message/line added; native, guards, process observation, deadline, parser and cleanup unchanged.'},indent=2)+'\n')
s=json.loads(state.read_text());nonce=s['directoryNonce'];owner='future-classroom-owned-r60-registry-value-intent/v1:'+nonce
prefix="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);"
scope="$root=Join-Path ([IO.Path]::GetTempPath()) 'FutureClassroomRegistryValueIntent-"+nonce+"';"
def encoded(data):return "[Convert]::FromBase64String('"+base64.b64encode(data).decode()+"')"
if MODE=='preflight':
    source=(P/'root-diagnostic-runner-first.ps1').read_bytes();assert hashlib.sha256(source).hexdigest()==s['sourceSha256']
    ps=prefix+scope+"$s=[Text.Encoding]::UTF8.GetString("+encoded(source)+");$t=$null;$e=$null;$a=[Management.Automation.Language.Parser]::ParseInput($s,[ref]$t,[ref]$e);@{status=$(if(@($e).Count -eq 0){'PASS_PARSE_ONLY'}else{'FAIL_PARSE_ONLY'});errors=@($e|ForEach-Object{@{id=$_.ErrorId;message=$_.Message;line=$_.Extent.StartLineNumber}});root=$root;rootPresent=(Test-Path -LiteralPath $root);sshd=(Get-Service sshd).Status.ToString();powershell=$PSVersionTable.PSVersion.ToString();nativeExecuted=$false}|ConvertTo-Json -Depth 5 -Compress;"
elif MODE=='upload':
    pre=json.loads((P/'root-preflight-first.stdout').read_text(encoding='utf-8-sig'));assert pre['status']=='PASS_PARSE_ONLY' and not pre['rootPresent']
    files=[('.owner',owner.encode()),('registry-value-intent.exe',(P/'source-final/registry-value-intent.exe').read_bytes()),('run-windows-probe.ps1',(P/'root-diagnostic-runner-first.ps1').read_bytes())]
    assert hashlib.sha256(files[1][1]).hexdigest()==EXE_SHA and hashlib.sha256(files[2][1]).hexdigest()==s['sourceSha256']
    ps=prefix+scope+"if(Test-Path -LiteralPath $root){throw 'Owned directory exists'};$null=New-Item -ItemType Directory -Path $root -ErrorAction Stop;$rows=@();"
    for name,data in files:
        ps+="$p=Join-Path $root '"+name+"';$d="+encoded(data)+";$f=[IO.File]::Open($p,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);try{$f.Write($d,0,$d.Length)}finally{$f.Dispose()};$rows+=@{name='"+name+"';bytes=$d.Length;sha256=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()};"
    ps+="@{status='UPLOADED_FRESH_OWNED_FIXTURE';root=$root;files=$rows}|ConvertTo-Json -Depth 5 -Compress;"
elif MODE=='execute':
    u=json.loads((P/'root-upload-first.stdout').read_text(encoding='utf-8-sig'));assert u['status']=='UPLOADED_FRESH_OWNED_FIXTURE'
    ps=prefix+scope+"if([IO.File]::ReadAllText((Join-Path $root '.owner')) -cne '"+owner+"'){throw 'Wrong owner'};if((Get-FileHash -LiteralPath (Join-Path $root 'run-windows-probe.ps1') -Algorithm SHA256).Hash.ToLowerInvariant() -cne '"+s['sourceSha256']+"'){throw 'Runner hash differs'};& (Join-Path $root 'run-windows-probe.ps1') -FixtureDirectory $root -ExpectedSha256 '"+EXE_SHA+"';"
elif MODE=='collect':
    ps=prefix+scope+"if([IO.File]::ReadAllText((Join-Path $root '.owner')) -cne '"+owner+"'){throw 'Wrong owner'};$rows=@();foreach($n in @('native-first.json','native-first.stdout.log','native-first.stderr.log')){$p=Join-Path $root $n;$f=Get-Item -LiteralPath $p;if($f.Length -gt 2MB){throw 'Report over bound'};$d=[IO.File]::ReadAllBytes($p);$rows+=@{name=$n;bytes=$d.Length;sha256=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant();base64=[Convert]::ToBase64String($d)}};@{root=$root;files=$rows}|ConvertTo-Json -Depth 5 -Compress;"
else:raise ValueError(MODE)
label=P/('root-'+MODE+'-first');label.with_suffix('.input.ps1').open('x').write(ps+'\n')
out=label.with_suffix('.stdout');err=label.with_suffix('.stderr');report=label.with_suffix('.json');assert not any(x.exists() for x in (out,err,report))
t=time.monotonic()
with out.open('xb') as a,err.open('xb') as b:
    child=subprocess.Popen(['ssh','-T','-o','BatchMode=yes','-o','ConnectTimeout=10','future-classroom-win11','powershell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command -'],stdin=subprocess.PIPE,stdout=a,stderr=b)
    child.communicate((ps+'\n').encode())
r={'mode':MODE,'sshExitCode':child.returncode,'elapsedSeconds':time.monotonic()-t,'sourceSha256':s['sourceSha256'],'stdoutBytes':out.stat().st_size,'stdoutSha256':hashlib.sha256(out.read_bytes()).hexdigest(),'stderrBytes':err.stat().st_size,'stderrSha256':hashlib.sha256(err.read_bytes()).hexdigest()}
if MODE=='collect' and child.returncode==0:
    raw=json.loads(out.read_text(encoding='utf-8-sig'));dest=P/'windows-original-first';dest.mkdir()
    for file in raw['files']:
        name=file['name'];assert '/' not in name and '\\' not in name and name not in ('.','..')
        data=base64.b64decode(file['base64'],validate=True);assert len(data)==file['bytes'] and hashlib.sha256(data).hexdigest()==file['sha256']
        with (dest/name).open('xb') as f:f.write(data)
    r['copiedOriginals']=[{k:v for k,v in f.items() if k!='base64'} for f in raw['files']]
report.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r),flush=True)
if MODE!='collect':print(out.read_text(encoding='utf-8-sig'),flush=True)
print(err.read_text(encoding='utf-8-sig'),flush=True)
raise SystemExit(child.returncode)
