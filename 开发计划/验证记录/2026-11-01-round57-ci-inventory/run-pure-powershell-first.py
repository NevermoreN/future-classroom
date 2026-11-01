from pathlib import Path
import base64, hashlib, json, runpy, subprocess, time

ROOT = Path('/workspace/共享/Eliora/未来教室计划')
TREE = Path('/tmp/fc-round57-ci-inventory-worktree')
E = Path('/tmp/fc-round57-ci-inventory-evidence')
script = TREE / 'scripts/check-ci-source-inventory.ps1'
source = TREE / 'scripts/windows-msvc-ci.ps1'
sha = lambda b: hashlib.sha256(b).hexdigest()
raw, checker = source.read_bytes(), script.read_bytes()
assert sha(raw) == '0cef8840cd51b1383e252b08df14b5c19f8dea9c3903583f97a4e6d4e5f365bd'
assert sha(checker) == 'b706d3d68627cc99570550c2e59fac645d66d2c997197393e00f689996bebbb8'
helper = runpy.run_path(str(ROOT / 'scripts/windows-probe.py'))
remote = helper['Remote']('elior@192.168.10.212', Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212'))
body = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding;\n"
body += "$ci=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('" + base64.b64encode(raw).decode() + "'));\n"
body += "$check=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('" + base64.b64encode(checker).decode() + "'));\n"
body += "& ([ScriptBlock]::Create($check)) -TextSource $ci -ExpectedSourceSha256 '" + sha(raw) + "'\n"
transmitted = body.encode('utf-8')
wrapper = '[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);$source=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($source));'
args = ['ssh', *remote.options, remote.host, 'powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + base64.b64encode(wrapper.encode('utf-16le')).decode()]
report = E / 'pure-powershell-first.json'
assert not report.exists()
record = {'status': 'RUNNING', 'sourceSha256': sha(raw), 'checkerSha256': sha(checker), 'timeoutSeconds': 90,
          'scope': 'Actual Windows PowerShell pure in-memory helper tests only; no hosted CI entrypoint, product, registry, file mutation, browser or foreground interaction.',
          'remoteInput': {'bytes': len(transmitted), 'sha256': sha(transmitted)}, 'args': args}
report.write_text(json.dumps(record, indent=2) + '\n')
started = time.monotonic()
try:
    result = subprocess.run(args, input=transmitted, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=90)
    stdout, stderr = result.stdout, result.stderr
    record.update(exitCode=result.returncode, actualSshExit=True)
    for kind, data in [('stdout', stdout), ('stderr', stderr), ('remote-input.ps1', transmitted)]:
        p = E / ('pure-powershell-first.' + kind)
        with p.open('xb') as stream:
            stream.write(data)
        record[kind] = {'path': str(p), 'bytes': len(data), 'sha256': sha(data)}
    assert len(stdout) <= 16 * 1024 * 1024 and len(stderr) <= 16 * 1024 * 1024
    parsed = json.loads(stdout.decode('utf-8-sig').strip())
    record['originalPowershellReport'] = parsed
    record['status'] = 'PASS_ACTUAL_POWERSHELL_MEMORY_ONLY' if result.returncode == 0 and parsed['status'] == 'PASS_PURE_POWERSHELL_INVENTORY_ONLY' else 'FAIL_ACTUAL_POWERSHELL_MEMORY_ONLY'
except Exception as error:
    record.update(status='FAIL', errorKind=type(error).__name__)
finally:
    record['elapsedSeconds'] = time.monotonic() - started
    record['sourceUnchanged'] = source.read_bytes() == raw and script.read_bytes() == checker
    report.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({k: record.get(k) for k in ['status', 'exitCode', 'elapsedSeconds', 'sourceUnchanged', 'errorKind']}), flush=True)
raise SystemExit(0 if record['status'] == 'PASS_ACTUAL_POWERSHELL_MEMORY_ONLY' else 1)
