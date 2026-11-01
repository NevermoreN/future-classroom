from pathlib import Path
import json,hashlib,difflib
P=Path(__file__).parent;p=P/'run-windows-probe.ps1';old=p.read_text();saved=(P/'implementation-final/run-windows-probe.ps1').read_text();assert old==saved
new=old
new=new.replace("$script:unknown = $false; $script:records", "$script:ownerKey = $null; $script:ownerHandle = $null; $script:ownerNew = $false; $script:markersWritten = $false\n$script:unknown = $false; $script:records")
a="$existing = $base.OpenSubKey($expectedRegistry,$false)\nif ($existing) { $existing.Dispose();$base.Dispose();throw 'Original owned HKCU root already exists; refusing adoption.' }\n"
assert a in new;new=new.replace(a,'')
insert=r'''# Native disposition, not a prior Absent observation, establishes this fixture's NEW root.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
namespace FcMsiFixtureOwnerV2 {
 public sealed class Creation {
  public int ParentOpenError, CreateError, ParentCloseError;
  public uint Disposition;
  public SafeRegistryHandle Key;
 }
 public static class Registry {
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
  static extern int RegOpenKeyExW(IntPtr key, string subkey, uint options, uint access, out IntPtr result);
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
  static extern int RegCreateKeyExW(IntPtr key, string subkey, uint reserved, string cls, uint options, uint access, IntPtr security, out IntPtr result, out uint disposition);
  [DllImport("advapi32.dll", ExactSpelling=true)] static extern int RegCloseKey(IntPtr key);
  public static Creation Create(string leaf) {
   if (leaf == null || !System.Text.RegularExpressions.Regex.IsMatch(leaf, @"\AFutureClassroom-MsiGuardian-Test-[a-f0-9]{32}\z")) throw new ArgumentException("Invalid fixture leaf.");
   Creation r = new Creation(); IntPtr parent;
   r.ParentOpenError = RegOpenKeyExW(new IntPtr(unchecked((int)0x80000001)), "Software", 0, 0x2001f | 0x200, out parent);
   if (r.ParentOpenError != 0) return r;
   try {
    IntPtr child; uint disposition;
    r.CreateError = RegCreateKeyExW(parent, leaf, 0, null, 0, 0x2001f | 0x200, IntPtr.Zero, out child, out disposition);
    r.Disposition = disposition;
    if (r.CreateError == 0) r.Key = new SafeRegistryHandle(child, true);
   } finally { r.ParentCloseError = RegCloseKey(parent); }
   return r;
  }
 }
}
'@
function Assert-OriginalsReturned {
    if($script:unknown){throw 'Physical operation unresolved.'}
    foreach($r in $script:records){
        if(-not $r.physicalMsiExit -or $r.guardianObserved.Count -ne 1){throw 'Original MSI/guardian not fully observed.'}
        $g=$r.guardianObserved[0]
        if(-not $g.ContainsKey('actualExit') -or -not $g.actualExit){throw 'Original guardian did not physically return.'}
    }
}
function Assert-KnownOwnerTree {
    if(-not $script:ownerNew -or -not $script:markersWritten -or $null -eq $script:ownerKey){throw 'No fully marked original NEW root.'}
    $k=$script:ownerKey
    foreach($n in @('ProbeNonce','OwnerSid')){if($k.GetValueKind($n) -ne [Microsoft.Win32.RegistryValueKind]::String){throw 'Owner marker kind differs.'}}
    if($k.GetValue('ProbeNonce') -cne $fixture.nonce -or $k.GetValue('OwnerSid') -cne $sid){throw 'Original owner markers differ.'}
    foreach($n in @($k.GetValueNames())){if(@('ProbeNonce','OwnerSid','CurrentOperation','Decision') -cnotcontains $n){throw 'Unknown root value; retain scope.'}}
    foreach($n in @($k.GetSubKeyNames())){if(@('Evidence','Payload') -cnotcontains $n){throw 'Unknown root child; retain scope.'}}
    $payload=$k.OpenSubKey('Payload',$false)
    if($payload){try{if(@($payload.GetSubKeyNames()).Count -ne 0){throw 'Unknown payload child.'};foreach($n in @($payload.GetValueNames())){if($n -cne 'OwnedPayload' -or $payload.GetValueKind($n) -ne [Microsoft.Win32.RegistryValueKind]::DWord -or $payload.GetValue($n) -ne 1){throw 'Unknown payload content.'}}}finally{$payload.Dispose()}}
    $ev=$k.OpenSubKey('Evidence',$false)
    if($ev){try{
        if(@($ev.GetValueNames()).Count -ne 0){throw 'Unknown evidence-parent values.'}
        $names=@($ev.GetSubKeyNames());if($names.Count -gt 5){throw 'Evidence count exceeded.'}
        foreach($name in $names){
            $rows=@($script:records|Where-Object{$_.operation -ceq $name});if($rows.Count -ne 1){throw 'Unknown evidence operation.'}
            $op=$ev.OpenSubKey($name,$false)
            try{
                if(@($op.GetValueNames()).Count -ne 0){throw 'Unknown operation values.'}
                $kids=@($op.GetSubKeyNames());if($kids.Count -ne 1 -or $rows[0].guardianObserved.Count -ne 1 -or $kids[0] -cne ('Guardian-'+$rows[0].guardianObserved[0].pid)){throw 'Unknown evidence process.'}
                $g=$op.OpenSubKey($kids[0],$false)
                try{
                    if(@($g.GetSubKeyNames()).Count -ne 0){throw 'Unknown guardian child.'}
                    $dwords=@('Pid','ArgCount','TransactionId','RequestedDecision','EventOpenError','ObserveAckWait','JoinExportPresent','EndExportPresent','ExportLookupError','JoinReturn','OwnerEventBeforeEnd','EndEntered','EndReturn','EndReturned','AboutToReturnCode')
                    foreach($n in @($g.GetValueNames())){
                        $kind=$g.GetValueKind($n);$v=$g.GetValue($n)
                        if($dwords -ccontains $n){if($kind -ne [Microsoft.Win32.RegistryValueKind]::DWord){throw 'Guardian DWORD kind differs.'}}
                        elseif($n -ceq 'CreationFileTime'){if($kind -ne [Microsoft.Win32.RegistryValueKind]::QWord){throw 'Guardian creation kind differs.'}}
                        elseif($n -ceq 'ActualSid'){if($kind -ne [Microsoft.Win32.RegistryValueKind]::String -or $v -cne $sid){throw 'Guardian SID differs.'}}
                        elseif($n -ceq 'Stage'){if($kind -ne [Microsoft.Win32.RegistryValueKind]::String -or @('Entered','Joined','EndReturned') -cnotcontains $v){throw 'Unknown guardian stage.'}}
                        else{throw 'Unknown guardian value.'}
                    }
                }finally{$g.Dispose()}
            }finally{$op.Dispose()}
        }
    }finally{$ev.Dispose()}}
}
'''
idx=new.index('function Read-ProductState');new=new[:idx]+insert+new[idx:]
new=new.replace("if($script:unknown){throw 'Previous physical operation is unresolved; no new install or cleanup allowed.'}","Assert-OriginalsReturned\n    if(-not $script:ownerNew -or -not $script:markersWritten){throw 'No fully marked NEW owner; no MSI allowed.'}")
new=new.replace("$operation=[Guid]::NewGuid().ToString('N');$control=$base.OpenSubKey($expectedRegistry,$true)\n    try{$control.SetValue('CurrentOperation',$operation,[Microsoft.Win32.RegistryValueKind]::String);$control.SetValue('Decision',$decision,[Microsoft.Win32.RegistryValueKind]::DWord)}finally{$control.Dispose()}","$operation=[Guid]::NewGuid().ToString('N');$control=$script:ownerKey\n    $control.SetValue('CurrentOperation',$operation,[Microsoft.Win32.RegistryValueKind]::String);$control.SetValue('Decision',$decision,[Microsoft.Win32.RegistryValueKind]::DWord)")
# Never proceed past an unseen guardian; an msiexec exit is not proof of guardian exit.
needle="        $row.guardianEvidence=@(Snapshot-Evidence $operation);$row.productAfter=Read-ProductState;"
assert needle in new
new=new.replace(needle,"        if($null -eq $guardian){$script:unknown=$true;$row.completion='UNKNOWN_GUARDIAN_NOT_RETAINED';throw 'Exact original guardian was not retained; no further MSI or cleanup.'}\n"+needle)
needle="        $row.completion='OBSERVED_ORIGINALS_ONLY';return $row"
assert needle in new
new=new.replace(needle,"        if($row.guardianEvidence.Count -ne 1 -or -not $row.guardianEvidence[0].Contains('EndReturned') -or $row.guardianEvidence[0].EndReturned -ne 1 -or -not $row.guardianEvidence[0].Contains('EndReturn') -or $row.guardianEvidence[0].EndReturn -ne 0){$script:unknown=$true;throw 'Exact transaction End success not observed; no next MSI.'}\n"+needle)
new=new.replace("if(($procStarted -and -not $proc.HasExited) -or ($guardian -and -not $guardian.HasExited))", "if($procStarted -and (-not $row.physicalMsiExit -or $null -eq $guardian -or -not $guardian.HasExited))")
needle="    $control=$base.CreateSubKey($expectedRegistry);try{$control.SetValue('ProbeNonce',$fixture.nonce,[Microsoft.Win32.RegistryValueKind]::String);$control.SetValue('OwnerSid',$sid,[Microsoft.Win32.RegistryValueKind]::String)}finally{$control.Dispose()}"
assert needle in new
new=new.replace(needle,"""    $created=[FcMsiFixtureOwnerV2.Registry]::Create('FutureClassroom-MsiGuardian-Test-'+$fixture.nonce)
    $script:ownerHandle=$created.Key
    $report.registryCreation=[ordered]@{parentOpenError=$created.ParentOpenError;createError=$created.CreateError;parentCloseError=$created.ParentCloseError;disposition=$created.Disposition;actualNew=$false;nonceWritten=$false;sidWritten=$false;markersComplete=$false;handleDisposed=$false}
    if($created.ParentOpenError -ne 0 -or $created.CreateError -ne 0){throw 'Native root creation failed; no ownership claimed.'}
    if($created.Disposition -ne 1){throw 'Native disposition is not NEW; existing root is never written or deleted.'}
    $script:ownerNew=$true;$report.registryCreation.actualNew=$true
    if($created.ParentCloseError -ne 0){throw 'Parent close failed; retain created root without MSI.'}
    $script:ownerKey=[Microsoft.Win32.RegistryKey]::FromHandle($script:ownerHandle,[Microsoft.Win32.RegistryView]::Registry32)
    $script:ownerKey.SetValue('ProbeNonce',$fixture.nonce,[Microsoft.Win32.RegistryValueKind]::String);$report.registryCreation.nonceWritten=$true
    $script:ownerKey.SetValue('OwnerSid',$sid,[Microsoft.Win32.RegistryValueKind]::String);$report.registryCreation.sidWritten=$true
    $script:markersWritten=$true;$report.registryCreation.markersComplete=$true""")
