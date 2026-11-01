from pathlib import Path
import hashlib, json, subprocess, time
E = Path('/tmp/fc-round67-ci-evidence')
ROOT = Path('/workspace/共享/Eliora/未来教室计划')
script = ROOT / 'scripts/windows-probe.py'
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
import sys
selected=sys.argv[1]
assert selected in ('modern','win7')
report = E / (selected+'-public-visual-operation-first.json')
assert not report.exists()
record = {'status': 'RUNNING', 'scope': 'Dedicated public20 and visual22 per original R67 MSVC normal program, actual Win11 hidden controls only. No new TLS/browser/certificate/installer/Win7-OS claim.', 'scriptSha256': sha(script), 'operations': []}
def save():
    report.write_text(json.dumps(record, indent=2) + '\n')
save()
for tag in [selected]:
    audit = json.loads((E / (tag + '-original-independent-first.json')).read_text())
    runtime = json.loads((E / (tag + '-msvc-runtime-first.json')).read_text())
    assert audit['status'] == 'PASS_STATIC_ONLY_RUNTIME_NOT_RUN'
    assert runtime['status'] == 'PASS' and runtime['cleanup'] == 'PASS'
    exe = Path(audit['normalExecutable']['localPath'])
    assert sha(exe) == audit['normalExecutable']['producer']['sha256']
    for kind, flag in [('public', '--public-desktop'), ('visual', '--subject-visuals')]:
        label = tag + '-msvc-' + kind + '-native-first'
        destination = E / (label + '.json')
        assert not destination.exists()
        args = ['python3', '-B', str(script), '--host', 'elior@192.168.10.212', '--identity', '/config/.local/share/future-classroom-test-access/win11-192.168.10.212', '--exe', str(exe), '--native-seconds', '1', flag, '--report', str(destination)]
        row = {'label': label, 'status': 'RUNNING', 'args': args, 'exeSha256': sha(exe)}
        record['operations'].append(row)
        save()
        print('NATIVE_FLOW_STARTED ' + label, flush=True)
        started = time.monotonic()
        with (E / (label + '.stdout.log')).open('xb') as out, (E / (label + '.stderr.log')).open('xb') as err:
            child = subprocess.run(args, cwd=ROOT, stdout=out, stderr=err)
        original = json.loads(destination.read_text()) if destination.exists() else {}
        row.update(exitCode=child.returncode, actualExit=True, elapsedSeconds=time.monotonic() - started, cleanup=original.get('cleanup'), resultStatus=original.get('status'))
        for name in ['stdout', 'stderr']:
            p = E / (label + '.' + name + '.log')
            row[name] = {'path': str(p), 'bytes': p.stat().st_size, 'sha256': sha(p)}
        row['status'] = 'PASS' if row['exitCode'] == 0 and row['cleanup'] == 'PASS' and row['resultStatus'] == 'PASS' else 'FAIL'
        save()
        if row['status'] != 'PASS':
            record['status'] = 'FAIL'
            save()
            raise SystemExit(1)
assert sha(script) == record['scriptSha256']
record['status'] = 'PASS_SELECTED2_ORIGINAL_MSVC_PUBLIC_VISUAL_FLOWS'
save()
print(record['status'], flush=True)
