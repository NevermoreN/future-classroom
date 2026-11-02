"""Finite explicit finished product evidence only; never enters source/git/cache."""
from pathlib import Path
import hashlib,json,stat
R=Path('/tmp/fc-round80-controlled-repair')
DIRS=['a470-msvc-product-preparation-first','a470-msvc-product-runtime-preparation-first','a470-msvc-product-runtime-preparation-second','a470-msvc-product-runtime-first','a470-msvc-product-runtime-second']
ROOTFILES=['root-a470-msvc-product-ps-parse-first.json','root-a470-msvc-product-ps-parse-first.stdout.log','root-a470-msvc-product-ps-parse-first.stderr.log','root-a470-msvc-product-ps-parse-second.json','root-a470-msvc-product-ps-parse-second.stdout.log','root-a470-msvc-product-ps-parse-second.stderr.log','root-a470-msvc-product-first-nonce-readonly.json','root-a470-msvc-product-first-nonce-readonly.stdout.log','root-a470-msvc-product-first-nonce-readonly.stderr.log','root-a470-msvc-product-runtime-review-first.json']
INPUTFILES=['.owner','manifest.json','operations.json','prepare-operation-first.json','prepare-operation-first.stdout.log','prepare-operation-first.stderr.log','source-init.stdout.log','source-init.stderr.log','source-detached-head.stdout.log','source-detached-head.stderr.log','source-index.stdout.log','source-index.stderr.log']
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
  binary=p.suffix.lower() in ['.exe','.dll','.msi','.zip','.pdb','.tar','.gz'] or b.startswith((b'MZ',b'PK\x03\x04',b'\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1'))
  (ids if binary else rows).append(row)
 def descend(p):
  assert p.is_dir() and not p.is_symlink()
  for q in sorted(p.iterdir()):
   assert not q.is_symlink()
   if q.is_dir():descend(q)
   else:add(q)
 for name in DIRS:descend(R/name)
 for name in ROOTFILES:add(R/name)
 for name in INPUTFILES:add(R/'a470-msvc-product-input-first'/name)
 for name in ['a470-msvc-product-assembly-first','a470-msvc-product-package-first']:
  p=R/name
  for q in sorted(p.iterdir()):
   assert not q.is_symlink()
   if q.is_file():add(q)
   elif name.endswith('package-first') and q.name in ['lifecycle','Binary']:
    for f in sorted(q.iterdir()):assert f.is_file();add(f)
   else:pruned.append(dict(path=str(q.relative_to(R)),policy='DUPLICATE_DERIVED_PAYLOAD_TREE_NOT_TRAVERSED',identitySource='a470-msvc-product-package-first/report.json payloads/exact extraction'))
 # Only the three actual official normal identities, not the full CI/test closure.
 for name in ['product/fc-bootstrap/fc-bootstrap.exe','product/fc-bootstrap/fc-installer-lifecycle.exe','custom_action_library/fc-windows/fc_windows.dll']:add(R/'a470-msvc-product-input-first/ci-evidence'/name)
 pruned.extend([dict(path='a470-msvc-product-input-first/source',policy='PARTIAL_CI_SOURCE_COPY_AND_SELF_GIT_METADATA_NOT_TRAVERSED_IDENTITY_ONLY',identitySource='a470-msvc-product-input-first/manifest.json gitIdentity/materializationProof/raw618'),dict(path='a470-msvc-product-input-first/ci-evidence',policy='BROAD_CI_TEXT_NOT_REPEATED; ONLY3NORMAL_BINARY_IDENTITIES_READ',identitySource='upstream D3 frozen official ZIP/CI evidence'),dict(path='a470-msvc-product-input-first/web-dist',policy='DERIVED_WEB_FILES_NOT_REPEATED',identitySource='input manifest complete65 source/7dist SHA ledger')])
 assert len(rows)+len(ids)<1024 and len({r['path'] for r in rows+ids})==len(rows)+len(ids)
 return sorted(rows,key=lambda r:r['path']),sorted(ids,key=lambda r:r['path']),pruned
if __name__=='__main__':
 rows,ids,pruned=collect();out=Path(__file__).parent/'source-manifest-first.json'
 out.write_text(json.dumps(dict(status='FINITE_FINISHED_A470_MSVC_PRODUCT_SOURCE_SELECTION',originals=rows,identityOnly=ids,pruned=pruned,originalCount=len(rows),originalBytes=sum(r['bytes'] for r in rows),selectorSha256=sha(Path(__file__).read_bytes())),ensure_ascii=False,indent=2)+'\n')
 print(json.dumps(dict(originals=len(rows),bytes=sum(r['bytes'] for r in rows),identityOnly=len(ids))))
