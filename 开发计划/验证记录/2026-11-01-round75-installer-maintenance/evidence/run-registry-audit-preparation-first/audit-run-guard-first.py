"""R75 narrow HKCU32 RunGuard fixture name audit. Root explicitly --execute.
No script upload, registry value read/mutation, directory/task creation or EXE
launch beyond the authorized SSH PowerShell transport. Never reads a private key.
"""
from pathlib import Path
import argparse, base64, datetime, hashlib, json, re, subprocess, sys, time
sys.dont_write_bytecode=True
ROOT=Path('/tmp/fc-round75-installer-maintenance');E=ROOT/'evidence';D=E/'run-registry-audit-preparation-first'
HOST='elior@192.168.10.212'
IDENTITY='/config/.local/share/future-classroom-test-access/win11-192.168.10.212'
PREFIX='FutureClassroom-Test-InstallerRunGuard-'
PATTERN=re.compile(r'FutureClassroom-Test-InstallerRunGuard-([1-9][0-9]{0,9})-([0-9a-f]{32})')
REMOTE=r"""$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$base=$null;$software=$null
try {
 if($PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1){throw 'EXPECTED_DESKTOP51'}
 $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry32)
 $software=$base.OpenSubKey('Software',$false)
 $names=@();$malformed=0
 if($null -ne $software){
  if($software.SubKeyCount -gt 8192){throw 'SOFTWARE_SINGLE_LEVEL_NAME_BOUND'}
  foreach($name in $software.GetSubKeyNames()){
   if(-not $name.StartsWith('FutureClassroom-Test-InstallerRunGuard-',[StringComparison]::Ordinal)){continue}
   if($name -cmatch '^FutureClassroom-Test-InstallerRunGuard-([1-9][0-9]{0,9})-([0-9a-f]{32})$' -and [UInt64]::Parse($Matches[1]) -le 4294967295){
    $names+= $name
    if($names.Count -gt 256){throw 'CANONICAL_PREFIX_OUTPUT_BOUND'}
   }else{$malformed+=1}
  }
 }
 $names=@($names | Sort-Object -Unique)
 $result=[ordered]@{status='PASS_READONLY_HKCU32_PREFIX_NAMES';hive='HKCU';view='Registry32';parent='Software';prefix='FutureClassroom-Test-InstallerRunGuard-';names=$names;count=$names.Count;noncanonicalPrefixCount=$malformed;powershell=[ordered]@{version=$PSVersionTable.PSVersion.ToString();edition=$PSVersionTable.PSEdition};valuesRead=$false;registryChanged=$false;childKeysOpened=0;scope='Single Software key immediate names only; no subtree or values. Existing keys are not attributed or removed.'}
} finally {
 if($null -ne $software){$software.Dispose();$software=$null}
 if($null -ne $base){$base.Dispose();$base=$null}
}
$result['keyHandlesClosed']=$true
$result | ConvertTo-Json -Depth 5 -Compress
"""
def need(v,c):
    if not v:raise AssertionError(c)
def sha(data):return hashlib.sha256(data).hexdigest()
def safe(path):
    p=Path(path);need(p.is_absolute() and p.resolve().is_relative_to(E),'R75_EVIDENCE_PATH_ONLY')
    need(not any(x.is_symlink() for x in [p,*p.parents] if x==E or E in x.parents),'NO_SYMLINK_REPORT');return p
def fact(path):
    p=safe(path);need(p.is_file() and p.suffix.lower() not in {'.key','.pfx','.p12'},'REGULAR_NONPRIVATE_REPORT');data=p.read_bytes();return dict(path=str(p),bytes=len(data),sha256=sha(data))
def canonical(names):
    need(isinstance(names,list) and len(names)<=256 and all(isinstance(n,str) for n in names) and names==sorted(set(names)),'BOUNDED_UNIQUE_SORTED_NAMES')
    for name in names:
        m=PATTERN.fullmatch(name);need(m is not None and 1<=int(m[1])<=4294967295,'EXACT_CANONICAL_PID_NONCE_NAME')
    return names
