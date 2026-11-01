from pathlib import Path
import hashlib, json, subprocess

ROOT = Path.cwd()
EV = Path(__file__).resolve().parent
BASE = '6082dd0173a0b8884c27ac78fda5054275f99f14'
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip() == BASE
assert not subprocess.check_output(['git','diff','--cached','--name-only'])
source = {r['path'] for r in json.loads(Path('/tmp/fc-round61-gui/evidence/main-integration-first.json').read_text())['files']}
source.update({'.gitattributes','开发计划/10-实施进度与验证记录.md'})
assert len(source) == 15
expected = set(source)
for name, checksum in [
    ('2026-11-01-round59-ci','8e3b771c13acaac58b1966e42fcdbf2f26a990fd17f167906c4b33694cf68650'),
    ('2026-11-01-round61-gui','1d84139a1cbaf8cf30d0b9c9150ab235ce87344c711f67a9235317e8bce5d0ed')]:
    folder = ROOT / '开发计划/验证记录' / name
    assert hashlib.sha256((folder/'archive-index-first.json').read_bytes()).hexdigest() == checksum
    expected.update(p.relative_to(ROOT).as_posix() for p in folder.rglob('*') if p.is_file())
raw = subprocess.check_output(['git','status','--porcelain','-z','--untracked-files=all'])
observed = {s[3:].decode() for s in raw.split(b'\0') if s}
assert not observed-expected, ('Unexpected paths',sorted(observed-expected))
missing = expected-observed
checked = subprocess.run(['git','check-ignore','-z','--stdin'],input=b''.join(s.encode()+b'\0' for s in sorted(missing)),capture_output=True)
ignored = {s.decode() for s in checked.stdout.split(b'\0') if s}
assert missing == ignored, 'Missing paths are not precisely inherited ignored archive logs'
rows = []
for name in sorted(expected):
    p = ROOT/name
    assert p.is_file() and not p.is_symlink()
    data = p.read_bytes(); data.decode('utf-8')
    rows.append(dict(path=name,bytes=len(data),sha256=hashlib.sha256(data).hexdigest()))
inventory = dict(status='READY_EXACT_GUI_SOURCE_AND_R59_R61_ARCHIVES',sourceHead=BASE,
                 sourcePaths=sorted(source),count=len(rows),bytes=sum(r['bytes'] for r in rows),
                 explicitIgnoredLogs=len(ignored),files=rows)
out = EV/'batch-inventory-first.json'
assert not out.exists()
out.write_text(json.dumps(inventory,ensure_ascii=False,indent=2)+'\n')
subprocess.run(['git','add','-f','--',*[r['path'] for r in rows]],check=True)
staged = {s.decode() for s in subprocess.check_output(['git','diff','--cached','--name-only','-z']).split(b'\0') if s}
assert staged == expected
subprocess.run(['git','diff','--cached','--check'],check=True)
entries = {}
for row in subprocess.check_output(['git','ls-files','-s','-z']).split(b'\0'):
    if row:
        info,name = row.split(b'\t',1);mode,oid,stage = info.split()
        assert stage == b'0'
        entries[name.decode()] = oid.decode()
proc = subprocess.Popen(['git','cat-file','--batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
for row in rows:
    proc.stdin.write((entries[row['path']]+'\n').encode()); proc.stdin.flush()
    header = proc.stdout.readline().split()
    assert len(header)==3 and header[1]==b'blob'
    size = int(header[2]); data = proc.stdout.read(size)
    assert len(data)==size and proc.stdout.read(1)==b'\n'
    assert size == row['bytes'] and hashlib.sha256(data).hexdigest() == row['sha256'], row['path']
proc.stdin.close(); assert proc.wait(timeout=10)==0
result = dict(status='PASS_ALL_EXACT_STAGED_BLOBS',count=len(rows),bytes=inventory['bytes'],
              explicitIgnoredLogs=len(ignored),inventorySha256=hashlib.sha256(out.read_bytes()).hexdigest(),
              scope='Exact15 source/config/docs and two fully reviewed raw evidence archives only. No binary or credential added. No commit/push/CI yet.')
out = EV/'staged-bytes-first.json'
assert not out.exists(); out.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result),flush=True)
