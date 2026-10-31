from pathlib import Path
import json,hashlib,base64,difflib
r=Path('/tmp/fc-round30-evidence');p=Path('scripts/windows-msvc-ci.ps1');before=p.read_text();after=before
helper='''function Write-CrtFlagState([string]$Phase) {
    if ($Phase -notin @('before-vs', 'after-vs', 'before-cargo')) { throw 'Invalid CRT diagnostic phase.' }
    $flags = [ordered]@{}
    foreach ($name in @('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS')) {
        $present = Test-Path -LiteralPath "Env:\\$name"
        $value = [Environment]::GetEnvironmentVariable($name, 'Process')
        if ($null -ne $value -and $value.Length -gt 4096) { throw 'Compiler flag diagnostic exceeds bound.' }
        $flags[$name] = [ordered]@{
            present = $present; value = $value
            length = $(if ($null -eq $value) { $null } else { $value.Length })
        }
    }
    Write-NewJson (Join-Path $Logs "$Phase-crt-flags.json") ([ordered]@{
        phase = $Phase; powershell = $PSVersionTable.PSVersion.ToString()
        dotnet = [Environment]::Version.ToString(); flags = $flags
        cargo_precedence = @('CARGO_ENCODED_RUSTFLAGS', 'RUSTFLAGS', 'matching target rustflags', 'build.rustflags')
        scope = 'Two exact process variables only; presence is distinct from an empty value. No environment dump.'
    })
}

'''
anchor='function Get-ActiveProcessIdentity(';assert after.count(anchor)==1;after=after.replace(anchor,helper+anchor,1)
vs="    & (Join-Path $vsPath 'Common7\\Tools\\Launch-VsDevShell.ps1') -Arch x86 -HostArch amd64 -SkipAutomaticLocation | Out-Null"
assert after.count(vs)==1;after=after.replace(vs,"    Write-CrtFlagState 'before-vs'\n"+vs+"\n    Write-CrtFlagState 'after-vs'",1)
anchor="    $tests = Invoke-Logged 'workspace-test-compile' $cargo @('test',";assert after.count(anchor)==1;after=after.replace(anchor,"    Write-CrtFlagState 'before-cargo'\n    $tests = Invoke-Logged 'workspace-test-compile' $cargo @('test','-vv',",1)
for label,command in [('product-compile','build'),('clippy','clippy')]:
 anchor=f"Invoke-Logged '{label}' $cargo @('{command}',";assert after.count(anchor)==1;after=after.replace(anchor,f"Invoke-Logged '{label}' $cargo @('{command}','-vv',",1)
# Diagnostic only: preserve current suspect null setter and all gating/cleanup.
assert "[Environment]::SetEnvironmentVariable('CARGO_ENCODED_RUSTFLAGS', $null, 'Process')" in after
assert after.count('Direct dynamic CRT import found')==before.count('Direct dynamic CRT import found')==1
for marker in ["$env:RUSTFLAGS = '-C target-feature=+crt-static'",'if ($payloadBytes -ne $script:PayloadBytes)',"if ($Mode -eq 'Cleanup')"]:assert marker in after
for name,text in [('ci-crt-flag-diagnostics.patch',''.join(difflib.unified_diff(before.splitlines(True),after.splitlines(True),fromfile='a/scripts/windows-msvc-ci.ps1',tofile='b/scripts/windows-msvc-ci.ps1'))),('windows-msvc-ci.crt-diagnostics.ps1',after)]:
 with (r/name).open('x') as f:f.write(text)
report={'status':'PREPARED_DIAGNOSTIC_PATCH_NOT_APPLIED_NOT_EXECUTED','before':{'path':str(p),'bytes':len(before.encode()),'sha256':hashlib.sha256(before.encode()).hexdigest(),'base64':base64.b64encode(before.encode()).decode()},'candidate':{'path':str(r/'windows-msvc-ci.crt-diagnostics.ps1'),'bytes':len(after.encode()),'sha256':hashlib.sha256(after.encode()).hexdigest(),'base64':base64.b64encode(after.encode()).decode()},'patchPath':str(r/'ci-crt-flag-diagnostics.patch'),'evidencePaths':['/tmp/fc-round30-evidence/ci-sixth-import-diagnosis.json','/tmp/fc-round30-evidence/ci-sixth-import-diagnosis-raw/logs/rust-target-cfg.stdout.log'],'observedFacts':['Example, fc-domain and fc-protocol PE members all directly import VCRUNTIME140 and UCRT API sets; selected member CRC/SHA is not a whole-ZIP verification.','Direct rustc --print cfg explicitly passed +crt-static; this does not establish actual Cargo rustc flags.'],'primaryDocumentFinding':'Microsoft PowerShell7.5 documentation explicitly states Environment.SetEnvironmentVariable(name,$null) leaves an empty variable, whereas provider/variable null removes it. Cargo gives an existing CARGO_ENCODED_RUSTFLAGS first precedence, even over RUSTFLAGS. This directly fits the global symptom but current runner exact flags/runtime still require capture.','patchScope':['Only record exact two compiler variables presence/value/length (max4096 chars each) and PS/.NET versions immediately before/after VS plus before Cargo.','Only add Cargo -vv to target test/build/clippy so existing bounded original stderr captures actual compiler/build-script commands.','No flag correction or CRT gate change in this diagnostic-only candidate; original null setter deliberately preserved.','No whole environment dump, dependency/toolchain changes, tests/EXE execution, or change to log bounds/cleanup/source SHA guards.'],'limits':['Candidate not PowerShell-parsed locally; no local pwsh available.','No Cargo/Windows/Git/CI invocation or source edit performed.','Do not predeclare VS overwrote flags; before/after snapshots are needed.'],'primarySources':['https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_environment_variables?view=powershell-7.5','https://learn.microsoft.com/en-us/dotnet/api/system.environment.setenvironmentvariable?view=net-9.0','https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags'],'safeFixIfConfirmed':'Use the PowerShell Env provider to remove the encoded variable (or deliberately set correct encoded tokens), assert intended presence/values after VS, preserve prior exact absence vs empty restoration; retain unchanged PE rejection. This patch deliberately does not apply that fix.'}
with (r/'ci-crt-flag-diagnostics-prepared.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
assert p.read_text()==before
print(json.dumps({'status':report['status'],'beforeSha256':report['before']['sha256'],'candidateSha256':report['candidate']['sha256'],'patch':report['patchPath'],'productionEdited':False}))
