"""Finite explicit finished product evidence only; never enters source/git/cache."""
from pathlib import Path
import hashlib,json,stat
R=Path('/tmp/fc-round80-controlled-repair')
DIRS=['a470-msvc-post-write-preparation-first','a470-msvc-post-write-operation-first','a470-msvc-post-write-runtime-preparation-first','a470-msvc-post-write-runtime-first']
ROOTFILES=['root-a470-msvc-post-write-ps-parse-first.json','root-a470-msvc-post-write-ps-parse-first.stdout.log','root-a470-msvc-post-write-ps-parse-first.stderr.log','root-a470-msvc-post-write-runtime-review-first.json']
def sha(b):return hashlib.sha256(b).hexdigest()
def regular(p):
 assert p.is_absolute() and p.is_relative_to(R) and p.resolve()==p
 for q in (p,*p.parents):
  if q==R or q.is_relative_to(R):assert not q.is_symlink()
 assert stat.S_ISREG(p.lstat().st_mode);return p

def collect():
 rows=[];ids=[];pruned=[]
 def add(p):
  p=regular(p);b=p.read_bytes();assert len(b)<=96*1024*1024
  row=dict(path=str(p.relative_to(R)),bytes=len(b),sha256=sha(b))
  binary=p.suffix.lower() in ['.exe','.dll','.msi','.cab','.zip','.pdb','.tar','.gz'] or b.startswith((b'MZ',b'MSCF',b'\xfe\xff\x00\x00\x05\x00\x02\x00',b'PK\x03\x04',b'\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1'))
  (ids if binary else rows).append(row)
 def descend(p):
  assert p.is_dir() and not p.is_symlink()
  for q in sorted(p.iterdir()):
   assert not q.is_symlink()
   if q.is_dir():descend(q)
   else:add(q)
 for name in DIRS:descend(R/name)
 for name in ROOTFILES:add(R/name)
 p=R/'a470-msvc-post-write-package-first'
 for q in sorted(p.iterdir()):
  assert not q.is_symlink()
  if q.is_file():add(q)
  elif q.name=='Binary':
   for f in sorted(q.iterdir()):assert f.is_file();add(f)
  elif q.name=='extracted':pruned.append(dict(path=str(q.relative_to(R)),policy='DUPLICATE_14_PAYLOAD_TREE_NOT_TRAVERSED',identitySource='derivation-report.json exact14 payload extraction; frozen D4 normal package'))
  else:raise AssertionError('UNKNOWN_DIRECTORY '+str(q))
 pruned.extend([dict(path='upstream-D3-CI',policy='ALREADY_FROZEN_FULL_CI_SOURCE_LOGS_AND_OFFICIAL_ZIP_NOT_DUPLICATED'),dict(path='upstream-D4-normal-product',policy='ALREADY_FROZEN_CANONICAL_INPUTS_AND_NORMAL_PACKAGE_NOT_DUPLICATED'),dict(path='tools/targets/caches/.git/sourcecopy/privatekeys/host/Reader',policy='OUTSIDE_EXPLICIT_SELECTION_NOT_TRAVERSED')])
 assert len(rows)+len(ids)<1024 and len({r['path'] for r in rows+ids})==len(rows)+len(ids)
 return sorted(rows,key=lambda r:r['path']),sorted(ids,key=lambda r:r['path']),pruned
if __name__=='__main__':
 rows,ids,pruned=collect();out=Path(__file__).parent/'source-manifest-first.json'
 out.write_text(json.dumps(dict(status='FINITE_FINISHED_A470_MSVC_PRODUCT_GNU_WITNESS_POST_WRITE_SELECTION',originals=rows,identityOnly=ids,pruned=pruned,originalCount=len(rows),originalBytes=sum(r['bytes'] for r in rows),selectorSha256=sha(Path(__file__).read_bytes())),ensure_ascii=False,indent=2)+'\n')
 print(json.dumps(dict(originals=len(rows),bytes=sum(r['bytes'] for r in rows),identityOnly=len(ids))))
