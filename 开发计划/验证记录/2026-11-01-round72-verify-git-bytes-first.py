"""Bind reviewed R72 working bytes to exact staged or committed Git blobs."""
from pathlib import Path
import argparse, hashlib, json, re, subprocess
repo = Path('/workspace/共享/Eliora/未来教室计划')
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--commit')
p.add_argument('--report', type=Path, required=True)
a = p.parse_args()
assert not a.report.exists()
def git(*words): return subprocess.check_output(['git', *words], cwd=repo)
if a.commit:
    assert re.fullmatch('[0-9a-f]{40}', a.commit)
    assert git('rev-parse', a.commit + '^{commit}').decode().strip() == a.commit
    paths = git('diff-tree', '--no-commit-id', '--name-only', '-r', '-z', a.commit).decode().split('\0')
else:
    paths = git('diff', '--cached', '--name-only', '--diff-filter=AM', '-z').decode().split('\0')
paths = [x for x in paths if x]
assert paths and len(paths) == len(set(paths))
outer = {'2026-11-01-round72-record-archive-first.py', '2026-11-01-round72-archive-operation-first.json',
    '2026-11-01-round72-archive-operation-first.stdout.log', '2026-11-01-round72-archive-operation-first.stderr.log',
    '2026-11-01-round72-byte-replay-first.json', '2026-11-01-round72-verify-git-bytes-first.py'}
rows = []
for name in paths:
    assert name in {'.gitattributes', '开发计划/10-实施进度与验证记录.md'} or name.startswith('开发计划/验证记录/2026-11-01-round72-ci/') or name in {'开发计划/验证记录/' + x for x in outer}
    path = repo / name
    assert path.is_file() and not path.is_symlink()
    working = path.read_bytes()
    spec = (a.commit + ':' if a.commit else ':') + name
    blob = git('show', spec)
    assert working == blob
    rows.append(dict(path=name, bytes=len(blob), sha256=hashlib.sha256(blob).hexdigest(), gitBlobOid=git('rev-parse', spec).decode().strip()))
result = dict(status='PASS_EXACT_COMMITTED_BLOBS' if a.commit else 'PASS_EXACT_STAGED_BLOBS',
    commit=a.commit, parentSource='00120a026eea5dc4ff330f8526bf31169b373565', files=rows,
    fileCount=len(rows), bytes=sum(r['bytes'] for r in rows),
    scope='Reviewed R72 archive/controller/progress/attributes only, each working byte equals actual Git blob. No new runtime or OS acceptance.')
with a.report.open('x') as s: json.dump(result, s, indent=2); s.write('\n')
print(json.dumps({k:result[k] for k in ('status','commit','fileCount','bytes')}))
