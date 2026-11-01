"""Prepared registry-only fixture controller. Root explicitly --execute after review.
One local 120s deadline includes staging, native /i/finally /x and retention.
Native receives only the remaining budget minus a 20s retention reserve.
"""
from pathlib import Path, PureWindowsPath
import argparse, base64, gzip, hashlib, json, re, subprocess, sys, time
sys.dont_write_bytecode = True
ROOT = Path('/tmp/fc-round75-installer-maintenance')
E = ROOT / 'evidence'
D = E / 'registry-keypath-controller-preparation-second'
AUTHOR = ROOT / 'registry-keypath-fixture-ae966fa5484f4235b154b7b874ff728f'
NONCE = 'ae966fa5484f4235b154b7b874ff728f'
PRODUCT = '{60E68721-62F0-5656-B5BE-144E6B889AB6}'
MARKER = 'future-classroom-owned-r75-registry-keypath/v1\n'
HOST = 'elior@192.168.10.212'
IDENTITY = '/config/.local/share/future-classroom-test-access/win11-192.168.10.212'
INPUTS = {
 'fixture.msi': (AUTHOR / 'fixture.msi', 'd3c6d5e4937d5dd673e9835a7fbc8d5c7d03af8b0527e1a12d565da1a0e4e248'),
 'fixture-spec.json': (AUTHOR / 'fixture-spec.json', 'a41186d39d1ed187bfdbe99b0d94cb2e4eefc6fc36866257bddaaff34edd643f'),
 'query-registry-x86.ps1': (D / 'query-registry-x86.ps1', '920cc1bfb56913efc4e73268cb8e45ac5053b404c25dd1db8e8ed22aa5a3d1d4'),
}
STATIC = E / 'registry-keypath-fixture-preparation-second/actual-check-second.json'
STATIC_SHA = 'c5769adf0fd3cddf8505f144cf896a84cc4e643564c780257e1d975b664d661d'
ALLOWED = {'.owner','fixture.msi','fixture-spec.json','query-registry-x86.ps1','controller.ps1','controller-report.json','Before.json','Installed.json','After.json','install.log','uninstall.log'} | {x+'.'+y+'.log' for x in ('query-Before','query-Installed','query-After','install','uninstall') for y in ('stdout','stderr')}
TEMPS = {'query-temp-Before','query-temp-Installed','query-temp-After'}
CREATE = r"""$r=Join-Path ([IO.Path]::GetFullPath($env:TEMP)) ('FutureClassroom-r75-registry-keypath-'+$c.nonce)
if(Test-Path -LiteralPath $r){throw 'ROOT_NOT_FRESH'}
$d=[IO.Path]::GetDirectoryName($r);while($d){$i=Get-Item -LiteralPath $d -Force;if(-not $i.PSIsContainer -or ($i.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'ANCESTOR_REPARSE'};$d=[IO.Path]::GetDirectoryName($d)}
[IO.Directory]::CreateDirectory($r)|Out-Null
$acl=New-Object Security.AccessControl.DirectorySecurity;$acl.SetAccessRuleProtection($true,$false)
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User
foreach($s in @($sid,[Security.Principal.SecurityIdentifier]::new('S-1-5-18'),[Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))){$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($s,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))}
[IO.Directory]::SetAccessControl($r,$acl)
$f=[IO.File]::Open((Join-Path $r '.owner'),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
try{$b=[Text.Encoding]::ASCII.GetBytes($c.marker);$f.Write($b,0,$b.Length)}finally{$f.Dispose()}
@{status='CREATED';root=$r;nonce=$c.nonce}|ConvertTo-Json -Compress
"""
INVOKE = r"""$p=Join-Path $c.root 'controller.ps1'
if((Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant() -cne $c.controllerSha256){throw 'CONTROLLER_SHA'}
& $p -Mode $c.mode -Root $c.root -RelativeFile $c.relative -ExpectedProofSha256 $c.proof -AvailableMilliseconds $c.available
exit $LASTEXITCODE
"""
PROOF_PREFLIGHT = r"""$p=Join-Path $c.root 'cleanup-proof.json';if(Test-Path -LiteralPath $p){throw 'PROOF_NOT_FRESH'};'PROOF_ABSENT'"""
def need(value, code):
    if not value: raise AssertionError(code)
