from pathlib import Path
import base64, datetime, hashlib, json, runpy, subprocess

r = Path('/tmp/fc-round68-production-https/evidence')
p = r / 'loopback-dotnet-diagnostic-first.ps1'
report = r / 'windows-dotnet-loopback-first.json'
assert not report.exists()
source = p.read_bytes()
wrapper = '[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);$source=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($source));'
encoded = base64.b64encode(wrapper.encode('utf-16-le')).decode()
helper = runpy.run_path('/workspace/共享/Eliora/未来教室计划/scripts/windows-probe.py')
remote = helper['Remote']('elior@192.168.10.212', Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212'))
command = ['ssh', *remote.options, remote.host, 'powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + encoded]
raw = lambda b: {'bytes': len(b), 'sha256': hashlib.sha256(b).hexdigest(), 'base64': base64.b64encode(b).decode()}
result = subprocess.run(command, input=source, capture_output=True, timeout=30)
d = {'status': 'DIAGNOSTIC_COMPLETED' if result.returncode == 0 else 'FAIL_COMMAND',
     'recordedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'command': command,
     'source': {'path': str(p), **raw(source)}, 'wrapper': wrapper,
     'exitCode': result.returncode, 'stdout': raw(result.stdout), 'stderr': raw(result.stderr),
     'scope': 'Separate bounded .NET loopback diagnostic; no firewall, registry, trust, service, school setting or private-key read.'}
if result.returncode == 0:
    d['observations'] = json.loads(result.stdout.decode('utf-8-sig'))
report.open('x').write(json.dumps(d, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({'status': d['status'], 'observations': d.get('observations')}, ensure_ascii=False))
raise SystemExit(result.returncode)
