from pathlib import Path
import hashlib, json, runpy, time, uuid

ROOT = Path('/tmp/fc-round61-gui/worktree')
EV = Path(__file__).resolve().parent
EXE = EV.parent / 'target/stable/i686-pc-windows-gnu/release/fc-bootstrap.exe'
SHA = '9095d86eeb7d7c1e4c82dd4a278a6b33fbe71af1454ba14ca7270a24a97797e4'
assert hashlib.sha256(EXE.read_bytes()).hexdigest() == SHA
OUT = EV / 'gui-diagnostics-first.json'
assert not OUT.exists()
helper = runpy.run_path(str(ROOT / 'scripts/windows-probe.py'))
remote = helper['Remote']('elior@192.168.10.212', Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212'))
literal = helper['ps_literal']
nonce = uuid.uuid4().hex
record = dict(status='RUNNING', cleanup='NOT_RUN', exeSha256=SHA, scope='Actual GUI --version and invalid-argument inherited pipe/exit checks only; no normal/autostart/repair launch, MessageBox, installed data or foreground change.', perChildDeadlineMillis=5000)
directory = None
def save(): OUT.write_text(json.dumps(record, ensure_ascii=False, indent=2)+'\n')
save(); started = time.monotonic()
try:
    directory = remote.powershell("$r=Join-Path $env:TEMP " + literal('FutureClassroom-GuiDiagnostics-'+nonce) + ";New-Item -ItemType Directory -Path $r -ErrorAction Stop|Out-Null;Set-Content -LiteralPath (Join-Path $r '.owner') -Value "+literal(nonce)+";$r")
    record['ownedRemoteRoot'] = directory; save()
    remote.upload(EXE, directory)
    root = literal(directory)
    body = r'''
$root=ROOT_LITERAL;
$exe=Join-Path $root 'fc-bootstrap.exe';
if((Get-FileHash -Algorithm SHA256 -LiteralPath $exe).Hash.ToLowerInvariant() -cne 'SHA_LITERAL'){throw 'GUI input SHA mismatch'}
$results=@();
foreach($argument in @('--version','--future-classroom-invalid-launch-probe')){
 $info=New-Object System.Diagnostics.ProcessStartInfo;
 $info.FileName=$exe;$info.Arguments=$argument;$info.WorkingDirectory=$root;
 $info.UseShellExecute=$false;$info.CreateNoWindow=$false;
 $info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;
 $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false);
 $process=New-Object System.Diagnostics.Process;$process.StartInfo=$info;
 $began=$false;$timedOut=$false;
 try{
  $began=$process.Start();if(-not $began){throw 'GUI child did not start'}
  $originalPid=$process.Id;
  if(-not $process.WaitForExit(5000)){$timedOut=$true;$process.Kill();if(-not $process.WaitForExit(5000)){throw 'Original owned diagnostic did not return'}}
  $process.WaitForExit();
  $stdout=$process.StandardOutput.ReadToEnd();$stderr=$process.StandardError.ReadToEnd();
  if($stdout.Length -gt 8192 -or $stderr.Length -gt 8192){throw 'Diagnostic output overflow'}
  $results+=@{argument=$argument;pid=$originalPid;actualExit=$true;exitCode=$process.ExitCode;timedOut=$timedOut;stdout=$stdout;stderr=$stderr;createNoWindow=$false;useShellExecute=$false;redirectedPipes=$true};
 }finally{
  if($began -and -not $process.HasExited){$process.Kill();$null=$process.WaitForExit(5000)}
  if(-not $began -or $process.HasExited){$process.Dispose()}
 }
}
@{operations=$results;exeSha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $exe).Hash.ToLowerInvariant();noConsoleApisCalled=$true}|ConvertTo-Json -Depth 8 -Compress
'''.replace('ROOT_LITERAL', root).replace('SHA_LITERAL', SHA)
    script = EV / 'gui-diagnostics-first.remote.ps1'; script.write_text(body)
    record['remoteScript'] = dict(path=str(script), bytes=script.stat().st_size, sha256=hashlib.sha256(script.read_bytes()).hexdigest())
    result = remote.powershell(body, timeout=30)
    raw = EV / 'gui-diagnostics-first.remote.stdout'; raw.write_text(result+'\n')
    record['observed'] = json.loads(result)
    rows = record['observed']['operations']; assert len(rows) == 2
    assert all(r['actualExit'] and not r['timedOut'] and not r['createNoWindow'] and r['redirectedPipes'] for r in rows)
    assert rows[0]['argument'] == '--version' and rows[0]['exitCode'] == 0 and rows[0]['stdout'].startswith('FutureClassroom 0.1.0 / D11 / development;') and rows[0]['stderr'] == ''
    assert rows[1]['argument'] == '--future-classroom-invalid-launch-probe' and rows[1]['exitCode'] == 1 and rows[1]['stdout'] == '' and rows[1]['stderr'].startswith('FutureClassroom: unknown arguments;')
    assert record['observed']['exeSha256'] == SHA
    record['status'] = 'PASS_ACTUAL_GUI_INHERITED_PIPES_AND_EXIT_CODES'
except Exception as error:
    record.update(status='FAIL', errorKind=type(error).__name__)
finally:
    if directory:
        try:
            result = remote.powershell("$r="+literal(directory)+";if((Get-Content -Raw -LiteralPath (Join-Path $r '.owner')).Trim() -cne "+literal(nonce)+"){throw 'Cleanup owner mismatch'};$unexpected=@(Get-ChildItem -Force -LiteralPath $r|Where-Object{$_.Name -notin @('.owner','fc-bootstrap.exe') -or $_.PSIsContainer});if($unexpected.Count -ne 0){throw 'Unexpected cleanup content'};Remove-Item -Recurse -Force -LiteralPath $r;if(Test-Path -LiteralPath $r){throw 'Owned cleanup did not finish'};'PASS'")
            assert result == 'PASS'; record['cleanup'] = 'PASS'
        except Exception as error:
            record.update(status='FAIL_CLEANUP', cleanup='FAIL', cleanupErrorKind=type(error).__name__)
    record['elapsedSeconds'] = time.monotonic()-started
    record['inputUnchanged'] = hashlib.sha256(EXE.read_bytes()).hexdigest() == SHA
    save()
print(json.dumps({k:record[k] for k in ('status','cleanup','elapsedSeconds','inputUnchanged')}), flush=True)
raise SystemExit(0 if record['status'].startswith('PASS_') and record['cleanup']=='PASS' and record['inputUnchanged'] else 1)