def sha(data): return hashlib.sha256(data).hexdigest()
def scoped(path):
    p = Path(path)
    need(p.is_absolute() and p.resolve().is_relative_to(E) and not any(x.is_symlink() for x in [p,*p.parents] if x == ROOT or ROOT in x.parents), 'OWNED_E_REGULAR_SCOPE')
    return p
def fact(path):
    p = Path(path)
    need(p.is_file() and not p.is_symlink(), 'REGULAR_INPUT')
    b = p.read_bytes()
    return dict(path=str(p), bytes=len(b), sha256=sha(b))
def eligible_cleanup(r):
    if not all(r.get(k) is True for k in ('identityVerified','beforeAbsent','installAttempted','afterAbsent')): return False
    commands = r.get('commands', [])
    if not isinstance(commands,list): return False
    xs = [c for c in commands if c.get('name') == 'uninstall']
    return (len(xs)==1 and xs[0].get('exitCode') in (0,1605)
            and all(c.get('actualExit') is True and c.get('pipesComplete') is True and c.get('timedOut') is False for c in commands))
def validate_snapshot(s, root):
    need(s.get('status')=='SNAPSHOT_ONLY' and s.get('root')==root and s.get('nonce')==NONCE, 'SNAPSHOT_IDENTITY')
    rows=s.get('files');need(isinstance(rows,list) and 1<=len(rows)<=128,'MEMBER_COUNT')
    seen=set();total=0
    for row in rows:
        need(set(row)=={'path','bytes','sha256'},'EXACT_FILE_FACT')
        name=row['path'];need(isinstance(name,str) and re.fullmatch(r'[A-Za-z0-9_.\-/]+',name) and '\\' not in name,'MEMBER_PATH')
        parts=name.split('/');need(all(p not in ('','.','..') for p in parts),'MEMBER_COMPONENT')
        need((len(parts)==1 and name in ALLOWED) or (len(parts)>1 and parts[0] in TEMPS),'MEMBER_ALLOWLIST')
        need(name.casefold() not in seen,'MEMBER_DUPLICATE');seen.add(name.casefold())
        need(type(row['bytes']) is int and 0<=row['bytes']<=16777216,'FILE_BOUND')
        need(isinstance(row['sha256'],str) and re.fullmatch('[0-9a-f]{64}',row['sha256']),'FILE_SHA')
        total+=row['bytes']
    need(total<=67108864,'TREE_BOUND')
    need({'.owner',*INPUTS,'controller.ps1','controller-report.json'} <= {r['path'] for r in rows},'REQUIRED_ORIGINALS')
    return rows

def remote_command(script, config):
    packed=base64.b64encode(json.dumps(config,separators=(',',':')).encode()).decode()
    prefix="$ErrorActionPreference='Stop';[Console]::OutputEncoding=New-Object Text.UTF8Encoding($false);if($PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1){throw 'EXPECTED_DESKTOP51'};$c=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('"+packed+"'))|ConvertFrom-Json;"
    body=base64.b64encode(gzip.compress((prefix+script).encode(),mtime=0)).decode()
    decoder="$b=[Convert]::FromBase64String('"+body+"');$m=[IO.MemoryStream]::new();$m.Write($b,0,$b.Length);$m.Position=0;$g=[IO.Compression.GZipStream]::new($m,[IO.Compression.CompressionMode]::Decompress);$r=[IO.StreamReader]::new($g,[Text.Encoding]::UTF8);try{$s=$r.ReadToEnd()}finally{$r.Dispose();$g.Dispose();$m.Dispose()};& ([ScriptBlock]::Create($s))"
    encoded=base64.b64encode(decoder.encode('utf-16-le')).decode();need(len(encoded)<=7500,'ENCODED_COMMAND_BOUND')
    return ['ssh','-i',IDENTITY,'-o','BatchMode=yes','-o','ConnectTimeout=8',HOST,'powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand',encoded]
