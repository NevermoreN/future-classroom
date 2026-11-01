from pathlib import Path
import datetime, hashlib, json, os, re, signal, stat, subprocess, time

R = Path('/tmp/fc-round73-installer-broker')
W = R / 'worktree'
E = R / 'evidence'
O = E / 'n-host-first.json'
S = R / 'host-scratch-first'
MARKER = b'future-classroom-owned-r73-host-scratch/v1\n'
assert not O.exists() and not S.exists()
assert (R / '.owner').read_bytes() == b'future-classroom-owned-r73-installer-broker/v1\n'

def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()

def inputs():
    names = set()
    for command in [['git', 'diff', '--name-only', '-z', 'HEAD'], ['git', 'ls-files', '--others', '--exclude-standard', '-z']]:
        names.update(s.decode() for s in subprocess.check_output(command, cwd=W).split(b'\0') if s)
    names.add('Cargo.lock')
    return [dict(path=n, bytes=(W / n).stat().st_size, sha256=sha(W / n)) for n in sorted(names) if (W / n).is_file()]

def inventory():
    assert (S / '.owner').read_bytes() == MARKER
    rows = []
    for parent, dirs, files in os.walk(S, followlinks=False):
        for name in sorted(dirs + files):
            p = Path(parent) / name
            st = p.lstat()
            rows.append(dict(path=str(p.relative_to(S)), mode=stat.S_IMODE(st.st_mode), type='symlink' if stat.S_ISLNK(st.st_mode) else 'directory' if stat.S_ISDIR(st.st_mode) else 'regular' if stat.S_ISREG(st.st_mode) else 'other', bytes=st.st_size, inode=st.st_ino, device=st.st_dev))
    return sorted(rows, key=lambda x: x['path'])

freeze = E / 'n-implementation-freeze-third.json'
for row in json.loads(freeze.read_text())['source']:
    assert sha(W / row['path']) == row['sha256']
S.mkdir(mode=0o700)
(S / '.owner').write_bytes(MARKER)
env = os.environ.copy()
env.update(TMPDIR=str(S), RUST_TEST_THREADS='2')
v = dict(status='RUNNING', scope='Actual complete Host release workspace/all-target tests, then Host release strict Clippy; no Windows, source mutation or unsupported normal-main execution.', normalHelperUnsupportedMain='NOT_RUN: libtest harness replaces the normal main; Host helper has zero tests.', toolchain='1.97.1', originalPerCommandBudgetSeconds=900, jobs=2, rustTestThreads=2, sourceBefore=inputs(), nativeFreezeSha256=sha(freeze), scratchRoot=str(S), scratchBefore=inventory(), startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), steps=[])

def save():
    O.write_text(json.dumps(v, indent=2) + '\n')

save()
begin_all = time.monotonic()
common = ['--locked', '--offline', '--jobs', '2', '--target-dir', str(R / 'target'), '--release', '--workspace', '--all-targets']
commands = [('tests', ['rustup', 'run', '1.97.1', 'cargo', 'test', *common, '--message-format=json']), ('strict-clippy', ['rustup', 'run', '1.97.1', 'cargo', 'clippy', *common, '--', '-D', 'warnings'])]
for name, command in commands:
    out = E / ('n-host-' + name + '-first.stdout.log')
    err = E / ('n-host-' + name + '-first.stderr.log')
    assert not out.exists() and not err.exists()
    step = dict(name=name, command=command, environmentOverrides=dict(TMPDIR=str(S), RUST_TEST_THREADS='2'), status='RUNNING')
    v['steps'].append(step)
    save()
    begin = time.monotonic()
    with out.open('xb') as stdout, err.open('xb') as stderr:
        child = subprocess.Popen(command, cwd=W, env=env, stdout=stdout, stderr=stderr, start_new_session=True)
        step['pid'] = child.pid
        save()
        try:
            code = child.wait(timeout=900)
            step['timedOut'] = False
        except subprocess.TimeoutExpired:
            step['timedOut'] = True
            os.killpg(child.pid, signal.SIGTERM)
            try:
                code = child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                code = child.wait(timeout=10)
    step.update(status='PASS' if code == 0 and not step['timedOut'] else 'FAIL', exitCode=code, actualExit=True, elapsedSeconds=time.monotonic() - begin, stdout=dict(path=out.name, bytes=out.stat().st_size, sha256=sha(out)), stderr=dict(path=err.name, bytes=err.stat().st_size, sha256=sha(err)))
    save()
    if step['status'] != 'PASS':
        break

