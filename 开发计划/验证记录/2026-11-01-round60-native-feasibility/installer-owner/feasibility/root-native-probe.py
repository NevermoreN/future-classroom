from pathlib import Path
import base64, hashlib, json, subprocess, sys, time, datetime

ROOT = Path(__file__).parent
NONCE = 'e3365555af2a409eba9bb94ab37b0e28'
OWNER = 'future-classroom-owned-r60-msi-native/v1:' + NONCE
SCRIPT_SHA = 'e787bf4ff9bebf7a21c875e14fee7d32be7c4023b8bcfd9b3877febe0a36e2d5'
MSI_SHA = '30a9eb8887321a4dc7a8c0caa2d8a63147cae486744314c324a32341b665016e'
GUARDIAN_SHA = '01c9c1182b1adab40c94f1daa40e617f4d0ca00e513c30a991b8f849fc16415f'
MODE = sys.argv[1]
LABEL = 'root-native-' + MODE + '-first'
prefix = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);"
scope = "$root=Join-Path ([IO.Path]::GetTempPath()) 'FutureClassroomMsiOwner-" + NONCE + "';"

def encoded(data):
    return '[Convert]::FromBase64String(\'' + base64.b64encode(data).decode() + '\')'

if MODE == 'preflight':
    source = (ROOT/'implementation-owner-fourth/run-windows-probe.ps1').read_bytes()
    assert hashlib.sha256(source).hexdigest() == SCRIPT_SHA
    checker = (ROOT/'check-owner-third-contract.ps1').read_bytes()
    ps = prefix + scope + '$s=[Text.Encoding]::UTF8.GetString(' + encoded(source) + ');$c=[Text.Encoding]::UTF8.GetString(' + encoded(checker) + ');'
    ps += "& ([scriptblock]::Create($c)) -TextSource $s -ExpectedSourceSha256 '" + SCRIPT_SHA + "';"
    ps += "$b=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry32);$k=$b.OpenSubKey('Software\\FutureClassroom-MsiGuardian-Test-" + NONCE + "',$false);$present=$null -ne $k;if($k){$k.Dispose()};$b.Dispose();"
    ps += "@{status='READ_ONLY_PREFLIGHT';root=$root;rootPresent=(Test-Path -LiteralPath $root);registryPresent=$present;msiexecCount=@(Get-Process -Name msiexec -ErrorAction SilentlyContinue).Count;sshd=(Get-Service sshd).Status.ToString();sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;powershell=$PSVersionTable.PSVersion.ToString()}|ConvertTo-Json -Compress;"
elif MODE == 'upload':
    pre = json.loads((ROOT/'root-native-preflight-first.stdout').read_text(encoding='utf-8-sig').splitlines()[-1])
    assert pre['rootPresent'] is False and pre['registryPresent'] is False and pre['msiexecCount'] == 0
    files = [(ROOT/'build-fifth/fixture.msi',MSI_SHA),(ROOT/'build-fifth/guardian.exe',GUARDIAN_SHA),(ROOT/'build-fifth/fixture.json','a53cd183503fe2e426814bec11280138fa3b4dc54542f7b3e952f299d51eed0b'),(ROOT/'implementation-owner-fourth/run-windows-probe.ps1',SCRIPT_SHA)]
    ps = prefix + scope + "if(Test-Path -LiteralPath $root){throw 'Fresh owned directory already exists'};$null=New-Item -ItemType Directory -Path $root -ErrorAction Stop;"
    allfiles = [('.owner', OWNER.encode())]
    for file, sha in files:
        data = file.read_bytes()
        assert hashlib.sha256(data).hexdigest() == sha
        allfiles.append((file.name,data))
    ps += '$rows=@();'
    for name,data in allfiles:
        ps += "$p=Join-Path $root '"+name+"';$data="+encoded(data)+";$f=[IO.File]::Open($p,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);try{$f.Write($data,0,$data.Length)}finally{$f.Dispose()};$rows+=@{name='"+name+"';bytes=$data.Length;sha256=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()};"
    ps += "@{status='UPLOADED_FRESH_OWNED_FIXTURE';root=$root;files=$rows}|ConvertTo-Json -Depth 5 -Compress;"
