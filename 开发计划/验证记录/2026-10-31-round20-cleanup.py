import hashlib, importlib.util, json, os, shutil, stat
from pathlib import Path

out = Path('开发计划/验证记录/2026-10-31-round20-cleanup.json')
active = []
for process in Path('/proc').iterdir():
    if not process.name.isdigit() or int(process.name) == os.getpid():
        continue
    try:
        arguments = (process / 'cmdline').read_bytes().decode(errors='replace').split('\0')
    except OSError:
        continue
    if any(argument.startswith('/tmp/fc-round20-') for argument in arguments):
        active.append(int(process.name))
if active:
    raise SystemExit('Refusing cleanup while owned tasks remain: ' + str(active))
paths = sorted(Path('/tmp').glob('fc-round20-*'))
records = []
for path in paths:
    files, total = 0, 0
    if path.is_dir() and not path.is_symlink():
        for base, directories, names in os.walk(path, followlinks=False):
            for name in names:
                entry = Path(base) / name
                files += 1
                total += entry.lstat().st_size
    else:
        files, total = 1, path.lstat().st_size
    records.append({'path': str(path), 'files': files, 'logicalBytes': total})
for path in paths:
    if path.is_dir() and not path.is_symlink():
        shutil.rmtree(path)
    else:
        path.unlink()
cache = Path(importlib.util.cache_from_source('scripts/windows-probe.py'))
removed_cache = []
if cache.is_file():
    cache.unlink()
    removed_cache.append(str(cache))
    try:
        cache.parent.rmdir()
    except OSError:
        pass
remaining = list(Path('/tmp').glob('fc-round20-*'))
key = Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212')
assert not remaining
assert stat.S_IMODE(key.stat().st_mode) == 0o600
report = {'status': 'PASS', 'scope': 'only this round owned temporary namespaces and generated Python bytecode', 'topLevelPaths': len(records), 'files': sum(r['files'] for r in records), 'logicalBytes': sum(r['logicalBytes'] for r in records), 'removed': records, 'removedBytecode': removed_cache, 'remainingOwnedPaths': [], 'persistentSshPrivateKeyRetained': True, 'persistentSshPrivateKeyMode': '0600', 'preserved': ['existing workspace target/dist/node_modules and global caches', '/mnt/data/FutureClassroom-0.1.0-public-calendar-dev.zip', 'Windows SSH service and authorized public key'], 'goalStatus': 'active'}
out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({k: report[k] for k in ['status', 'topLevelPaths', 'files', 'logicalBytes']}, ensure_ascii=False))