v.update(sourceAfter=inputs(), scratchAfter=inventory(), elapsedSeconds=time.monotonic() - begin_all)
v['sourceUnchanged'] = v['sourceBefore'] == v['sourceAfter']
v['scratchDelta'] = [x for x in v['scratchAfter'] if x not in v['scratchBefore']]
v['scratchMissing'] = [x for x in v['scratchBefore'] if x not in v['scratchAfter']]
v['status'] = 'PASS_HOST_TESTS_AND_CLIPPY' if len(v['steps']) == 2 and all(s['status'] == 'PASS' for s in v['steps']) and v['sourceUnchanged'] else 'FAIL'
if v['steps'][0]['status'] == 'PASS':
    try:
        output = (E / 'n-host-tests-first.stdout.log').read_text()
        errors = (E / 'n-host-tests-first.stderr.log').read_text()
        artifacts = {}
        for line in output.splitlines():
            try:
                item = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(item, dict) and item.get('reason') == 'compiler-artifact' and item.get('executable') and item.get('profile', {}).get('test') is True:
                p = Path(item['executable'])
                assert p.resolve().is_relative_to((R / 'target').resolve())
                artifacts[str(p)] = dict(target=item['target']['name'], kind=item['target']['kind'], packageId=item['package_id'], path=str(p), sha256=sha(p), profileTest=True)
        executed = re.findall(r'^\s+Running [^\n]*\(([^\n]+)\)\s*$', errors, re.M)
        headers = re.findall(r'^running (\d+) tests?\s*$', output, re.M)
        summaries = list(re.finditer(r'^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in ([0-9.]+)s\s*$', output, re.M))
        assert len(executed) == len(headers) == len(summaries) == len(artifacts)
        assert len(set(executed)) == len(executed) and set(executed) == set(artifacts)
        rows = []
        for path, header, summary in zip(executed, headers, summaries):
            status, passed, failed, ignored, measured, filtered, elapsed = summary.groups()
            counts = [int(x) for x in [passed, failed, ignored, measured, filtered]]
            assert status == 'ok' and counts[1:] == [0, 0, 0, 0] and int(header) == counts[0]
            rows.append(dict(**artifacts[path], running=int(header), passed=counts[0], failed=0, ignored=0, measured=0, filtered=0, elapsedSeconds=float(elapsed), summaryLine=output[:summary.start()].count('\n') + 1))
        v['testHarnesses'] = rows
        v['testHarnessCount'] = len(rows)
        v['totalPassed'] = sum(x['passed'] for x in rows)
        v['zeroTestHarnesses'] = [x['target'] for x in rows if x['running'] == 0]
    except Exception as error:
        v.update(status='FAIL_HARNESS_ATTRIBUTION', harnessAttributionError=type(error).__name__)
if not v['scratchDelta'] and not v['scratchMissing']:
    assert (S / '.owner').read_bytes() == MARKER
    (S / '.owner').unlink()
    S.rmdir()
    v['scratchCleanup'] = 'PASS_ONLY_EXACT_OWNED_MARKER_AND_EMPTY_ROOT_REMOVED'
else:
    v['scratchCleanup'] = 'RETAINED_DELTA_REQUIRES_EXACT_OWNERSHIP_REVIEW'
save()
print(json.dumps({k: v.get(k) for k in ['status', 'sourceUnchanged', 'testHarnessCount', 'totalPassed', 'zeroTestHarnesses', 'scratchCleanup', 'elapsedSeconds']}) + ' reportSha256=' + sha(O), flush=True)
raise SystemExit(0 if v['status'] == 'PASS_HOST_TESTS_AND_CLIPPY' and v['scratchCleanup'].startswith('PASS') else 1)