def verify_sources():
    need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n','ROOT_OWNER')
    need((D/'.owner').read_bytes()==b'future-classroom-owned-r75-registry-keypath-controller-preparation-second/v1\n','PREP_OWNER')
    need(fact(STATIC)['sha256']==STATIC_SHA,'ORIGINAL_STATIC_PASS_IDENTITY')
    for path,expected in INPUTS.values(): need(fact(path)['sha256']==expected,'ORIGINAL_INPUT_SHA')
    return {name:fact(path) for name,(path,_) in INPUTS.items()}
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report',type=Path,required=True);parser.add_argument('--execute',action='store_true')
    a=parser.parse_args();report=scoped(a.report);need(not report.exists() and report.parent.is_dir(),'FRESH_REPORT')
    inputs=verify_sources();controller=D/'controller.ps1';controller_fact=fact(controller)
    if not a.execute:
        print(json.dumps(dict(status='PREPARED_NOT_EXECUTED',inputs=inputs,controller=controller_fact,productCode=PRODUCT,originalOverallSeconds=120,retentionReserveSeconds=20)));return 0
    result=dict(status='RUNNING',nonce=NONCE,productCode=PRODUCT,inputs=inputs,controller=controller_fact,operations=[],cleanup='NOT_RUN',originalOverallSeconds=120,retentionReserveSeconds=20)
    rawdir=report.with_name(report.stem+'-original-bytes');rawdir.mkdir();(rawdir/'.local-owner').write_bytes(b'future-classroom-owned-r75-registry-capture/v1\n')
    def save():
        report.write_text(json.dumps(result,indent=2)+'\n')
    save()
    began_overall=time.monotonic()
    def remaining(): return max(0.0,120-(time.monotonic()-began_overall))
    def call(label,cmd,seconds,allow_nonzero=False):
        outpath=report.with_name(report.stem+'-'+label+'.stdout.log');errpath=report.with_name(report.stem+'-'+label+'.stderr.log')
        row=dict(label=label,command=cmd,actualExit=False,timedOut=False,exitCode=None);result['operations'].append(row)
        began=time.monotonic()
        need(remaining()>0,'ORIGINAL120S_OVERALL_EXPIRED')
        seconds=min(seconds,remaining())
        with outpath.open('xb') as out,errpath.open('xb') as err:
            child=subprocess.Popen(cmd,stdin=subprocess.DEVNULL,stdout=out,stderr=err)
            row['pid']=child.pid
            try: row['exitCode']=child.wait(timeout=seconds)
            except subprocess.TimeoutExpired:
                row['timedOut']=True;child.kill();row['exitCode']=child.wait()
            row['actualExit']=True
        row.update(elapsedSeconds=time.monotonic()-began,stdout=fact(outpath),stderr=fact(errpath));save()
        need(not row['timedOut'] and (allow_nonzero or row['exitCode']==0),'TRANSPORT_FAILURE_'+label)
        return outpath.read_bytes(),row['exitCode']
    root=None;config={}
    try:
        made=json.loads(call('01-create',remote_command(CREATE,dict(nonce=NONCE,marker=MARKER)),15)[0]);root=made['root']
        wp=PureWindowsPath(root);need(made['status']=='CREATED' and made['nonce']==NONCE and wp.is_absolute() and str(wp)==root and wp.name=='FutureClassroom-r75-registry-keypath-'+NONCE,'REMOTE_OWNED_ROOT')
        result['remoteRoot']=root;save()
        upload={**{n:p for n,(p,_) in INPUTS.items()},'controller.ps1':controller}
        def scp(label,path,name):
            return call(label,['scp','-i',IDENTITY,'-o','BatchMode=yes','-o','ConnectTimeout=8',str(path),HOST+':'+root.replace('\\','/')+'/'+name],20)
        for i,(name,path) in enumerate(upload.items()): scp('02-upload-'+str(i),path,name)
        config=dict(root=root,controllerSha256=controller_fact['sha256'],mode='Run',relative='',proof='',available=max(1,int((remaining()-20)*1000)))
        need(config['available']>=55000,'REMAINING_NATIVE_CLEANUP_RESERVE')
        _,exitcode=call('03-original120s-run',remote_command(INVOKE,config),135,True)
        result['nativeSshExitCode']=exitcode
        config['available']=120000  # Snapshot/Read/Cleanup do not start native work.
        # A transport timeout above skips all deletion: the remote original owner
        # may remain. No disconnect or failure is treated as a returned child.
        config['mode']='Snapshot';snap=json.loads(call('04-snapshot',remote_command(INVOKE,config),20)[0]);rows=validate_snapshot(snap,root)
        result['snapshot']=snap;save()
        for i,row in enumerate(rows):
            config.update(mode='Read',relative=row['path']);encoded=call('05-read-'+str(i),remote_command(INVOKE,config),15)[0].strip()
            data=base64.b64decode(encoded,validate=True);need(len(data)==row['bytes'] and sha(data)==row['sha256'],'REMOTE_CAPTURE_IDENTITY')
            dest=rawdir.joinpath(*row['path'].split('/'));dest.parent.mkdir(parents=True,exist_ok=True)
            with dest.open('xb') as f:f.write(data)
            need(dest.read_bytes()==data,'LOCAL_REOPEN_ORIGINAL_BYTES')
        native=json.loads((rawdir/'controller-report.json').read_bytes());result['nativeReport']=native
        need(eligible_cleanup(native),'NATIVE_EFFECT_OR_OWNER_RETAINS_ROOT')
        proof=dict(status='PASS_LOCAL_REOPENED_ORIGINAL_BYTES',root=root,nonce=NONCE,files=rows)
        proofpath=rawdir/'cleanup-proof.json';proofpath.write_text(json.dumps(proof,indent=2)+'\n');proofsha=fact(proofpath)['sha256']
        call('06-proof-absent',remote_command(PROOF_PREFLIGHT,config),10)
        scp('07-upload-retention-proof',proofpath,'cleanup-proof.json')
        config.update(mode='Cleanup',relative='',proof=proofsha)
        clean=json.loads(call('08-exact-owned-root-cleanup',remote_command(INVOKE,config),20)[0])
        need(clean['status']=='REMOVED_EXACT_OWNED_NONCE_ROOT' and clean['root']==root and clean['nonce']==NONCE and clean['retentionProofSha256']==proofsha,'CLEANUP_RECEIPT')
        result['cleanup']='PASS_EXACT_OWNED_ROOT_REMOVED';result['cleanupReceipt']=clean
        result['status']='PASS_MEASURED_FORMAL_UNINSTALL_AND_CLEANUP' if native['status']=='PASS_MEASURED_AND_FORMALLY_REMOVED' and exitcode==0 else 'FAIL_MEASUREMENT_CLEANUP_PASS'
    except Exception as error:
        result['status']='FAIL_RETAIN_ORIGINALS_AND_RESIDUAL_FACTS';result['failureType']=type(error).__name__;result['failureCode']=str(error)
        result['remoteResidualState']='UNKNOWN_RETAINED' if root else 'CREATE_NOT_CONFIRMED_NO_DELETION'
    finally:
        result['overallElapsedSeconds']=time.monotonic()-began_overall
        save()
    print(json.dumps(dict(status=result['status'],report=str(report),cleanup=result['cleanup'])))
    return 0 if result['status']=='PASS_MEASURED_FORMAL_UNINSTALL_AND_CLEANUP' else 1
if __name__=='__main__':raise SystemExit(main())
