from pathlib import Path
import hashlib, json, subprocess, time

ROOT = Path('/workspace/共享/Eliora/未来教室计划')
E = Path('/tmp/fc-round54-win7-loader-evidence')
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
source = json.loads((E / 'checks-first.json').read_text())
assert source['status'] == 'PASS_COMMANDS_ONLY_PE_AUDIT_PENDING_WINDOWS_NOT_RUN'
assert not source['changedInputs']
for path, facts in source['sourceAfter'].items():
    assert sha(ROOT / path) == facts['sha256']
script = ROOT / 'scripts/windows-probe.py'
raw_metadata = E / 'windows-compile-first.stdout.log'
selected = []
for line in raw_metadata.read_text().splitlines(keepends=True):
    item = json.loads(line)
    if (item.get('reason') == 'compiler-artifact' and item.get('profile', {}).get('test')
            and item.get('executable') and item['target']['name'] == 'fc_windows'):
        selected.append((line, Path(item['executable'])))
assert len(selected) == 1
metadata = E / 'windows-affected-selected-original-first.jsonl'
with metadata.open('x') as stream:
    stream.write(selected[0][0])
normal = Path('/tmp/fc-round54-win7-loader-target/stable/i686-pc-windows-gnu/release/fc-bootstrap.exe')
inputs = {'normal': {'path': str(normal), 'sha256': sha(normal)},
          'test': {'path': str(selected[0][1]), 'sha256': sha(selected[0][1])},
          'selectedOriginalMetadata': {'path': str(metadata), 'sha256': sha(metadata)},
          'fullOriginalMetadata': {'path': str(raw_metadata), 'sha256': sha(raw_metadata)}}
report = E / 'windows-affected-operation-first.json'
assert not report.exists()
record = {'status': 'RUNNING', 'scope': 'Only the affected Windows adapter test executable, then dedicated public/visual flows. Same frozen429 inputs; no repeat of unchanged Host/business tests. Actual OS Win11 only, GNU build does not certify Win7.',
          'inputs': inputs, 'scriptSha256': sha(script), 'operations': []}
def save():
    report.write_text(json.dumps(record, indent=2) + '\n')
save()
for label, flags in [('windows-affected-public-first', ['--test-metadata', str(metadata), '--public-desktop', '--system-host']),
                     ('windows-visual-first', ['--subject-visuals'])]:
    result = E / (label + '.json')
    assert not result.exists()
    args = ['python3', '-B', str(script), '--host', 'elior@192.168.10.212', '--identity',
            '/config/.local/share/future-classroom-test-access/win11-192.168.10.212',
            '--exe', str(normal), '--test-timeout-seconds', '900', '--native-seconds', '1',
            '--report', str(result), *flags]
    entry = {'name': label, 'args': args, 'status': 'RUNNING'}
    record['operations'].append(entry)
    save()
    start = time.monotonic()
    print('WINDOWS_STARTED ' + label, flush=True)
    with (E / (label + '.stdout.log')).open('xb') as out, (E / (label + '.stderr.log')).open('xb') as err:
        child = subprocess.run(args, cwd=ROOT, stdout=out, stderr=err)
    entry.update(exitCode=child.returncode, actualExit=True, elapsedSeconds=time.monotonic() - start)
    for kind in ('stdout', 'stderr'):
        p = E / (label + '.' + kind + '.log')
        entry[kind] = {'path': str(p), 'bytes': p.stat().st_size, 'sha256': sha(p)}
    data = json.loads(result.read_text()) if result.exists() else {}
    entry.update(status='PASS' if child.returncode == 0 and data.get('status') == 'PASS' and data.get('cleanup') == 'PASS' else 'FAIL',
                 resultStatus=data.get('status'), cleanup=data.get('cleanup'))
    save()
    print('WINDOWS_DONE ' + label + ' ' + entry['status'], flush=True)
    if entry['status'] != 'PASS':
        record['status'] = 'FAIL'
        save()
        raise SystemExit(1)
assert sha(script) == record['scriptSha256']
assert sha(normal) == inputs['normal']['sha256'] and sha(selected[0][1]) == inputs['test']['sha256']
record['status'] = 'PASS_ACTUAL_AFFECTED_WIN11_FLOWS'
save()
print(record['status'], flush=True)
