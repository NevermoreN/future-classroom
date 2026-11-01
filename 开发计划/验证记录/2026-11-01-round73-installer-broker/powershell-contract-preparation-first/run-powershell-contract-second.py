"""Root-only R73 Windows pure PowerShell contract controller; default prepare.
No CI entrypoint/compiler/product execution. SSH/SCP transport never reads key
contents. Original total budget120s includes finally cleanup, without renewal.
"""
from pathlib import Path
import argparse, base64, datetime, hashlib, json, re, subprocess, time, uuid
ROOT = Path('/tmp/fc-round73-installer-broker')
EV = Path(__file__).resolve().parent
WORK = ROOT / 'worktree'
HOST = 'elior@192.168.10.212'
IDENTITY = '/config/.local/share/future-classroom-test-access/win11-192.168.10.212'
FILES = {
 'check-msvc-gui-contract.ps1': 'cc6f4600666604824049d5bfc3d3ebb9da288452154e6cead300f1417b10cc50',
 'windows-msvc-ci.ps1': 'edec879ecad9bad5b3ef21f48afa895bb3e8f0ac58e6aba561d5748e5e147802'}
PREAMBLE = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding;"
class UnknownRemoteReturn(Exception): pass
def need(ok, code):
    if not ok: raise AssertionError(code)
def sha(data): return hashlib.sha256(data).hexdigest()
def fact(path):
    path = Path(path); need(path.is_file() and not path.is_symlink(), 'REGULAR_SOURCE_REQUIRED')
    data = path.read_bytes(); return dict(path=str(path), bytes=len(data), sha256=sha(data))
def lit(value): return "'" + str(value).replace("'", "''") + "'"
def encode(script):
    value = base64.b64encode((PREAMBLE + script).encode('utf-16-le')).decode('ascii')
    need(len(value) <= 7000, 'SHORT_ENCODED_COMMAND_BOUND')
    return value
def common(leaf, marker):
    need(re.fullmatch(r'FutureClassroomR73CiContract-[0-9a-f]{32}', leaf), 'EXACT_OPAQUE_LEAF')
    return "$leaf=" + lit(leaf) + ";$marker=" + lit(marker) + ";$root=Join-Path ([IO.Path]::GetTempPath()) $leaf;"
def guard(leaf, marker, files=True):
    script = common(leaf, marker) + "$d=Get-Item -LiteralPath $root -Force -ErrorAction Stop;if(-not $d.PSIsContainer -or ($d.Attributes -band 1024)){throw 'root-kind'};$o=Join-Path $root '.owner';if([IO.File]::ReadAllText($o,[Text.Encoding]::ASCII) -cne $marker){throw 'owner'};"
    script += "$names=@('.owner','check-msvc-gui-contract.ps1','windows-msvc-ci.ps1');foreach($f in @(Get-ChildItem -LiteralPath $root -Force)){if($f.PSIsContainer -or ($f.Attributes -band 1024) -or $f.Name -cnotin $names){throw 'member-kind'}};"
    if files:
        for name, digest in FILES.items():
            script += "if((Get-FileHash -LiteralPath (Join-Path $root " + lit(name) + ") -Algorithm SHA256).Hash.ToLowerInvariant() -cne " + lit(digest) + "){throw 'source-sha'};"
    return script
def creation(leaf, marker):
    return common(leaf, marker) + "if(Test-Path -LiteralPath $root){throw 'preexisting-root'};$null=New-Item -ItemType Directory -Path $root;[IO.File]::WriteAllBytes((Join-Path $root '.owner'),[Text.Encoding]::ASCII.GetBytes($marker));[ordered]@{status='CREATED';root=$root;marker=$marker}|ConvertTo-Json -Compress"
def discovery(leaf, marker):
    return guard(leaf, marker) + "$apps=@();foreach($c in @(Get-Command pwsh.exe -CommandType Application -All -ErrorAction SilentlyContinue)){if($c.Source -and $c.Source -notin @($apps|ForEach-Object{$_.path})){$v=[Diagnostics.FileVersionInfo]::GetVersionInfo($c.Source);$apps+=@{path=$c.Source;fileVersion=$v.FileVersion;productVersion=$v.ProductVersion}}};[ordered]@{status='HASHED';powershell=$PSVersionTable.PSVersion.ToString();edition=$PSVersionTable.PSEdition;pwsh=@($apps)}|ConvertTo-Json -Depth 5 -Compress"
