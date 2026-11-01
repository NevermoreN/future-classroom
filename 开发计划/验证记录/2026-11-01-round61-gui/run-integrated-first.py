from pathlib import Path
import datetime, hashlib, json, os, signal, subprocess, time

ROOT = Path('/tmp/fc-round61-gui/worktree')
EV = Path(__file__).resolve().parent
TARGET = EV.parent / 'target'
REPORT = EV / 'integrated-checks-first.json'
assert not REPORT.exists()

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def inputs():
    paths = {p for p in (ROOT / 'crates').rglob('*') if p.is_file() and
             (p.suffix == '.rs' or p.name == 'Cargo.toml')}
    paths.update(ROOT / p for p in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo/config.toml',
        'scripts/windows-msvc-ci.ps1', 'scripts/audit-pe.py', 'scripts/package-msi-candidate.py',
        'scripts/check-pe-subsystem.py', 'scripts/check-msvc-gui-contract.ps1', 'scripts/package-probe.py', 'README.md',
        'packaging/PROBE-README.txt', '开发计划/22-自启动与系统变更核对.md'))
    return {p.relative_to(ROOT).as_posix(): dict(bytes=p.stat().st_size, sha256=sha(p)) for p in sorted(paths)}

report = dict(status='RUNNING', scope='Isolated GUI candidate; host encoding tests, PE tooling models, compile and strict lint only. No Windows, MSVC, login, visible feedback or Seewo execution.',
              sourceBase='6082dd0173a0b8884c27ac78fda5054275f99f14', sourceBefore=inputs(), commands=[],
              commandTimeoutSeconds=900, streamLimitBytes=16*1024*1024, cargoJobs=2, testThreads=2)

def save():
    REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')

def run(label, args):
    row = dict(name=label, argv=args, status='RUNNING', startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
    report['commands'].append(row); save()
    outputs = {stream: EV / (label+'-first.'+stream+'.log') for stream in ('stdout', 'stderr')}
    env = dict(os.environ, CARGO_BUILD_JOBS='2', RUST_TEST_THREADS='2')
    start = time.monotonic(); reason = None
    with outputs['stdout'].open('xb') as out, outputs['stderr'].open('xb') as err:
        child = subprocess.Popen(args, cwd=ROOT, env=env, stdout=out, stderr=err, start_new_session=True)
        while child.poll() is None:
            if time.monotonic()-start > 900:
                reason = 'COMMAND_TIMEOUT'; break
            if any(p.stat().st_size > 16*1024*1024 for p in outputs.values()):
                reason = 'STREAM_LIMIT'; break
            time.sleep(.1)
        if reason:
            os.killpg(child.pid, signal.SIGTERM)
            try: child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL); child.wait(timeout=10)
        code = child.wait()
    row.update(status='PASS' if code == 0 and reason is None else 'FAIL', exitCode=code,
               reason=reason, actualExit=True, elapsedSeconds=time.monotonic()-start)
    row.update({s: dict(path=str(p), bytes=p.stat().st_size, sha256=sha(p)) for s,p in outputs.items()})
    save(); print(label+' '+row['status']+' '+str(round(row['elapsedSeconds'],2))+'s', flush=True)
    if row['status'] != 'PASS': raise RuntimeError(label)

def cargo(toolchain, command, args, kind='stable'):
    return ['cargo', '+'+toolchain, command]+args+['--target-dir',str(TARGET/kind)]

save()
try:
    run('fmt', ['cargo','+1.97.1','fmt','--all','--','--check'])
    run('architecture', ['python3','-B','scripts/check-architecture.py'])
    run('diffcheck', ['git','diff','--check'])
    run('pe-role-model', ['python3','-B','scripts/check-pe-subsystem.py'])
    run('msi-contract-model', ['python3','-B','scripts/check-msi-candidate.py'])
    run('host-windows-adapter-tests', cargo('1.97.1','test',['-p','fc-windows','--lib','--release','--locked','--offline'])+['--','--test-threads=2'])
    cross = ['--release','--locked','--offline','--target','i686-pc-windows-gnu']
    run('windows-adapter-compile', cargo('1.97.1','test',['-p','fc-windows','--lib']+cross+['--no-run','--message-format=json']))
    run('windows-product', cargo('1.97.1','build',['-p','fc-bootstrap','--bin','fc-bootstrap']+cross+['--message-format=json']))
    run('windows-stable-strict', cargo('1.97.1','clippy',['--workspace','--all-targets']+cross)+['--','-D','warnings'])
    run('windows-nightly-strict', cargo('nightly-2026-10-29','clippy',['--workspace','--all-targets']+cross,'nightly')+['--','-D','warnings'])
    report['status'] = 'PASS_HOST_MODELS_AND_COMPILE_LINT_ONLY_WINDOWS_NOT_RUN'
except Exception as error:
    report.update(status='FAIL', errorKind=type(error).__name__, failedCommand=str(error))
finally:
    report['sourceAfter'] = inputs()
    report['sourceUnchanged'] = report['sourceAfter'] == report['sourceBefore']
    if not report['sourceUnchanged']: report['status'] = 'FAIL_SOURCE_CHANGED'
    save()
raise SystemExit(0 if report['status'].startswith('PASS_') else 1)
