"""Read-only fresh Windows audit with complete original SSH/PowerShell bytes.

No remote upload/file write, process termination, task/registry/service/browser
mutation or private-key read is permitted. A caller-selected report is created
exclusively; reruns must choose another name. Existing project activity is only
reported and yields REVIEW_REQUIRED, never cleaned by this audit.
"""
from pathlib import Path
import argparse
import base64
from datetime import datetime, timezone
import hashlib
import json
import runpy
import stat
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path.cwd()
EVIDENCE = Path('/tmp/fc-round41-frontdoor-deadline-evidence')
KEY = Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212')
HOST = 'elior@192.168.10.212'
PREAMBLE = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding;"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def raw(data):
    return {'bytes': len(data), 'sha256': sha(data),
            'base64': base64.b64encode(data).decode('ascii'), 'captureScope': 'COMPLETE'}


def powershell_script(run_script):
    # Read only this project's Run value; no raw value or browser command line
    # is disclosed. Get-CimInstance input is filtered before DTO construction.
    return r'''
function Read-ProjectRunSnapshot {
''' + run_script + r'''
}
$runSnapshot = Read-ProjectRunSnapshot | ConvertFrom-Json;
$dirs = @(Get-ChildItem -LiteralPath $env:TEMP -Directory -Force |
 Where-Object {$_.Name -like 'FutureClassroom*'} |
 Sort-Object Name | Select-Object -ExpandProperty Name);
$processes = @(Get-CimInstance Win32_Process |
 Where-Object {
  ($_.Name -like 'FutureClassroom*') -or
  ($_.Name -match '^fc[-_]bootstrap(?:[.-].*)?\.exe$') -or
  ($_.ExecutablePath -and $_.ExecutablePath -match '(?:^|[\\/])FutureClassroom[^\\/]*(?:[\\/]|$)')
 } | Sort-Object ProcessId | ForEach-Object {
  @{name=$_.Name;pid=$_.ProcessId;executablePath=$_.ExecutablePath}
 });
$tasks = @(Get-ScheduledTask | Where-Object {$_.TaskName -like 'FutureClassroom*'} |
 Sort-Object TaskPath,TaskName | ForEach-Object {
  @{name=$_.TaskName;path=$_.TaskPath;state=$_.State.ToString()}
 });
$os = Get-CimInstance Win32_OperatingSystem;
@{
 os=$os.Caption;build=$os.BuildNumber;version=$os.Version;osArchitecture=$os.OSArchitecture;
 sshd=(Get-Service sshd).Status.ToString();
 tempDirectory=$env:TEMP;scratchDirectories=$dirs;ownedProcesses=$processes;ownedTasks=$tasks;
 projectRunReadOnlySnapshot=$runSnapshot;
 sshPublicAuthorizationSha256=(Get-FileHash -LiteralPath "$env:ProgramData\ssh\administrators_authorized_keys" -Algorithm SHA256).Hash.ToLowerInvariant()
} | ConvertTo-Json -Depth 10 -Compress
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, default=EVIDENCE / 'head-windows-final-after.json')
    parser.add_argument('--timeout-seconds', type=int, default=45)
    args = parser.parse_args()
    if args.report.parent.resolve() != EVIDENCE or args.report.suffix != '.json':
        parser.error('report must be a new JSON directly inside the owned evidence directory')
    if args.report.exists():
        parser.error('refusing to overwrite any existing report')
    if not 1 <= args.timeout_seconds <= 60:
        parser.error('read-only SSH timeout must be 1..60 seconds')
    baseline_data = args.baseline.read_bytes()
    baseline = json.loads(baseline_data)
    assert baseline['status'] == 'PASS'
    runner = ROOT / 'scripts/windows-probe.py'
    runner_bytes = runner.read_bytes()
    helper = runpy.run_path(str(runner))
    remote = helper['Remote'](HOST, KEY)
    run_script = helper['project_run_snapshot_script']()
    script = powershell_script(run_script)
    full_script = PREAMBLE + script
    # Windows OpenSSH's command-line limit can reject a large EncodedCommand
    # before PowerShell starts. Keep the actual SSH command short and transmit
    # the complete immutable script as stdin; execute entirely in memory.
    wrapper = "[Console]::InputEncoding=[System.Text.UTF8Encoding]::new($false);$auditSource=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($auditSource));"
    encoded = base64.b64encode(wrapper.encode('utf-16-le')).decode('ascii')
    transmitted = full_script.encode('utf-8')
    command = ['ssh', *remote.options, remote.host,
               'powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + encoded]
    mode = oct(stat.S_IMODE(KEY.stat().st_mode))  # Metadata only; never open/read the key.
    report = {
        'schemaVersion': 1, 'scope': 'Fresh read-only Windows inventory; no runtime/browser/test or cleanup mutation performed.',
        'observedAtUtc': datetime.now(timezone.utc).isoformat(),
        'contract': 'No upload, registry/task/service/trust/file mutation, private-key read, browser interaction or process termination. Project activity is recorded only.',
        'baseline': {'path': str(args.baseline), 'sha256': sha(baseline_data)},
        'auditSource': {'path': str(Path(__file__).resolve()), 'sha256': sha(Path(__file__).read_bytes())},
        'remoteAdapterSource': {'path': str(runner), 'sha256': sha(runner_bytes),
                                'scope': 'Unmodified Remote SSH options and exact preamble; raw subprocess is captured here because Remote.powershell otherwise discards original stderr.'},
        'powershell': {'script': script, 'preamble': PREAMBLE, 'fullSource': full_script,
                       'sourceSha256Utf8': sha(full_script.encode('utf-8')), 'sourceSha256Utf16Le': sha(full_script.encode('utf-16-le')),
                       'encodedCommand': encoded, 'encodedWrapperSource': wrapper,
                       'encodedWrapperSourceSha256Utf16Le': sha(wrapper.encode('utf-16-le')),
                       'stdin': raw(transmitted), 'command': command, 'timeoutSeconds': args.timeout_seconds,
                       'originalRunSnapshotSourceSha256': sha(run_script.encode('utf-8'))},
        'persistentKeyMetadata': {'path': str(KEY), 'mode': mode, 'contentsRead': False},
        'persistentKeyMode': mode,
    }
    status = 'FAIL'
    # Reserve a NEW local report before any remote action, so concurrent/rerun
    # audits cannot replace a prior observation.
    with args.report.open('x') as output:
        try:
            completed = subprocess.run(command, input=transmitted, capture_output=True, timeout=args.timeout_seconds)
            report['ssh'] = {'exitCode': completed.returncode, 'stdout': raw(completed.stdout), 'stderr': raw(completed.stderr)}
            if completed.returncode != 0:
                raise RuntimeError('read-only SSH command returned a nonzero exit code; complete original bytes retained')
            observed = json.loads(completed.stdout.decode('utf-8-sig'))
            report.update(observed)
            report['authentication'] = 'PASS'
            report['publicAuthorizationUnchanged'] = observed['sshPublicAuthorizationSha256'] == baseline['sshPublicAuthorizationSha256']
            report['projectRunUnchanged'] = observed['projectRunReadOnlySnapshot'] == baseline['projectRunReadOnlySnapshot']
            expected_views = {'Registry32', 'Registry64'}
            views = observed['projectRunReadOnlySnapshot']['views']
            assert len(views) == 2 and {v['view'] for v in views} == expected_views
            assert all(set(v) == {'view', 'keyPresent', 'valuePresent', 'kind', 'encodedBytes', 'sha256'} for v in views)
            invariants = (observed['sshd'] == 'Running' and mode == '0o600'
                          and report['publicAuthorizationUnchanged'] and report['projectRunUnchanged'])
            report['integrityStatus'] = 'PASS' if invariants else 'FAIL'
            report['activityObserved'] = bool(observed['scratchDirectories'] or observed['ownedProcesses'] or observed['ownedTasks'])
            report['activityHandling'] = 'Recorded only; nothing stopped, changed or deleted.'
            status = 'FAIL' if not invariants else 'REVIEW_REQUIRED' if report['activityObserved'] else 'PASS'
        except subprocess.TimeoutExpired as error:
            report['ssh'] = {'exitCode': None, 'timeout': True,
                             'stdout': {**raw(error.stdout or b''), 'captureScope': 'PARTIAL_TIMEOUT'},
                             'stderr': {**raw(error.stderr or b''), 'captureScope': 'PARTIAL_TIMEOUT'}}
            report['error'] = {'kind': type(error).__name__, 'message': 'Read-only SSH timeout; captured original bytes retained.'}
        except Exception as error:
            report['error'] = {'kind': type(error).__name__, 'message': str(error)}
        report['status'] = status
        report['completedAtUtc'] = datetime.now(timezone.utc).isoformat()
        output.write(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'status': status, 'report': str(args.report), 'reportSha256': sha(args.report.read_bytes()),
                      'authentication': report.get('authentication'), 'sshd': report.get('sshd'), 'build': report.get('build'),
                      'persistentKeyMode': mode, 'projectRunUnchanged': report.get('projectRunUnchanged'),
                      'publicAuthorizationUnchanged': report.get('publicAuthorizationUnchanged'),
                      'scratchDirectories': report.get('scratchDirectories'), 'ownedProcesses': report.get('ownedProcesses'),
                      'ownedTasks': report.get('ownedTasks'), 'error': report.get('error'),
                      'mutations': 'NONE'}, ensure_ascii=False, indent=2))
    return 0 if status == 'PASS' else 2 if status == 'REVIEW_REQUIRED' else 1


if __name__ == '__main__':
    raise SystemExit(main())
