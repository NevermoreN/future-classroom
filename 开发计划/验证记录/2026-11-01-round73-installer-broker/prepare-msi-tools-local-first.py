#!/usr/bin/env python3
"""Unpack already exact-verified R73 packages; load/version-check tools only."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import re
import subprocess
import time

ROOT = Path('/tmp/fc-round73-installer-broker')
E = ROOT / 'evidence'
T = ROOT / 'tools'
config = json.loads((E / 'msi-tools-input-first.json').read_text())
assert (T / '.owner').read_bytes() == b'future-classroom-owned-r73-msi-tools/v1\n'
report = {'schemaVersion': 1, 'status': 'FAIL', 'startedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
          'scope': 'Owned package extraction, ELF/library and version checks only; no MSI database produced or candidate run.',
          'commands': [], 'packages': [], 'tools': [], 'libraries': [],
          'systemInstallation': False, 'sudoInvoked': False, 'globalCacheWritten': False,
          'candidateMsiProduced': False, 'windowsInvoked': False, 'frozenSourceWritten': False}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def fact(path):
    data = path.read_bytes()
    return {'path': str(path), 'bytes': len(data), 'sha256': sha(data)}


def run(label, command, *, env=None, allowed=(0,), timeout=60):
    stdout = E / ('msi-tools-' + label + '-first.stdout.log')
    stderr = E / ('msi-tools-' + label + '-first.stderr.log')
    began = time.monotonic()
    with stdout.open('xb') as out, stderr.open('xb') as err:
        child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=out, stderr=err, env=env)
        try:
            code = child.wait(timeout=timeout)
            expired = False
        except subprocess.TimeoutExpired:
            child.kill()
            code = child.wait(timeout=5)
            expired = True
    for path in (stdout, stderr):
        assert path.stat().st_size <= 16 * 1024 * 1024
    row = {'name': label, 'command': command, 'actualExit': True, 'exitCode': code,
           'controllerTimedOut': expired, 'elapsedSeconds': time.monotonic()-began,
           'deadlineSeconds': timeout, 'stdout': fact(stdout), 'stderr': fact(stderr)}
    report['commands'].append(row)
    assert not expired and code in allowed
    return stdout.read_bytes()


try:
    # Extraction mutates one owned tree and is deliberately sequential.
    for item in config['packages']:
        package = T / 'packages' / Path(item['officialRepositoryPath']).name
        assert fact(package)['bytes'] == item['bytes'] and fact(package)['sha256'] == item['sha256']
        fields = run('fields-' + item['name'], ['/usr/bin/dpkg-deb', '--field', str(package), 'Package', 'Version', 'Architecture', 'Depends'])
        assert ('Package: ' + item['name'] + '\n').encode() in fields
        assert ('Version: ' + item['version'] + '\n').encode() in fields
        run('extract-' + item['name'], ['/usr/bin/dpkg-deb', '-x', str(package), str(T / 'root')])
        report['packages'].append({**item, 'download': fact(package), 'extracted': True})
    env = os.environ.copy()
    for name in ('LD_PRELOAD', 'LD_AUDIT', 'GIO_EXTRA_MODULES', 'GIO_MODULE_DIR'):
        env.pop(name, None)
    library = T / 'root/usr/lib/x86_64-linux-gnu'
    env.update(LD_LIBRARY_PATH=str(library), LC_ALL='C', GIO_USE_VFS='local', GSETTINGS_BACKEND='memory')
    report['scopedEnvironment'] = {name: env[name] for name in ('LD_LIBRARY_PATH', 'LC_ALL', 'GIO_USE_VFS', 'GSETTINGS_BACKEND')}
    resolved = set()
    for expected in config['expectedTools']:
        name = expected['name']
        binary = T / 'root/usr/bin' / name
        actual = fact(binary)
        assert actual['bytes'] == expected['bytes'] and actual['sha256'] == expected['sha256']
        listing = run('ldd-' + name, ['/usr/bin/ldd', str(binary)], env=env)
        assert b'not found' not in listing
        for text in listing.decode('utf-8', 'strict').splitlines():
            match = re.search(r'(?:=>\s+)?(/\S+)\s+\(', text)
            if match:
                path = Path(match.group(1)).resolve(strict=True)
                assert path.is_file()
                resolved.add(path)
        if name == 'msibuild':
            usage = run('usage-' + name, [str(binary)], env=env, allowed=(1,), timeout=25)
            assert usage.startswith(b'Usage: msibuild MSIFILE [OPTION]...')
            check = {'kind': 'EXPECTED_NO_ARGUMENT_USAGE_EXIT1', 'versionBasis': 'Exact msitools 0.103+repack-1build3 package/ELF; upstream has no --version option.'}
        else:
            output = run('version-' + name, [str(binary), '--version'], env=env, timeout=25)
            assert b'0.103' in output
            check = {'kind': 'ACTUAL_VERSION_EXIT0', 'stdoutSha256': sha(output)}
        report['tools'].append({'name': name, **actual, 'exactR60BinaryMatch': True, 'versionCheck': check})
    report['libraries'] = [fact(path) for path in sorted(resolved)]
    include = T / 'root/usr/share/wixl-0.103/include'
    assert include.is_dir() and any(include.glob('*.wxi'))
    # The package candidate takes this one fixed wrapper for wixl's data path.
    # Other tools can use their exact native ELF paths, including msibuild's
    # reviewed binary SHA, with the recorded process-scoped LD_LIBRARY_PATH.
    wrapper = T / 'wrappers/wixl'
    wrapper_text = '''#!/usr/bin/env python3
from pathlib import Path
import os,sys
r=Path('/tmp/fc-round73-installer-broker/tools')
assert (r/'.owner').read_bytes()==b'future-classroom-owned-r73-msi-tools/v1\\n'
env=os.environ.copy()
for name in ('LD_PRELOAD','LD_AUDIT','GIO_EXTRA_MODULES','GIO_MODULE_DIR'):env.pop(name,None)
env.update(LD_LIBRARY_PATH=str(r/'root/usr/lib/x86_64-linux-gnu'),LC_ALL='C',GIO_USE_VFS='local',GSETTINGS_BACKEND='memory')
binary=r/'root/usr/bin/wixl'
os.execve(binary,[str(binary),'--wxidir',str(r/'root/usr/share/wixl-0.103/include'),'--extdir',str(r/'root/usr/share/wixl-0.103/ext')]+sys.argv[1:],env)
'''
    with wrapper.open('x') as output:
        output.write(wrapper_text)
    wrapper.chmod(0o700)
    assert run('wrapper-wixl-version', [str(wrapper), '--version'], timeout=25) == b'0.103\n'
    report['wixlWrapper'] = fact(wrapper)
    report['wixlData'] = {'includeDirectory': str(include), 'manifest': [fact(path) for path in sorted(include.rglob('*')) if path.is_file()]}
    report['invocation'] = {'environment': {'LD_LIBRARY_PATH': str(library)}, 'wixl': str(wrapper),
                            **{name: str(T / 'root/usr/bin' / name) for name in ('msibuild', 'msiinfo', 'msiextract')},
                            'msibuild_sha256': next(row['sha256'] for row in report['tools'] if row['name'] == 'msibuild'),
                            'note': 'Set LD_LIBRARY_PATH only for the packaging child; no global environment/cache change. This preparation never calls package-msi-candidate.py.'}
    report['status'] = 'PASS_EXACT_PACKAGES_LOCAL_EXTRACTION_ELF_BINDING_AND_SCOPED_TOOL_LOADING_ONLY'
finally:
    report['completedUtc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (E / 'msi-tools-preparation-first.json').open('x') as output:
        json.dump(report, output, ensure_ascii=False, indent=2)
        output.write('\n')
print(json.dumps({'status': report['status'], 'packages': len(report['packages']), 'tools': len(report['tools']),
                  'resolvedLibraries': len(report['libraries']), 'invocation': report.get('invocation')}))
