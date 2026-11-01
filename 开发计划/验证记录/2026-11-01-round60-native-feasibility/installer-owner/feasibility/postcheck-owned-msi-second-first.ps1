# Read-only, exact synthetic MSI addresses. No install/uninstall, registry/file write, process kill, WMI or COM.
$ErrorActionPreference='Stop'
$expectedSid='S-1-5-21-3896111207-1867804362-1971593122-1001'
$actualSid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
if($actualSid -cne $expectedSid){throw 'Wrong SID for per-user MSI postcheck'}
if([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess){throw 'Run this observation in 64-bit PowerShell to avoid filesystem redirect ambiguity'}
$filePaths=@(
 'C:\Users\Elior\AppData\Local\FutureClassroomMsiFeasibility-e3365555af2a409eba9bb94ab37b0e28',
 'C:\Users\Elior\AppData\Local\Temp\FutureClassroomMsiOwner-e3365555af2a409eba9bb94ab37b0e28',
 'C:\Windows\Installer\31cd160d.msi',
 'C:\Windows\Installer\31cd1611.msi',
 'C:\Windows\Installer\31cd1618.msi',
 'C:\Windows\Installer\31cd161c.msi',
 'C:\Windows\Installer\MSI166C.tmp',
 'C:\Windows\Installer\MSI1833.tmp',
 'C:\Windows\Installer\MSI19AC.tmp',
 'C:\Windows\Installer\MSI1B34.tmp',
 'C:\Windows\Installer\SourceHash{51C60258-197D-4EA2-8859-F59ABA07AC38}'
)
$registryPaths=@(
 [pscustomobject]@{hive='CurrentUser';key='Software\FutureClassroom-MsiGuardian-Test-e3365555af2a409eba9bb94ab37b0e28'},
 [pscustomobject]@{hive='CurrentUser';key='Software\Microsoft\Windows\CurrentVersion\Uninstall\{51C60258-197D-4EA2-8859-F59ABA07AC38}'},
 [pscustomobject]@{hive='LocalMachine';key='Software\Microsoft\Windows\CurrentVersion\Uninstall\{51C60258-197D-4EA2-8859-F59ABA07AC38}'},
 [pscustomobject]@{hive='CurrentUser';key='Software\Microsoft\Installer\Products\85206C15D7912AE488955FA9AB70CA83'},
 [pscustomobject]@{hive='LocalMachine';key='Software\Classes\Installer\Products\85206C15D7912AE488955FA9AB70CA83'},
 [pscustomobject]@{hive='CurrentUser';key='Software\Microsoft\Installer\Features\85206C15D7912AE488955FA9AB70CA83'},
 [pscustomobject]@{hive='LocalMachine';key='Software\Classes\Installer\Features\85206C15D7912AE488955FA9AB70CA83'},
 [pscustomobject]@{hive='LocalMachine';key='Software\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-21-3896111207-1867804362-1971593122-1001\Products\85206C15D7912AE488955FA9AB70CA83'},
 [pscustomobject]@{hive='CurrentUser';key='Software\Microsoft\Installer\UpgradeCodes\F675AEACB1384A245B3679B354BF5113'},
 [pscustomobject]@{hive='LocalMachine';key='Software\Classes\Installer\UpgradeCodes\F675AEACB1384A245B3679B354BF5113'},
 [pscustomobject]@{hive='LocalMachine';key='Software\Microsoft\Windows\CurrentVersion\Installer\UpgradeCodes\F675AEACB1384A245B3679B354BF5113'},
 [pscustomobject]@{hive='CurrentUser';key='Software\Microsoft\Installer\Components\011C8458AD3476D4BAD90270814C389C'},
 [pscustomobject]@{hive='LocalMachine';key='Software\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-21-3896111207-1867804362-1971593122-1001\Components\011C8458AD3476D4BAD90270814C389C'}
)
$files=@();$keys=@();$errors=@()
foreach($path in $filePaths){
 try{
  $item=Get-Item -LiteralPath $path -Force -ErrorAction Stop
  $reparse=($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
  $row=[ordered]@{path=$path;present=$true;isDirectory=[bool]$item.PSIsContainer;reparsePoint=$reparse;bytes=$null;sha256=$null}
  if(-not $item.PSIsContainer -and -not $reparse){
   $row.bytes=[long]$item.Length
   if($item.Length -le 4194304){$row.sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant()}
   else{$row.hashNotRead='Over fixed 4MiB observation limit'}
  }
  $files+=([pscustomobject]$row)
 }catch [System.Management.Automation.ItemNotFoundException]{
  $files+=([pscustomobject]@{path=$path;present=$false})
 }catch{$errors+=([pscustomobject]@{kind='FileObservation';path=$path;exceptionType=$_.Exception.GetType().FullName;hresult=$_.Exception.HResult})}
}
foreach($viewName in @('Registry32','Registry64')){
 if($viewName -eq 'Registry64' -and -not [Environment]::Is64BitOperatingSystem){continue}
 foreach($entry in $registryPaths){
  $base=$null;$key=$null
  try{
   $hive=[Enum]::Parse([Microsoft.Win32.RegistryHive],$entry.hive)
   $view=[Enum]::Parse([Microsoft.Win32.RegistryView],$viewName)
   $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($hive,$view)
   $key=$base.OpenSubKey($entry.key,$false)
   $row=[ordered]@{hive=$entry.hive;view=$viewName;key=$entry.key;present=($null -ne $key)}
   if($null -ne $key){$row.valueCount=$key.ValueCount;$row.subkeyCount=$key.SubKeyCount;$row.expectedProductValuePresent=($key.GetValueNames() -ccontains '85206C15D7912AE488955FA9AB70CA83')}
   $keys+=([pscustomobject]$row)
  }catch{$errors+=([pscustomobject]@{kind='RegistryObservation';hive=$entry.hive;view=$viewName;key=$entry.key;exceptionType=$_.Exception.GetType().FullName;hresult=$_.Exception.HResult})}
  finally{if($null -ne $key){$key.Dispose()};if($null -ne $base){$base.Dispose()}}
 }
}
$presentFiles=@($files|Where-Object{$_.present});$presentKeys=@($keys|Where-Object{$_.present})
$status=if($errors.Count -ne 0){'INCOMPLETE_READONLY_OBSERVATION'}elseif($presentFiles.Count -ne 0 -or $presentKeys.Count -ne 0){'EXACT_ADDRESSES_PRESENT_NEEDS_REVIEW'}else{'OBSERVED_EXACT_ADDRESSES_ABSENT'}
[pscustomobject]@{schemaVersion=1;status=$status;scope='Only bounded exact synthetic MSI addresses; no deletion and no universal cleanup claim';actualSid=$actualSid;observedUtc=[DateTime]::UtcNow.ToString('o');files=$files;registry=$keys;errors=$errors;rollbackDiagnostic='Original Error in rollback skipped Return 5 is retained and not erased by this observation';sharedInstallerState='NOT_ENUMERATED';unrecordedEmbeddedTempPaths='NOT_OBSERVABLE_FROM_ORIGINAL_LOGS'}|ConvertTo-Json -Depth 10 -Compress