elif MODE == 'execute':
    upload=json.loads((ROOT/'root-native-upload-first.stdout').read_text(encoding='utf-8-sig'))
    assert upload['status']=='UPLOADED_FRESH_OWNED_FIXTURE'
    ps=prefix+scope+"if([IO.File]::ReadAllText((Join-Path $root '.owner')) -cne '"+OWNER+"'){throw 'Wrong owner'};"
    ps+="if((Get-FileHash -LiteralPath (Join-Path $root 'run-windows-probe.ps1') -Algorithm SHA256).Hash.ToLowerInvariant() -cne '"+SCRIPT_SHA+"'){throw 'Runner digest differs'};"
    ps+="& (Join-Path $root 'run-windows-probe.ps1') -FixtureDirectory $root -ExpectedMsiSha256 '"+MSI_SHA+"' -ExpectedGuardianSha256 '"+GUARDIAN_SHA+"' -ExecuteOwnedProbe;"
elif MODE == 'collect':
    ps=prefix+scope+"if([IO.File]::ReadAllText((Join-Path $root '.owner')) -cne '"+OWNER+"'){throw 'Wrong owner'};$files=@();"
    ps+="foreach($f in @(Get-ChildItem -LiteralPath $root -File)){if($f.Name -eq 'windows-observations-first.json' -or $f.Name -like '*.msi.log'){if($f.Length -gt 16MB){throw 'Original file over bound'};$d=[IO.File]::ReadAllBytes($f.FullName);$files+=@{name=$f.Name;bytes=$d.Length;sha256=(Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash.ToLowerInvariant();base64=[Convert]::ToBase64String($d)}}};@{root=$root;files=$files}|ConvertTo-Json -Depth 5 -Compress;"
else:
    raise ValueError(MODE)

input_path=ROOT/(LABEL+'.input.ps1')
with input_path.open('x',encoding='utf-8') as stream: stream.write(ps+'\n')
receipt=ROOT/(LABEL+'.json');out=ROOT/(LABEL+'.stdout');err=ROOT/(LABEL+'.stderr')
assert not any(p.exists() for p in (receipt,out,err))
start=time.monotonic()
with out.open('xb') as a,err.open('xb') as b:
    process=subprocess.Popen(['ssh','-T','-o','BatchMode=yes','-o','ConnectTimeout=10','future-classroom-win11','powershell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command -'],stdin=subprocess.PIPE,stdout=a,stderr=b)
    process.communicate((ps+'\n').encode())
r={'mode':MODE,'sshExitCode':process.returncode,'elapsedSeconds':time.monotonic()-start,'completedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceSha256':SCRIPT_SHA,'stdoutBytes':out.stat().st_size,'stdoutSha256':hashlib.sha256(out.read_bytes()).hexdigest(),'stderrBytes':err.stat().st_size,'stderrSha256':hashlib.sha256(err.read_bytes()).hexdigest()}
if MODE=='collect' and process.returncode==0:
    raw=json.loads(out.read_text(encoding='utf-8-sig'));dest=ROOT/'windows-original-first';dest.mkdir()
    for file in raw['files']:
        name=file['name'];assert '/' not in name and '\\' not in name and name not in ('.','..')
        data=base64.b64decode(file['base64'],validate=True)
        assert len(data)==file['bytes'] and hashlib.sha256(data).hexdigest()==file['sha256']
        with (dest/name).open('xb') as stream:stream.write(data)
    r['copiedOriginals']=[{k:v for k,v in f.items() if k!='base64'} for f in raw['files']]
receipt.write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps(r),flush=True)
if MODE in ('preflight','upload','execute'):
    print(out.read_text(encoding='utf-8-sig'),flush=True)
print(err.read_text(encoding='utf-8-sig'),flush=True)
raise SystemExit(process.returncode)
