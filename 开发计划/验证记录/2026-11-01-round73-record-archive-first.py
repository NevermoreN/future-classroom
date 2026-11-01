"""Record actual finite archive subprocess completion outside frozen evidence."""
from pathlib import Path
import datetime, hashlib, json, signal, subprocess, time
C = Path('/tmp/fc-round73-archive-controller')
E = Path('/tmp/fc-round73-installer-broker/evidence')
assert (C / '.owner').read_bytes() == b'future-classroom-owned-r73-archive-controller/v1\n'
helper = E / 'archive-preparation-first/archive-evidence-second.py'
assert hashlib.sha256(helper.read_bytes()).hexdigest() == 'c29f93178ddca078a0335234ea6c38e001b98027f7d76a6e68e118ddbba1740b'
host = json.loads((E / 'n-host-first.json').read_bytes())
assert host['status'] == 'PASS_HOST_TESTS_AND_CLIPPY' and host['sourceUnchanged']
assert host['scratchCleanup'].startswith('PASS')
assert all(s['actualExit'] and s['exitCode'] == 0 for s in host['steps'])
assert (E / 'n-host-producer-terminal-first.json').is_file()
command = ['python3', '-B', str(helper), '--source-base-git', '00120a026eea5dc4ff330f8526bf31169b373565',
    '--acceptance-report', 'independent-finite-acceptance-first.json',
    '--expected-acceptance-sha256', '8d2aaadc96212736aec0318a2b99b9de06570191f7206fde6d620562e23bb96a',
    '--expected-acceptance-status', 'PASS_R73_FINITE_GNU_WIN11_AND_PURE_PS_ONLY',
    '--expected-readonly-sha256', 'eb279c7fd15815a4a5f60b34472114abab983d62ea5ec5678cef01fd7e6d8387',
    '--root-controllers-terminal', '--execute']
report = C / 'archive-operation-first.json'
assert not report.exists()
v = dict(status='RUNNING', command=command, originalBudgetSeconds=300,
    startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    scope='Exact R73 frozen-evidence archive only; no new product/runtime acceptance.')
def save(): report.write_text(json.dumps(v, indent=2) + '\n')
save()
start = time.monotonic()
with (C / 'archive-operation-first.stdout.log').open('xb') as out, (C / 'archive-operation-first.stderr.log').open('xb') as err:
    p = subprocess.Popen(command, stdout=out, stderr=err, start_new_session=True)
    v['pid'] = p.pid; save()
    try:
        code = p.wait(timeout=300)
        v['timedOut'] = False
    except subprocess.TimeoutExpired:
        v['timedOut'] = True
        import os
        os.killpg(p.pid, signal.SIGTERM)
        try: code = p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL); code = p.wait(timeout=10)
v.update(status='PASS' if code == 0 and not v['timedOut'] else 'FAIL', actualExit=True,
    exitCode=code, elapsedSeconds=time.monotonic()-start, completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
for name in ('stdout', 'stderr'):
    q = C / ('archive-operation-first.' + name + '.log')
    data = q.read_bytes(); v[name] = dict(path=str(q), bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
save()
print(json.dumps(v))
raise SystemExit(0 if v['status'] == 'PASS' else 1)