new=new.replace("    if(-not $script:unknown){\n        try{\n            $state=Read-ProductState", "    if($script:ownerNew -and $script:markersWritten -and -not $script:unknown){\n        try{\n            Assert-OriginalsReturned\n            Assert-KnownOwnerTree\n            $state=Read-ProductState")
a="""                $owned=$base.OpenSubKey($expectedRegistry,$false)
                if($owned){try{if($owned.GetValue('ProbeNonce') -cne $fixture.nonce -or $owned.GetValue('OwnerSid') -cne $sid){throw 'Ownership marker differs.'}}finally{$owned.Dispose()};$base.DeleteSubKeyTree($expectedRegistry,$false)}
                $report.cleanup='PASS_OWN_PRODUCT_ABSENT_AND_NONCE_REGISTRY_REMOVED'"""
b="""                Assert-OriginalsReturned
                Assert-KnownOwnerTree
                $base.DeleteSubKeyTree($expectedRegistry,$true)
                $script:ownerKey.Dispose();$script:ownerKey=$null
                $script:ownerHandle.Dispose();$report.registryCreation.handleDisposed=$true
                $remaining=$base.OpenSubKey($expectedRegistry,$false)
                if($remaining){$remaining.Dispose();throw 'Root still present after original owner handles closed.'}
                $report.cleanup='PASS_OWN_PRODUCT_ABSENT_AND_NONCE_REGISTRY_REMOVED'"""
