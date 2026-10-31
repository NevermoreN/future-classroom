from pathlib import Path
import hashlib,json,base64,difflib,re,subprocess
r=Path('/tmp/fc-round30-evidence');p=Path('scripts/windows-msvc-ci.ps1');raw=p.read_bytes();source=raw.decode();before=json.loads((r/'ci-crt-env-fix-before-source.json').read_text());old=base64.b64decode(before['base64']).decode()
restore=re.search(r'(?ms)^function Restore-ProcessEnvironment\([^\n]*\) \{\n.*?^\}\n',source).group()
probe=r'''# Full-source AST only, then exact restore helper on unique process-only variables.
# No CI execution, external command/process launch, file writes, or persistent environment changes.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$source = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__SOURCE__'))
$tokens = $null; $errors = $null
$null = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
if (@($errors).Count -ne 0) { throw 'Full fixed CI script AST failed.' }
__RESTORE__
$prefix = 'FC_CRT_ENV_PROBE_' + [Guid]::NewGuid().ToString('N')
$absentName = $prefix + '_Absent'; $valueName = $prefix + '_Value'; $emptyName = $prefix + '_Empty'
$names = @($absentName, $valueName, $emptyName)
foreach ($name in $names) { if (Test-Path -LiteralPath "Env:\$name") { throw 'Unique process fixture unexpectedly exists.' } }
$cases = [Collections.Generic.List[object]]::new()
$emptySupported = $false
try {
    Set-Item -LiteralPath "Env:\$absentName" -Value 'temporary'
    Set-Item -LiteralPath "Env:\$valueName" -Value 'changed'
    $saved = @{}
    $saved[$absentName] = @{present=$false; value=$null}
    $saved[$valueName] = @{present=$true; value='original'}
    Restore-ProcessEnvironment $saved
    if (Test-Path -LiteralPath "Env:\$absentName") { throw 'Original absent variable was left present.' }
    if ([Environment]::GetEnvironmentVariable($valueName, 'Process') -cne 'original') { throw 'Original value not restored.' }
    $cases.Add([ordered]@{name='original-absence-restored-through-provider-remove'; passed=$true})
    $cases.Add([ordered]@{name='original-value-restored-through-provider-set'; passed=$true})
    Set-Item -LiteralPath "Env:\$absentName" -Value 'temporary'
    Set-Item -LiteralPath "Env:\$absentName" -Value $null
    if (Test-Path -LiteralPath "Env:\$absentName") { throw 'Provider null failed to delete process fixture.' }
    $cases.Add([ordered]@{name='provider-null-actually-removes'; passed=$true})
    Set-Item -LiteralPath "Env:\$emptyName" -Value ''
    $emptySupported = Test-Path -LiteralPath "Env:\$emptyName"
    if ($emptySupported) {
        Set-Item -LiteralPath "Env:\$emptyName" -Value 'changed'
        $emptySaved = @{}
        $emptySaved[$emptyName] = @{present=$true; value=''}
        Restore-ProcessEnvironment $emptySaved
        if (-not (Test-Path -LiteralPath "Env:\$emptyName") -or
            [Environment]::GetEnvironmentVariable($emptyName, 'Process') -cne '') { throw 'Original empty value not preserved.' }
        $cases.Add([ordered]@{name='original-present-empty-restored'; passed=$true})
    }
} finally {
    foreach ($name in $names) { Remove-Item -LiteralPath "Env:\$name" -ErrorAction SilentlyContinue }
}
foreach ($name in $names) { if (Test-Path -LiteralPath "Env:\$name") { throw 'Unique process fixture not cleaned.' } }
[ordered]@{
    status='PASS'; sourceSha256='__SHA__'; parserErrors=@($errors).Count
    powershell=$PSVersionTable.PSVersion.ToString(); dotnet=[Environment]::Version.ToString()
    caseCount=$cases.Count; cases=$cases.ToArray(); presentEmptySupported=$emptySupported
    emptyRestoration=$(if ($emptySupported) {'PASS'} else {'NOT_SUPPORTED_BY_THIS_RUNTIME_REQUIRE_POWERSHELL_7_5_CI'})
    scope='EXACT_HELPER_ONLY_UNIQUE_PROCESS_ENVIRONMENT_RESTORED_NO_EXE_OR_FILES_OR_CI'
} | ConvertTo-Json -Depth 8 -Compress
'''.replace('__SOURCE__',base64.b64encode(raw).decode()).replace('__RESTORE__',restore).replace('__SHA__',hashlib.sha256(raw).hexdigest())
probe_path=r/'ci-crt-env-fix-ast-restore-probe.ps1'
with probe_path.open('x') as f:f.write(probe)
# Minimal scope assertions: transport/deadline/log bounds/PE gate and source identity remain byte-identical.
for start,end in [('function Get-ActiveProcessIdentity(', 'function Require-Step('),('function Copy-Executable(', '$savedEnvironment = @{}')]:
 def section(t):return t[t.index(start):t.index(end,t.index(start))]
 assert section(old)==section(source)
