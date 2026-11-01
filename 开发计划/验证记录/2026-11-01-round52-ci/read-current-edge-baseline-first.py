from pathlib import Path
import base64
import hashlib
import json
import runpy
import subprocess

EV = Path(__file__).resolve().parent
ROOT = Path('/workspace/共享/Eliora/未来教室计划')
OUT = EV / 'edge-installed-readonly-first.json'
assert not OUT.exists()
helper = runpy.run_path(str(ROOT / 'scripts/windows-probe.py'))
remote = helper['Remote']('elior@192.168.10.212', Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212'))
source = r'''$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);
$p='C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe';
$item=Get-Item -LiteralPath $p;
$hash=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant();
$sig=Get-AuthenticodeSignature -LiteralPath $p;
@{path=$item.FullName;bytes=$item.Length;sha256=$hash;fileVersion=$item.VersionInfo.FileVersion;productVersion=$item.VersionInfo.ProductVersion;
signatureStatus=$sig.Status.ToString();signatureType=$sig.SignatureType.ToString();
signerSubject=$sig.SignerCertificate.Subject;signerIssuer=$sig.SignerCertificate.Issuer;signerThumbprint=$sig.SignerCertificate.Thumbprint;
reparsePoint=(($item.Attributes -band [IO.FileAttributes]::ReparsePoint)-ne 0);
browserStarted=$false;fileChanged=$false;systemSettingsChanged=$false} | ConvertTo-Json -Compress
'''
command = ['ssh', *remote.options, remote.host,
           'powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + base64.b64encode(source.encode('utf-16-le')).decode()]
process = subprocess.run(command, capture_output=True, timeout=60)
raw = lambda data: {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(), 'base64': base64.b64encode(data).decode()}
report = {'status': 'FAIL', 'scope': 'Read-only fixed installed Edge file/hash/version and Authenticode signature. No browser/task/registry/service/update/volume/trust changes.',
          'command': command, 'powershellSource': source, 'exitCode': process.returncode,
          'stdout': raw(process.stdout), 'stderr': raw(process.stderr)}
try:
    assert process.returncode == 0
    value = json.loads(process.stdout.decode('utf-8-sig'))
    assert value['path'] == r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
    assert value['signatureStatus'] == 'Valid' and 'O=Microsoft Corporation' in value['signerSubject']
    assert not value['reparsePoint'] and not value['browserStarted'] and not value['fileChanged'] and not value['systemSettingsChanged']
    assert len(value['sha256']) == 64 and 0 < value['bytes'] < 64 * 1024 * 1024
    report.update(status='PASS_CURRENT_INSTALLED_SIGNED_EDGE_READONLY', observed=value)
finally:
    with OUT.open('x') as stream:
        json.dump(report, stream, indent=2)
        stream.write('\n')
print(json.dumps({'status': report['status'], 'observed': report.get('observed')}))