def run_contract(leaf, marker):
    script = guard(leaf, marker)
    script += "[Console]::WriteLine('FC_R73_PS_START '+(@{pid=$PID;creationUtc=(Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o');powershell=$PSVersionTable.PSVersion.ToString();root=$root}|ConvertTo-Json -Compress));[Console]::Out.Flush();"
    script += "$lines=[Collections.Generic.List[string]]::new();$failed=$false;try{$text=[IO.File]::ReadAllText((Join-Path $root 'windows-msvc-ci.ps1'),[Text.UTF8Encoding]::new($false,$true));& (Join-Path $root 'check-msvc-gui-contract.ps1') -TextSource $text -ExpectedSourceSha256 " + lit(FILES['windows-msvc-ci.ps1']) + " | ForEach-Object{$lines.Add([string]$_)}}catch{$failed=$true;[Console]::Error.WriteLine(($_|Out-String))};"
    script += "$json=[string]::Join([char]10,$lines.ToArray());$bytes=[Text.Encoding]::UTF8.GetBytes($json);[Console]::WriteLine('FC_R73_PS_RESULT '+([ordered]@{status=$(if($failed){'FAIL'}else{'RETURNED'});powershell=$PSVersionTable.PSVersion.ToString();edition=$PSVersionTable.PSEdition;pid=$PID;contractUtf8Bytes=$bytes.Length;contractBase64=[Convert]::ToBase64String($bytes);capture='Exact checker pipeline JSON string encoded UTF8; SSH raw streams retained independently.'}|ConvertTo-Json -Compress));if($failed){exit 1}"
    return script
def cleanup(leaf, marker):
    return common(leaf, marker) + "if(-not(Test-Path -LiteralPath $root)){@{status='ABSENT';root=$root}|ConvertTo-Json -Compress;exit 0};" + guard(leaf, marker, False) + "Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction Stop;if(Test-Path -LiteralPath $root){throw 'cleanup-retained'};@{status='REMOVED';root=$root;marker=$marker}|ConvertTo-Json -Compress"
def packet(data, prefix=''):
    lines = data.decode('utf-8-sig').splitlines()
    if prefix:
        selected = [s[len(prefix):] for s in lines if s.startswith(prefix)]
        need(len(selected) == 1, 'UNIQUE_REMOTE_JSON_PACKET'); return json.loads(selected[0])
    return json.loads('\n'.join(lines).strip())
