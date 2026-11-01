from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import urllib.request

EV = Path(__file__).resolve().parent
TOOLS = Path('/tmp/fc-round53-msi-tools')
BASE = 'https://mirrors.edge.kernel.org/ubuntu/'
PACKAGES = ['wixl', 'wixl-data', 'msitools', 'libmsi-1.0-0',
            'libgcab-1.0-0', 'libgsf-1-114', 'libgsf-1-common']
REPORT = EV / 'tools-preparation-second.json'
assert not REPORT.exists()
assert (TOOLS / '.owner').read_text() == 'future-classroom-owned-r53-msi-tools/v1\n'
result = {'status': 'RUNNING', 'packages': [], 'tools': [],
          'scope': 'Exact Ubuntu official repository package SHA; mirror is transfer only. Owned extraction, no sudo/install/global linker changes/Windows actions.'}


def save():
    REPORT.write_text(json.dumps(result, indent=2) + '\n')


save()
try:
    for name in PACKAGES:
        raw = subprocess.check_output(['apt-cache', 'show', name])
        metadata = EV / (name + '-apt-metadata-first.txt')
        if metadata.exists():
            assert metadata.read_bytes() == raw
        else:
            metadata.write_bytes(raw)
        fields = {}
        for line in raw.decode().split('\n\n')[0].splitlines():
            if ': ' in line and not line.startswith(' '):
                key, value = line.split(': ', 1)
                fields[key] = value
        assert fields['Origin'] == 'Ubuntu'
        expected_size = int(fields['Size'])
        assert 0 < expected_size < 2 * 1024 * 1024
        path = TOOLS / 'downloads' / Path(fields['Filename']).name
        if path.exists():
            data = path.read_bytes()
        else:
            with urllib.request.urlopen(BASE + fields['Filename'], timeout=25) as response:
                data = response.read(expected_size + 1)
            with path.open('xb') as stream:
                stream.write(data)
        assert len(data) == expected_size and hashlib.sha256(data).hexdigest() == fields['SHA256']
        process = subprocess.run(['dpkg-deb', '--extract', str(path), str(TOOLS / 'root')],
                                 capture_output=True, timeout=30)
        assert process.returncode == 0
        result['packages'].append({'name': name, 'version': fields['Version'],
                                   'bytes': expected_size, 'sha256': fields['SHA256'],
                                   'officialRepositoryPath': fields['Filename'],
                                   'transferBase': BASE, 'extracted': True})
        save()
    env = dict(os.environ)
    env['LD_LIBRARY_PATH'] = str(TOOLS / 'root/usr/lib/x86_64-linux-gnu')
    for name in ['wixl', 'msiinfo', 'msiextract']:
        path = TOOLS / 'root/usr/bin' / name
        process = subprocess.run([str(path), '--version'], env=env,
                                 capture_output=True, timeout=15)
        for label, data in [('stdout', process.stdout), ('stderr', process.stderr)]:
            with (EV / (name + '-version-second.' + label)).open('xb') as stream:
                stream.write(data)
        result['tools'].append({'path': str(path), 'exitCode': process.returncode,
                                'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
        assert process.returncode == 0
        save()
    result['status'] = 'PASS_OWNED_TOOL_EXTRACTION_AND_VERSIONS_ONLY'
except Exception as error:
    result.update(status='FAIL', errorKind=type(error).__name__)
finally:
    result['finishedUtc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    save()
print(json.dumps({'status': result['status'], 'packages': len(result['packages']),
                  'tools': result['tools'], 'errorKind': result.get('errorKind')}))
raise SystemExit(0 if result['status'].startswith('PASS') else 1)