assert a in new;new=new.replace(a,b)
a="""    }else{$report.cleanup='UNKNOWN_LIVE_OPERATION_RETAINED_NO_CLEANUP'}
    $report.unknownPhysicalOperation=$script:unknown;$base.Dispose()"""
b="""    }elseif($script:unknown){$report.cleanup='UNKNOWN_LIVE_OPERATION_RETAINED_NO_CLEANUP'}
    elseif($script:ownerNew){$report.cleanup='RETAINED_ACTUAL_NEW_ROOT_MARKERS_INCOMPLETE_NO_MSI_CLEANUP'}
    else{$report.cleanup='NOT_OWNED_NO_ROOT_OR_PRODUCT_MUTATION_OR_CLEANUP'}
    if($script:ownerKey){$script:ownerKey.Dispose();$script:ownerKey=$null}
    if($script:ownerHandle){$script:ownerHandle.Dispose();if($report.Contains('registryCreation')){$report.registryCreation.handleDisposed=$true}}
    $report.unknownPhysicalOperation=$script:unknown;$base.Dispose()"""
assert a in new;new=new.replace(a,b)
# Keep report filename first: Windows has not run either runner, and contract still one fresh owned target.
p.write_text(new)
(P/'owner-second.patch').open('x').write(''.join(difflib.unified_diff(old.splitlines(True),new.splitlines(True),fromfile='first/run-windows-probe.ps1',tofile='second/run-windows-probe.ps1')))
print(len(new.encode()),hashlib.sha256(new.encode()).hexdigest())