def main():
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('--execute', action='store_true')
    parser.add_argument('--report', type=Path, default=EV / 'windows-powershell-contract-second.json'); args = parser.parse_args()
    need((ROOT / '.owner').read_bytes() == b'future-classroom-owned-r73-installer-broker/v1\n', 'EXACT_R73_OWNER')
    before = {name: fact(WORK / 'scripts' / name) for name in FILES}
    need(all(before[name]['sha256'] == FILES[name] for name in FILES), 'EXACT_B_FROZEN_SOURCES')
    # Build and bound every PS command without executing it during preparation.
    token = uuid.uuid4().hex; leaf = 'FutureClassroomR73CiContract-' + token; marker = 'future-classroom-r73-ci-contract/v1|' + token
    scripts = {'create':creation(leaf, marker), 'discovery':discovery(leaf, marker), 'contract':run_contract(leaf, marker), 'cleanup':cleanup(leaf, marker)}
    lengths = {name: len(encode(text)) for name, text in scripts.items()}
    if not args.execute:
        print(json.dumps(dict(status='PREPARED_ONLY_NOT_EXECUTED', inputs=before, budgetSeconds=120, encodedCharacters=lengths,
            scope='Windows pure PS contract only. No SSH/SCP/Windows/CI/compiler/PE invocation or private-key content access during preparation.'))); return 0
    need(args.report.parent.resolve() == EV and not args.report.exists(), 'FRESH_REPORT_REQUIRED')
    local = EV / ('run-' + token); local.mkdir(); (local / '.owner').write_text(marker)
    report = dict(status='RUNNING', startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), budgetSeconds=120,
        host=HOST, inputs=before, leaf=leaf, marker=marker, helper=fact(Path(__file__)), encodedCharacters=lengths,
        operations=[], versions=[], cleanup='NOT_RUN', scope='Actual authorized Windows pure PowerShell argument/AST/synthetic PE-header contract only; no CI entrypoint, compiler, linker, actual PE or runtime proof. Desktop PowerShell5.1 is not hosted PowerShell7.2 evidence.')
    args.report.open('x').write(json.dumps(report, indent=2) + '\n')
    options = ['-i', IDENTITY, '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=8']
    began = time.monotonic(); deadline = began + 120; unknown = False
    def call(label, command, reserve=12, is_contract=False):
        nonlocal unknown
        remaining = deadline - time.monotonic() - reserve; need(remaining > 0, 'ORIGINAL120_BUDGET_EXHAUSTED')
        started = time.monotonic(); code = None; timed_out = False
        try:
            completed = subprocess.run(command, capture_output=True, timeout=remaining)
            code, out, err = completed.returncode, completed.stdout, completed.stderr
        except subprocess.TimeoutExpired as error:
            timed_out = True; out, err = error.stdout or b'', error.stderr or b''
            if is_contract: unknown = True
        paths = {}
        for name, data in [('stdout', out), ('stderr', err)]:
            p = local / (label + '.' + name + '.log'); p.open('xb').write(data); paths[name] = dict(**fact(p), base64=base64.b64encode(data).decode())
        entry = dict(label=label, command=command, exitCode=code, actualLocalParentReturned=not timed_out,
            timedOut=timed_out, elapsedSeconds=time.monotonic() - started, **paths)
        report['operations'].append(entry)
        if code == 255 and is_contract: unknown = True
        if timed_out: raise UnknownRemoteReturn(label)
        return code, out
    def ps(label, text, executable='powershell.exe', reserve=12, is_contract=False):
        need('"' not in executable and '\n' not in executable and '\r' not in executable, 'CLOSED_INTERPRETER_PATH')
        command = ('"' + executable + '"') if ' ' in executable else executable
        if executable.lower() == 'powershell.exe': command += ' -ExecutionPolicy Bypass'
        command += ' -NoProfile -NonInteractive -EncodedCommand ' + encode(text)
        need(len(command) <= 7600, 'SHORT_COMMAND_LINE_BOUND')
        return call(label, ['ssh', *options, HOST, command], reserve, is_contract)
    try:
        code, raw = ps('01-create', scripts['create']); need(code == 0, 'CREATE_PARENT_EXIT0')
        created = packet(raw); need(created['status'] == 'CREATED' and created['marker'] == marker, 'EXACT_CREATED_OWNER')
        directory = created['root']; need(directory.endswith('\\' + leaf) and re.match(r'^[A-Za-z]:\\', directory), 'EXACT_REMOTE_TEMP_ROOT')
        report['remoteRoot'] = directory
        for number, name in enumerate(FILES, 2):
            target = HOST + ':' + directory.replace('\\', '/') + '/' + name
            code, _ = call(f'{number:02d}-scp', ['scp', '-q', *options, str(WORK / 'scripts' / name), target]); need(code == 0, 'SCP_PARENT_EXIT0')
        code, raw = ps('04-discovery', scripts['discovery']); need(code == 0, 'HASH_DISCOVERY_PARENT_EXIT0')
        discovered = packet(raw); need(discovered['status'] == 'HASHED' and discovered['powershell'].startswith('5.1.'), 'ACTUAL_DESKTOP51_HOST')
        report['discovery'] = discovered
        interpreters = [('desktop51', 'powershell.exe')]
        for number, entry in enumerate(discovered['pwsh']):
            path = entry['path']; need(re.fullmatch(r'[A-Za-z]:\\[^\r\n\x00"]+\\pwsh\.exe', path, re.I) is not None, 'DISCOVERED_APPLICATION_PWSH_PATH')
            interpreters.append(('pwsh' + str(number), path))
        expected_cases = re.findall(r"^\s*Check '([^']+)'", (WORK / 'scripts/check-msvc-gui-contract.ps1').read_text(), re.M)
        for label, executable in interpreters:
            code, raw = ps('05-contract-' + label, scripts['contract'], executable, is_contract=True)
            returned = packet(raw, 'FC_R73_PS_RESULT '); data = base64.b64decode(returned['contractBase64'], validate=True)
            need(len(data) == returned['contractUtf8Bytes'], 'ACTUAL_CONTRACT_JSON_BYTES')
            contract = json.loads(data); version = dict(interpreter=executable, actualVersion=returned['powershell'], edition=returned['edition'],
                sshExitCode=code, actualContract=dict(bytes=len(data), sha256=sha(data), base64=returned['contractBase64']),
                ciHostedPowerShell72Proof=False, capture=returned['capture'])
            report['versions'].append(version)
            need(code == 0 and returned['status'] == 'RETURNED' and contract['status'] == 'PASS_PURE_POWERSHELL_GUI_CONTRACT_ONLY', 'ACTUAL_PURE_CONTRACT_SUCCESS')
            need(contract['sourceSha256'] == contract['sourceExpectedSha256'] == FILES['windows-msvc-ci.ps1'] and contract['fullSourceParseErrors'] == 0 and contract['fatal'] is None, 'EXACT_FULL_SOURCE_CONTRACT')
            need([r['name'] for r in contract['checks']] == expected_cases and all(r['status'] == 'PASS' and r['assertions'] > 0 for r in contract['checks']), 'COMPLETE_ACTUAL_CHECKS')
            need(contract['powershell'] == returned['powershell'] and all(contract[k] is False for k in ('ciEntrypointExecuted', 'filesystemOrEnvironmentWritten', 'processOrThreadStarted')) and all(contract[k] == 'NOT_RUN' for k in ('windowsRuntime', 'actualProductPe', 'actualRedirectedDiagnostic')), 'PURE_SCOPE_REMAINS_EXPLICIT')
        code, _ = ps('06-post-source-hashes', scripts['discovery']); need(code == 0, 'POST_REMOTE_SOURCE_HASHES')
        report['checksCompleted'] = True
    except Exception as error:
        report['failure'] = dict(kind=type(error).__name__, code=str(error) if isinstance(error, AssertionError) else 'TRANSPORT_OR_CONTRACT_FAILURE')
    finally:
        if unknown:
            report['cleanup'] = 'ROOT_REVIEW_REQUIRED_REMOTE_RETURN_UNKNOWN'
            report['rootRecovery'] = dict(scope='Root sole Windows owner must inspect exact marker and any PID/creation packet in retained stdout before using this bounded closed-member cleanup. No process termination is authorized by this controller.',
                command=['ssh', *options, HOST, 'powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + encode(scripts['cleanup'])])
        else:
            try:
                code, raw = ps('99-finally-cleanup', scripts['cleanup'], reserve=0)
                result = packet(raw); need(code == 0 and result['status'] in ('REMOVED', 'ABSENT'), 'EXACT_FINALLY_CLEANUP')
                report['cleanup'] = 'PASS'; report['cleanupResult'] = result
            except Exception as error:
                report['cleanup'] = 'FAIL_ROOT_OWNER_REVIEW_REQUIRED'; report['cleanupErrorKind'] = type(error).__name__
                report['rootRecovery'] = dict(command=['ssh', *options, HOST, 'powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + encode(scripts['cleanup'])], scope='Root must review exact owner and retained failure; closed-member cleanup only, no process termination.')
        after = {name: fact(WORK / 'scripts' / name) for name in FILES}; report['sourceUnchanged'] = before == after
        report.update(completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), elapsedSeconds=time.monotonic() - began,
            status='PASS_ACTUAL_WINDOWS_PURE_POWERSHELL_CONTRACT_ONLY' if report.get('checksCompleted') and not report.get('failure') and report['cleanup'] == 'PASS' and before == after and time.monotonic() <= deadline else 'FAIL_WINDOWS_PURE_POWERSHELL_CONTRACT_OR_CLEANUP')
        args.report.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(status=report['status'], cleanup=report['cleanup'], report=str(args.report), reportSha256=fact(args.report)['sha256'])))
    return 0 if report['status'].startswith('PASS_') else 1
if __name__ == '__main__': raise SystemExit(main())
