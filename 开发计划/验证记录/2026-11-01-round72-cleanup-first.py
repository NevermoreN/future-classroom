"""Delete only terminal R72 owned scratch after committed exact-byte proof."""
from pathlib import Path
import base64, gzip, hashlib, json, os, shutil, stat, subprocess, time
repo = Path('/workspace/共享/Eliora/未来教室计划')
records = repo / '开发计划/验证记录'
source = Path('/tmp/fc-round72-ci-evidence')
control = Path('/tmp/fc-round72-archive-controller')
commit = '276d0f5943fdd6024c894d1b21868a0394466931'
report = records / '2026-11-01-round72-cleanup-first.json'
assert not report.exists()
def sha(b): return hashlib.sha256(b).hexdigest()
def git(*args, data=None): return subprocess.check_output(['git', *args], cwd=repo, input=data)
proof_path = records / '2026-11-01-round72-committed-bytes-first.json'
proof = json.loads(proof_path.read_bytes())
assert proof['status'] == 'PASS_EXACT_COMMITTED_BLOBS' and proof['commit'] == commit
assert git('rev-parse', commit + '^{commit}').decode().strip() == commit
oids = sorted({r['gitBlobOid'] for r in proof['files']})
raw = git('cat-file', '--batch', data=('\n'.join(oids)+'\n').encode())
blobs = {}; offset = 0
for oid in oids:
    end = raw.index(b'\n', offset)
    actual, kind, size = raw[offset:end].decode().split()
    assert actual == oid and kind == 'blob'
    start = end + 1; size = int(size)
    blobs[oid] = raw[start:start+size]
    assert raw[start+size:start+size+1] == b'\n'
    offset = start + size + 1
assert offset == len(raw)
for row in proof['files']:
    data = blobs[row['gitBlobOid']]
    assert len(data) == row['bytes'] and sha(data) == row['sha256'] and (repo/row['path']).read_bytes() == data
archive = records / '2026-11-01-round72-ci'
index = json.loads((archive/'archive-index-first.json').read_bytes())
assert (source/'.owner').read_bytes() == b'future-classroom-owned-r72-ci-evidence/v1\n'
assert (control/'.owner').read_bytes() == b'future-classroom-owned-r72-archive-controller/v1\n'
expected = {r['path'] for r in index['files'] + index['excludedCompiledPayloads']}
assert {p.relative_to(source).as_posix() for p in source.rglob('*') if p.is_file()} == expected
for row in index['files']:
    actual = (source/row['path']).read_bytes()
    stored = (archive/row.get('archivePath',row['path'])).read_bytes()
    if row.get('storage') == 'gzip_original_bytes': stored = gzip.decompress(stored)
    assert actual == stored and len(actual) == row['bytes'] and sha(actual) == row['sha256']
for row in index['excludedCompiledPayloads']:
    data = (source/row['path']).read_bytes()
    assert len(data) == row['bytes'] and sha(data) == row['sha256']
mapping = {'record-archive-first.py':'record-archive-first.py', 'archive-operation-first.json':'archive-operation-first.json',
    'archive-operation-first.stdout.log':'archive-operation-first.stdout.log', 'archive-operation-first.stderr.log':'archive-operation-first.stderr.log',
    'byte-replay-first.json':'byte-replay-first.json', 'staged-bytes-first.json':'staged-bytes-first.json', 'committed-bytes-first.json':'committed-bytes-first.json'}
assert {p.name for p in control.iterdir()} == set(mapping) | {'.owner'}
for local, target in mapping.items():
    assert (control/local).read_bytes() == (records/('2026-11-01-round72-'+target)).read_bytes()
parents = set(); pid = os.getpid()
while pid > 0 and pid not in parents:
    parents.add(pid)
    try: pid = int(Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()[1])
    except (OSError,ValueError,IndexError): break
active = []
for p in Path('/proc').iterdir():
    if not p.name.isdigit() or int(p.name) in parents: continue
    try:
        if p.stat().st_uid != os.geteuid(): continue
        command = (p/'cmdline').read_bytes()
    except (OSError,ProcessLookupError): continue
    if any(str(root).encode() in command for root in (source,control)): active.append(int(p.name))
assert not active, 'ACTIVE_OWNED_PROCESS_RETAINS_ROOT'
inventories = []
for root in (source, control):
    assert root.is_dir() and not root.is_symlink() and root.resolve() == root
    members = list(root.rglob('*'))
    assert all(not p.is_symlink() and (p.is_dir() or stat.S_ISREG(p.lstat().st_mode)) for p in members)
    files = [p for p in members if p.is_file()]
    inventories.append(dict(path=str(root), files=len(files), logicalBytes=sum(p.stat().st_size for p in files)))
key = Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212')
assert stat.S_IMODE(key.stat().st_mode) == 0o600
assert Path('/tmp/fc-round73-installer-broker/.owner').is_file()
t = time.monotonic()
for root in (source, control): shutil.rmtree(root)
assert not source.exists() and not control.exists()
v = dict(status='PASS_ONLY_TERMINAL_OWNED_R72_ROOTS_REMOVED', verifiedCommit=commit,
    committedProof=dict(path=str(proof_path.relative_to(repo)), sha256=sha(proof_path.read_bytes())),
    committedFilesReopened=len(proof['files']), archivedOriginalsReplayed=len(index['files']), excludedPayloadIdentitiesVerified=len(index['excludedCompiledPayloads']),
    removedRoots=inventories, activeOwnedProcesses=active, elapsedDeletionSeconds=time.monotonic()-t,
    persistentPrivateKey=dict(mode='0o600',contentsRead=False,preserved=True),
    preserved='Active R73, main target/global caches, development ZIPs, persistent Windows SSH service/public authorization/private key.',
    scope='Local scratch cleanup only; exact committed blobs and source/stored/gzip bytes checked before deletion. No Windows/network/process termination or new runtime acceptance; physical disk freed not measured.')
with report.open('x') as s: json.dump(v,s,indent=2); s.write('\n')
print(json.dumps(v))
