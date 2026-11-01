from pathlib import Path
import hashlib
import json
import subprocess

ROOT = Path('/workspace/共享/Eliora/未来教室计划')
TEST_TREE = Path('/tmp/fc-round53-msi-candidate-worktree')
EV = Path(__file__).resolve().parent
OUT = EV / 'finite-remaining-second.json'
assert not OUT.exists()
first = json.loads((EV / 'finite-flows-first.json').read_text())
assert first['status'] == 'FAIL' and first['failedOperation'] == 'modern-msvc-default-browser-continuous-first'
for tag in ('modern', 'win7'):
    runtime = json.loads((EV / (tag + '-msvc-runtime-first.json')).read_text())
    assert runtime['status'] == runtime['cleanup'] == 'PASS'
for name in ('modern-msvc-public-native-first', 'modern-msvc-visual-native-first', 'modern-msvc-https-certificate-native-first'):
    result = json.loads((EV / (name + '.json')).read_text())
    assert result['status'] == result['cleanup'] == 'PASS'
assert hashlib.sha256((TEST_TREE / 'scripts/windows-test-edge-owner.py').read_bytes()).hexdigest() == '8687830a4db9fe5a336949797ff58a0534097569680172ae55cf11c915817009'
commands = []
audit = json.loads((EV / 'win7-original-independent-first.json').read_text())
exe = audit['normalExecutable']['localPath']
assert hashlib.sha256(Path(exe).read_bytes()).hexdigest() == audit['normalExecutable']['producer']['sha256']
base = ['python3', '-B', str(ROOT / 'scripts/windows-probe.py'), '--host', 'elior@192.168.10.212',
        '--identity', '/config/.local/share/future-classroom-test-access/win11-192.168.10.212', '--exe', exe]
for name, flag in [('public-native', '--public-desktop'), ('visual-native', '--subject-visuals')]:
    label = 'win7-msvc-' + name + '-first'
    commands.append((label, base + [flag, '--report', str(EV / (label + '.json'))]))
label = 'win7-msvc-https-certificate-native-first'
commands.append((label, ['python3', '-B', str(EV / 'run-https-certificate-native-first.py'), '--execute',
                        '--host', 'elior@192.168.10.212', '--identity', '/config/.local/share/future-classroom-test-access/win11-192.168.10.212',
                        '--exe', exe, '--report', str(EV / (label + '.json'))]))
audit = json.loads((EV / 'modern-original-independent-first.json').read_text())
exe = audit['normalExecutable']['localPath']
assert hashlib.sha256(Path(exe).read_bytes()).hexdigest() == audit['normalExecutable']['producer']['sha256']
bundle = Path('/mnt/data/FutureClassroom-0.1.0-voice-capability-dev.zip')
assert hashlib.sha256(bundle.read_bytes()).hexdigest() == '5e280a84e726f0366db3e0ea1bd704158e26ac1b3fdb4e542f397b48fc1706c0'
label = 'modern-msvc-default-browser-continuous-second'
commands.append((label, ['python3', '-B', str(TEST_TREE / 'scripts/windows-tls-business-probe.py'),
                        '--ssh-target', 'elior@192.168.10.212', '--identity', '/config/.local/share/future-classroom-test-access/win11-192.168.10.212',
                        '--exe', exe, '--web-bundle', str(bundle), '--continuous-accept', '--browser', '--voice-capabilities', '--report', str(EV / (label + '.json'))]))
label = 'windows-readonly-final-first'
commands.append((label, ['python3', '-B', str(EV / 'audit-windows-readonly.py'), '--report', str(EV / (label + '.json')),
                        '--baseline', str(ROOT / '开发计划/验证记录/2026-11-01-round50-https-integration/windows-readonly-final-first.json')]))
for label, _ in commands:
    assert not any((EV / (label + suffix)).exists() for suffix in ['.json', '.command.json', '.stdout.log', '.stderr.log'])
report = {'status': 'RUNNING', 'scope': 'Remaining never-run Win7-candidate native workflows on actual Win11, plus fresh modern browser after current Microsoft-signed installed Edge baseline was explicitly verified. No repeat of passed native workflows, no Win7/Win10 runtime or strict5s claim.', 'operations': []}
OUT.write_text(json.dumps(report, indent=2) + '\n')
for label, args in commands:
    print('REMAINING_FLOW_STARTED ' + label, flush=True)
    child = subprocess.run(['python3', '-B', str(EV / 'run-recorded.py'), label, *args], cwd=ROOT)
    report['operations'].append({'name': label, 'exitCode': child.returncode})
    if child.returncode:
        report.update(status='FAIL', failedOperation=label)
        OUT.write_text(json.dumps(report, indent=2) + '\n')
        raise SystemExit(child.returncode)
    OUT.write_text(json.dumps(report, indent=2) + '\n')
report['status'] = 'PASS'
OUT.write_text(json.dumps(report, indent=2) + '\n')
print('REMAINING_FLOWS_TERMINAL_PASS', flush=True)
