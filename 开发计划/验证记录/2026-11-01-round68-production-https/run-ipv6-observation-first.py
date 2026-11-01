from pathlib import Path
import base64, datetime, hashlib, json, runpy, subprocess, time

r = Path('/tmp/fc-round68-production-https/evidence')
p = r / 'observe-ipv6-policy-first.ps1'
report = r / 'windows-ipv6-policy-observation-first.json'
assert not report.exists()
source = p.read_bytes()
assert hashlib.sha256(source).hexdigest() == 'd5c4e6f007402e9dc591a5408270ca5985bc23119845797303adad46046f2642'
wrapper = "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$source=[Console]::In.ReadToEnd();$tokens=$null;$errors=$null;$null=[Management.Automation.Language.Parser]::ParseInput($source,[ref]$tokens,[ref]$errors);if($errors.Count){throw 'owned read-only source failed PowerShell parsing'};& ([ScriptBlock]::Create($source));"
encoded = base64.b64encode(wrapper.encode('utf-16-le')).decode()
helper = runpy.run_path('/workspace/共享/Eliora/未来教室计划/scripts/windows-probe.py')
remote = helper['Remote']('elior@192.168.10.212', Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212'))
command = ['ssh', *remote.options, remote.host, 'powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + encoded]
raw = lambda b: {'bytes': len(b), 'sha256': hashlib.sha256(b).hexdigest(), 'base64': base64.b64encode(b).decode()}
d = {'status': 'RUNNING', 'startedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
     'command': command, 'source': {'path': str(p), **raw(source)}, 'wrapper': wrapper,
     'hardControllerBudgetSeconds': 60, 'internalObservationBudgetSeconds': 45,
     'scope': 'PowerShell parse before read-only bounded local policy queries; no effective WFP verdict and no system setting mutation.'}
report.open('x').write(json.dumps(d, ensure_ascii=False, indent=2) + '\n')
started = time.monotonic()
try:
    result = subprocess.run(command, input=source, capture_output=True, timeout=60)
    d.update(status='DIAGNOSTIC_COMPLETED' if result.returncode == 0 else 'FAIL_COMMAND',
             exitCode=result.returncode, stdout=raw(result.stdout), stderr=raw(result.stderr))
    if result.returncode == 0:
        d['observations'] = json.loads(result.stdout.decode('utf-8-sig'))
except subprocess.TimeoutExpired as error:
    d.update(status='FAIL_TIMEOUT_PARTIAL_OBSERVATION', exitCode=None,
             stdout=raw(error.stdout or b''), stderr=raw(error.stderr or b''))
except Exception as error:
    d.update(status='FAIL_OBSERVATION', errorType=type(error).__name__)
d.update(elapsedSeconds=time.monotonic()-started, completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
report.write_text(json.dumps(d, ensure_ascii=False, indent=2) + '\n')
observed = d.get('observations', {})
print(json.dumps({'status': d['status'], 'elapsedSeconds': d['elapsedSeconds'],
                  'process': observed.get('process'), 'disabledComponents': observed.get('disabledComponents'),
                  'loopbackRoute': observed.get('loopbackRoute'), 'firewallProfiles': observed.get('firewallProfiles'),
                  'outboundBlockRules': observed.get('outboundBlockRules')}, ensure_ascii=False))
raise SystemExit(0 if d['status'] == 'DIAGNOSTIC_COMPLETED' else 1)
