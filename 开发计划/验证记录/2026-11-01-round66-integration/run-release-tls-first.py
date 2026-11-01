from pathlib import Path
import datetime, hashlib, json, subprocess, sys, time

e = Path('/tmp/fc-round66-integration/evidence')
main = Path('/workspace/共享/Eliora/未来教室计划')
w = Path('/tmp/fc-round63-installer-owner/worktree')
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
freeze = e / 'implementation-freeze-first.json'
f = json.loads(freeze.read_text())
prior = json.loads((e / 'windows-release-operation-first.json').read_text())
assert prior['status'] == 'PASS_COMMAND' and prior['cleanup'] == 'PASS'
exe = Path(prior['normal']['path'])
assert sha(exe) == prior['normal']['sha256']
bundle = Path('/mnt/data/FutureClassroom-0.1.0-voice-capability-dev.zip')
assert sha(bundle) == '5e280a84e726f0366db3e0ea1bd704158e26ac1b3fdb4e542f397b48fc1706c0'
script = main / 'scripts/windows-tls-business-probe.py'
record = e / 'windows-release-tls-series-first.json'
assert not record.exists()
r = {'status': 'RUNNING', 'createdUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
     'sourceFreezeSha256': sha(freeze), 'normal': prior['normal'], 'scriptSha256': sha(script),
     'webBundleSha256': sha(bundle), 'operations': [],
     'scope': 'Finite synthetic loopback TLS only; browser isolated; header samples retain original numeric 5e9ns threshold. No production listener, physical audio or arbitrary OS hard realtime claim.'}
def source_same():
    return all(sha(root / row['path']) == row['sha256']
               for row in f['allRustCargoInputs'] for root in (main, w))
def save():
    record.write_text(json.dumps(r, indent=2) + '\n')
assert source_same()
save()
for kind, flags in [('business-continuous-browser', ['--continuous-accept', '--browser', '--voice-capabilities']),
                    ('header-deadline', ['--header-deadline-only'])]:
    name = 'windows-release-' + kind + '-first'
    out = e / (name + '.json')
    assert not out.exists() and source_same()
    cmd = ['python3', '-B', str(script), '--exe', str(exe), '--web-bundle', str(bundle),
           '--report', str(out), *flags]
    row = {'kind': kind, 'status': 'RUNNING', 'command': cmd}
    r['operations'].append(row)
    save()
    print('TLS_FLOW_STARTED ' + kind, flush=True)
    t = time.monotonic()
    with (e / (name + '.stdout.log')).open('xb') as a, (e / (name + '.stderr.log')).open('xb') as b:
        code = subprocess.run(cmd, cwd=main, stdout=a, stderr=b).returncode
    d = json.loads(out.read_text()) if out.exists() else {}
    row.update(exitCode=code, elapsedSeconds=time.monotonic() - t, resultStatus=d.get('status'),
               cleanup=d.get('cleanup'), sourceUnchanged=source_same())
    row['status'] = 'PASS' if code == 0 and d.get('status') == 'PASS' and d.get('cleanup') == 'PASS' else 'FAIL'
    for stream in ('stdout', 'stderr'):
        p = e / (name + '.' + stream + '.log')
        row[stream] = {'bytes': p.stat().st_size, 'sha256': sha(p)}
    save()
    if row['cleanup'] != 'PASS' or not row['sourceUnchanged']:
        r['status'] = 'FAIL_CLEANUP_OR_SOURCE'
        save()
        sys.exit(1)
r['status'] = 'PASS' if all(x['status'] == 'PASS' for x in r['operations']) else 'FAIL_FINITE_FLOW'
save()
print(json.dumps({'status': r['status'], 'operations': [{k: x[k] for k in ('kind', 'status', 'cleanup', 'elapsedSeconds')} for x in r['operations']]}), flush=True)
sys.exit(0 if r['status'] == 'PASS' else 1)
