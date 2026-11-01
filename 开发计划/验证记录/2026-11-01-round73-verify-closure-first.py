"""Reopen exact current commit blobs for this reviewed R73 two-commit batch."""
from pathlib import Path
import argparse, hashlib, json, subprocess
M = Path('/workspace/共享/Eliora/未来教室计划')
p = argparse.ArgumentParser(); p.add_argument('--commit', required=True); p.add_argument('--report', type=Path, required=True); a = p.parse_args()
def git(*args, data=None): return subprocess.check_output(['git', *args], cwd=M, input=data)
assert git('rev-parse', 'HEAD').decode().strip() == a.commit
assert not git('diff', '--cached', '--name-only', '-z') and not a.report.exists()
base = '9c410aaea8e5cd9487d78b9d2540e20268ad03c7'
parts = git('diff', '--name-status', '--no-renames', '-z', base, a.commit).decode().split('\0')
assert parts[-1] == ''; changes = list(zip(parts[:-1:2], parts[1:-1:2])); assert all(s in ('A', 'M') for s,n in changes)
source = set(json.loads((M/'开发计划/验证记录/2026-11-01-round73-candidate-source-paths-first.json').read_bytes()))
docs = {'.gitattributes', 'AGENTS.md', '开发计划/10-实施进度与验证记录.md', '开发计划/11-Windows兼容矩阵与验收.md'}
names = [n for s,n in changes]
assert len(names) == len(set(names)) and source <= set(names)
assert all(n in source|docs or n.startswith('开发计划/验证记录/2026-11-01-round73-') for n in names)
oids = {n: git('rev-parse', a.commit+':'+n).decode().strip() for n in names}
unique = sorted(set(oids.values())); raw = git('cat-file','--batch',data=('\n'.join(unique)+'\n').encode())
blobs = {}; offset = 0
for oid in unique:
    end = raw.index(b'\n', offset); actual,kind,size = raw[offset:end].decode().split(); assert actual==oid and kind=='blob'
    start = end+1; size=int(size); blobs[oid]=raw[start:start+size]; assert raw[start+size:start+size+1]==b'\n'; offset=start+size+1
assert offset==len(raw)
rows=[]
for name in names:
    path=M/name; assert path.is_file() and not path.is_symlink(); data=blobs[oids[name]]; assert path.read_bytes()==data
    rows.append(dict(path=name,bytes=len(data),sha256=hashlib.sha256(data).hexdigest(),gitBlobOid=oids[name]))
result=dict(status='PASS_EXACT_COMMITTED_BLOBS',commit=a.commit,parentGitBase=base,files=rows,fileCount=len(rows),bytes=sum(x['bytes'] for x in rows),scope='Exact final R73 source/archive/controller/cleanup-plan blobs only. This new proof is outside temporary roots and does not claim its own bytes were already committed. No runtime acceptance expansion.')
with a.report.open('x') as f: json.dump(result,f,ensure_ascii=False,indent=2); f.write('\n')
print(json.dumps({k:result[k] for k in ('status','commit','fileCount','bytes')}))
