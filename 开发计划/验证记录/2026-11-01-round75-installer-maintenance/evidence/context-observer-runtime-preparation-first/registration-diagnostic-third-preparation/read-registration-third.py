"""Prepare/execute only read-only COM count/type diagnostics, never absence gate.
No observer/MSI/registration mutation, directory/task creation or key read.
"""
from pathlib import Path
import argparse, base64, datetime, hashlib, json, subprocess, time
R=Path('/tmp/fc-round75-installer-maintenance');E=R/'evidence';D=E/'context-observer-runtime-preparation-first/registration-diagnostic-third-preparation'
PRODUCT='{4BFAFA4A-6E0D-5FAB-90F7-4C9967F2ED46}'
REMOTE=r"""$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
function Scalar($v){$type=$null;$text=$null;if($null-ne$v){$type=$v.GetType().FullName;$text=[string]$v};return @{isNull=($null-eq$v);type=$type;value=$text}}
$i=$null;$x=$null;$out=[ordered]@{status='ACTUAL_READONLY_COM_REGISTRATION_DIAGNOSTIC_ONLY';product='{4BFAFA4A-6E0D-5FAB-90F7-4C9967F2ED46}';userSidArgument='';contexts=7;absenceGate='NOT_EVALUATED';powershell=$PSVersionTable.PSVersion.ToString()}
try{
 $i=New-Object -ComObject WindowsInstaller.Installer
 $x=$i.GetType().InvokeMember('ProductsEx',[Reflection.BindingFlags]::GetProperty,$null,$i,@($out.product,'',7))
 $out.collectionType=$x.GetType().FullName;$out.isComObject=[Runtime.InteropServices.Marshal]::IsComObject($x)
 $psCount=$x.Count;$out.psAdaptedCount=(Scalar $psCount)
 $out.psCountPropertyPresent=($null-ne$x.PSObject.Properties['Count'])
 try{$n=$x.GetType().InvokeMember('Count',[Reflection.BindingFlags]::GetProperty,$null,$x,@());$out.comReflectedCount=@{status='RETURNED';scalar=(Scalar $n)}}catch{$out.comReflectedCount=@{status='ERROR';exceptionType=$_.Exception.GetType().FullName;hresult=$_.Exception.HResult;message=$_.Exception.Message}}
 $s=$i.GetType().InvokeMember('ProductState',[Reflection.BindingFlags]::GetProperty,$null,$i,@($out.product));$out.productState=(Scalar $s)
 $row=@{status='ACTUAL_READONLY_CALLER_AND_MACHINE_CONTEXTS';product=$out.product;count=$psCount;productState=$s;contexts=7;userSidArgument='';otherUsers='NOT_QUERIED';cache='NOT_QUERIED'}
 $out.returnedRow=@{type=$row.GetType().FullName;indexCount=(Scalar $row['count']);dotCount=(Scalar $row.count);psBaseDictionaryCount=(Scalar $row.psbase.Count);indexState=(Scalar $row['productState']);dotState=(Scalar $row.productState)}
}finally{if($null-ne$x){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($x)};if($null-ne$i){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($i)}}
$out.comObjectsReleased=$true;$out.observerOrMsiExecuted=$false;$out.registryOrRegistrationChanged=$false
$out|ConvertTo-Json -Depth 10 -Compress
"""
def need(v,c):
    if not v:raise AssertionError(c)
def sha(b):return hashlib.sha256(b).hexdigest()
def fact(path):
    p=Path(path);need(p.resolve().is_relative_to(R) and not p.is_symlink() and p.is_file() and p.suffix.lower() not in {'.key','.pfx','.p12'},'OWNED_NONPRIVATE_REPORT');b=p.read_bytes();return dict(path=str(p),bytes=len(b),sha256=sha(b))
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--report',type=Path,required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
    need((R/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n' and (D/'.owner').read_bytes()==b'future-classroom-owned-r75-registration-diagnostic-third/v1\n','EXACT_OWNERS')
    need(a.report.is_absolute() and a.report.parent.resolve().is_relative_to(E) and not a.report.exists(),'FRESH_OWNED_REPORT');stdout=a.report.with_suffix('.stdout.log');stderr=a.report.with_suffix('.stderr.log');need(not stdout.exists() and not stderr.exists(),'FRESH_RAW_CAPTURE')
    bootstrap="$ErrorActionPreference='Stop';[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);$s=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($s))";encoded=base64.b64encode(bootstrap.encode('utf-16-le')).decode()
    cmd=['ssh','-i','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','-o','BatchMode=yes','-o','ConnectTimeout=8','elior@192.168.10.212','powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',encoded]
    if not a.execute:print(json.dumps(dict(status='PREPARED_READONLY_COM_DIAGNOSTIC_NOT_EXECUTED',product=PRODUCT,absenceGate='NOT_EVALUATED',bootstrapCharacters=len(encoded))));return 0
    source=REMOTE.encode();result=dict(status='RUNNING',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),command=cmd,stdin=dict(bytes=len(source),sha256=sha(source),base64=base64.b64encode(source).decode()),originalBudgetSeconds=35,scope='Read-only exact-product COM/type/Count diagnostics only; no absence acceptance, observer/MSI execution or source changes.')
    with a.report.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
    began=time.monotonic();code=None;out=b'';err=b''
    try:
        c=subprocess.run(cmd,input=source,capture_output=True,timeout=35);code=c.returncode;out=c.stdout;err=c.stderr;result.update(actualExit=True,exitCode=code)
        need(code==0,'ACTUAL_DIAGNOSTIC_SSH_EXIT0');actual=json.loads(out.decode('utf-8-sig'));need(actual['status']=='ACTUAL_READONLY_COM_REGISTRATION_DIAGNOSTIC_ONLY' and actual['product']==PRODUCT and actual['absenceGate']=='NOT_EVALUATED' and actual['comObjectsReleased'] is True and actual['observerOrMsiExecuted'] is False and actual['registryOrRegistrationChanged'] is False,'READONLY_DIAGNOSTIC_SCOPE');result.update(status='ACTUAL_READONLY_DIAGNOSTIC_RETURNED_NOT_ABSENCE_ACCEPTANCE',actual=actual)
    except subprocess.TimeoutExpired as error:out=error.stdout or b'';err=error.stderr or b'';result.update(status='FAIL_DIAGNOSTIC_TIMEOUT_UNKNOWN_REMOTE',actualExit=False,exitCode=None)
    except Exception as error:result.update(status='FAIL_DIAGNOSTIC_OR_CAPTURE',exitCode=code,error=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'READONLY_COM_OR_JSON_FAILURE'))
    for key,path,data in [('stdout',stdout,out),('stderr',stderr,err)]:
        with path.open('xb') as f:f.write(data)
        result[key]=dict(**fact(path),base64=base64.b64encode(data).decode())
    result.update(elapsedSeconds=time.monotonic()-began,completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat());a.report.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(status=result['status'],report=fact(a.report))));return 0 if result['status']=='ACTUAL_READONLY_DIAGNOSTIC_RETURNED_NOT_ABSENCE_ACCEPTANCE' else 1
if __name__=='__main__':raise SystemExit(main())