assert "[Environment]::SetEnvironmentVariable('CARGO_ENCODED_RUSTFLAGS', $null" not in source
assert '$env:CARGO_ENCODED_RUSTFLAGS = $null' in source
assert source.count("$cargo @('test','-vv',")==1 and source.count("$cargo @('build','-vv',")==1 and source.count("$cargo @('clippy','-vv',")==1
assert '$LogLimit = 16MB' in source
assert old[old.index('    $number = 0\n'):old.index('} catch {\n    $failure =')]==source[source.index('    $number = 0\n'):source.index('} catch {\n    $failure =')]
# Pure local model: presence, not truthiness, gives encoded precedence.
def effective(env):
 for name in ['CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS']:
  if name in env:return env[name]
 return None
assert effective({'CARGO_ENCODED_RUSTFLAGS':'','RUSTFLAGS':'-C target-feature=+crt-static'})==''
assert effective({'RUSTFLAGS':'-C target-feature=+crt-static'})=='-C target-feature=+crt-static'
env={'absent':'changed','value':'changed','empty':'changed'}
for n,meta in {'absent':{'present':False,'value':None},'value':{'present':True,'value':'original'},'empty':{'present':True,'value':''}}.items():
 if meta['present']:env[n]=meta['value']
 else:env.pop(n,None)
assert env=={'value':'original','empty':''}
result=subprocess.run(['git','diff','--check','--','scripts/windows-msvc-ci.ps1'],capture_output=True);assert result.returncode==0
report={'status':'SOURCE_FIXED_FROZEN_STATIC_ONLY_PENDING_POWERSHELL_AND_CI','before':before,'after':{'path':str(p),'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'base64':base64.b64encode(raw).decode()},'probe':{'path':str(probe_path),'bytes':len(probe.encode()),'sha256':hashlib.sha256(probe.encode()).hexdigest()},'checks':{'localPythonPrecedenceAndRestorationModel':'PASS_NOT_POWERSHELL_EXECUTION','processIdentityInvokeLoggedCopyExecutableAndPeGateUnchanged':True,'sourceShaGuardLog16MiBAndPhysicalCleanupPreserved':True,'cargoActualCommandsVerbose':True,'boundedFlagsOnlyNoEnvironmentDump':True,'diffCheckExit':result.returncode},'runtimeEvidence':'No local pwsh available; no AST/runtime, Cargo, Windows, Git write or CI dispatch executed. Parent will validate exact probe via stdin and new hosted CI.','documentedCause':'PowerShell7.5 method binding passes null setter as empty, leaving CARGO_ENCODED_RUSTFLAGS present; Cargo encoded precedence masks RUSTFLAGS. Official documentation directly describes this behavior; new CI must verify concrete runtime effect.','primarySources':['https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_environment_variables?view=powershell-7.5','https://learn.microsoft.com/en-us/dotnet/api/system.environment.setenvironmentvariable?view=net-9.0','https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags'],'diff':''.join(difflib.unified_diff(old.splitlines(True),source.splitlines(True),fromfile='before/windows-msvc-ci.ps1',tofile='after/windows-msvc-ci.ps1'))}
with (r/'ci-crt-env-fix-frozen-static.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps({'status':report['status'],'sourceSha256':report['after']['sha256'],'sourceBytes':len(raw),'probe':report['probe']}))