def validate_actual(actual):
    need(actual['status']=='PASS_READONLY_HKCU32_PREFIX_NAMES' and actual['hive']=='HKCU' and actual['view']=='Registry32' and actual['parent']=='Software' and actual['prefix']==PREFIX,'NARROW_READONLY_REGISTRY_SCOPE')
    names=canonical(actual['names']);need(actual['count']==len(names) and type(actual['noncanonicalPrefixCount']) is int and actual['noncanonicalPrefixCount']>=0,'BOUNDED_EXACT_COUNTS')
    need(actual['powershell']['version'].startswith('5.1.') and actual['powershell']['edition']=='Desktop' and actual['valuesRead'] is False and actual['registryChanged'] is False and actual['keyHandlesClosed'] is True and actual['childKeysOpened']==0,'ACTUAL_DESKTOP51_NO_VALUES_OR_MUTATIONS_AND_HANDLES_CLOSED');return names
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--label',required=True);p.add_argument('--report',type=Path,required=True);p.add_argument('--before-report',type=Path);p.add_argument('--before-report-sha256');p.add_argument('--execute',action='store_true');a=p.parse_args()
    need(a.label=='before' or re.fullmatch(r'after[1-9][0-9]{0,2}',a.label),'BEFORE_OR_CANONICAL_AFTERN_LABEL')
    need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n' and (D/'.owner').read_bytes()==b'future-classroom-owned-r75-run-registry-audit-preparation/v1\n','EXACT_R75_OWNERS')
    report=safe(a.report);stdout=report.with_suffix('.stdout.log');stderr=report.with_suffix('.stderr.log');need(all(not x.exists() for x in (report,stdout,stderr)),'EXCLUSIVE_FRESH_REPORT_AND_RAW')
    need((a.before_report is None)==(a.before_report_sha256 is None),'BEFORE_PATH_SHA_PAIRED')
    need(a.label!='before' or a.before_report is None,'BEFORE_CANNOT_COMPARE_TO_ANOTHER_BEFORE')
    before=None;before_fact=None
    if a.before_report is not None:
        need(re.fullmatch('[0-9a-f]{64}',a.before_report_sha256),'EXPLICIT_BEFORE_SHA')
        before_fact=fact(a.before_report);need(before_fact['sha256']==a.before_report_sha256,'EXACT_BEFORE_ORIGINAL');before=json.loads(safe(a.before_report).read_bytes())
        need(before['status']=='PASS_R75_READONLY_RUN_GUARD_PREFIX_AUDIT_ONLY' and before['label']=='before' and before['actualExit'] is True and before['exitCode']==0,'ACTUAL_SUCCESSFUL_BEFORE_BASELINE')
        validate_actual(before['actual'])
    encoded=base64.b64encode(REMOTE.encode('utf-16-le')).decode();need(len(encoded)<=12000,'SHORT_ENCODED_COMMAND')
    cmd=['ssh','-i',IDENTITY,'-o','BatchMode=yes','-o','ConnectTimeout=8',HOST,'powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',encoded]
    if not a.execute:print(json.dumps(dict(status='PREPARED_READONLY_REGISTRY_AUDIT_NOT_EXECUTED',label=a.label,host=HOST,originalBudgetSeconds=35,encodedCommandCharacters=len(encoded),beforeReport=before_fact,registryScope='HKCU32 Software immediate exact-prefix canonical names only; no values or deletion.',reportCreated=False)));return 0
    result=dict(status='RUNNING',label=a.label,startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),host=HOST,originalBudgetSeconds=35,sshCommand=cmd,remoteSourceSha256=sha(REMOTE.encode()),helper=fact(Path(__file__)),beforeReport=before_fact,privateKey='Path used only as authorized SSH transport; contents/mode not inspected.',scope='Read-only current-user32 Software immediate key names. No child key/value/Run traversal, mutations, directory/task creation, script upload, EXE/MSI/browser execution or attribution of existing keys.')
    with report.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
    started=time.monotonic();code=None;out=b'';err=b''
    try:
        # subprocess.run kills/waits its direct SSH child on timeout. Timeout
        # never claims remote completion; no new budget or second SSH is used.
        completed=subprocess.run(cmd,capture_output=True,timeout=35);code=completed.returncode;out=completed.stdout;err=completed.stderr
        result.update(exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-started)
        need(code==0 and result['elapsedSeconds']<=35,'ACTUAL_SSH_PARENT_EXIT0_WITHIN_ORIGINAL35S');need(len(out)<=131072 and len(err)<=131072,'BOUNDED_REMOTE_OUTPUT')
        actual=json.loads(out.decode('utf-8-sig'));names=validate_actual(actual);result['actual']=actual
        if before is not None:
            previous=set(before['actual']['names']);current=set(names);delta=sorted(current-previous);missing=sorted(previous-current)
            result['comparison']=dict(newDelta=delta,newDeltaCount=len(delta),baselineMissingNames=missing,baselineMissingCount=len(missing),existingBaselineNamesNotAttributedOrRemoved=True)
            need(not delta,'NEW_MATCHING_RUN_GUARD_RESIDUE')
            need(fact(a.before_report)==before_fact,'BEFORE_BASELINE_UNCHANGED')
        else:result['comparison']=dict(status='NOT_REQUESTED_BASELINE_ONLY',existingNamesNotAttributedOrRemoved=True)
        result['status']='PASS_R75_READONLY_RUN_GUARD_PREFIX_AUDIT_ONLY'
    except subprocess.TimeoutExpired as error:
        out=error.stdout or b'';err=error.stderr or b'';result.update(status='FAIL_READONLY_AUDIT_TIMEOUT_UNKNOWN_REMOTE',exitCode=None,actualExit=False,elapsedSeconds=time.monotonic()-started,error='ORIGINAL35S_BUDGET_EXPIRED_REMOTE_COMPLETION_UNKNOWN')
    except Exception as error:result.update(status='FAIL_READONLY_AUDIT_OR_RESIDUE',exitCode=code,elapsedSeconds=time.monotonic()-started,error=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'SSH_OR_JSON_CAPTURE_FAILURE'))
    for name,path,data in (('stdout',stdout,out),('stderr',stderr,err)):
        with path.open('xb') as f:f.write(data)
        result[name]=dict(**fact(path),base64=base64.b64encode(data).decode())
    result['completedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat();report.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(dict(status=result['status'],label=a.label,exitCode=code,report=fact(report),count=result.get('actual',{}).get('count'),newDeltaCount=result.get('comparison',{}).get('newDeltaCount'))));return 0 if result['status']=='PASS_R75_READONLY_RUN_GUARD_PREFIX_AUDIT_ONLY' else 1
if __name__=='__main__':raise SystemExit(main())
