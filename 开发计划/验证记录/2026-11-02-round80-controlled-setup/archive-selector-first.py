#!/usr/bin/env python3
"""Only the root-designated closed setup evidence set; no root-wide traversal."""
from pathlib import Path
import hashlib,json,os,stat,tarfile
R=Path('/tmp/fc-round80-controlled-repair');P=R/'controlled-setup-archive-preparation-first';M=Path('/workspace/共享/Eliora/未来教室计划')
DIRS=['setup-source-preservation-first','setup-replay-source-preservation-first','setup-revision-first','setup-revision-second','guard-setup-build-first','guard-setup-build-second','guard-setup-clippy-first','setup-native-preparation-first','setup-native-runtime-first','setup-replay-native-preparation-first','setup-replay-native-preparation-second','setup-replay-native-runtime-first','guard-setup-replay-build-first','guard-setup-replay-clippy-first','controlled-packaging-implementation-first','controlled-product-producer-first','controlled-product-bindings-first','controlled-product-preparation-first','controlled-product-preparation-second','controlled-product-runtime-first','controlled-product-failure-review-first']
FILES=['build-controlled-product-first.py','build-controlled-web-first.py','package-controlled-product-first.py','check-controlled-product-ps-first.py','check-guard-candidate.py','setup-rust-inputs-first.json','setup-rust-inputs-second.json','setup-replay-rust-inputs-first.json','controlled-web-first/report.json']
PREFIXES=['root-setup-native-ps-parse-first','root-setup-ci-ast-first','root-setup-native-runtime-operation-first','root-controlled-product-package-first','root-controlled-product-ps-parse-first','root-controlled-product-runtime-operation-first','root-setup-replay-compiler-operation-first','root-setup-replay-native-ps-parse-first','root-setup-replay-native-runtime-operation-first','setup-replay-candidate-sync-first']
BACKEND=M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz';MEMBER='archive-preparation-second/archive-second.py'
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(j):return (json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode()
def regular(p):
 assert p.is_relative_to(R) and p.resolve()==p and p.is_file() and not p.is_symlink()
 assert not any(v in p.parts for v in ('target','target-first','node_modules','deps','checkout-52d6392-first','__pycache__'))
 return p

def collect():
 paths=[];links=[];pruned=[]
 def walk(p):
  for e in sorted(os.scandir(p),key=lambda x:x.name):
   q=Path(e.path);rel=q.relative_to(R).as_posix()
   if e.is_symlink():links.append({'path':rel,'target':os.readlink(q),'followed':False})
   elif e.is_dir(follow_symlinks=False):
    assert e.name not in ('target','target-first','node_modules','deps','__pycache__'),'UNEXPECTED_CACHE_INSIDE_EXPLICIT_SCOPE'
    walk(q)
   else:regular(q);paths.append(rel)
 for n in DIRS:
  p=R/n;assert p.is_dir() and not p.is_symlink();walk(p)
 for n in FILES:regular(R/n);paths.append(n)
 for prefix in PREFIXES:
  rows=[]
  for p in sorted(R.glob(prefix+'*')):
   if p.is_symlink():links.append({'path':p.name,'target':os.readlink(p),'followed':False})
   elif p.is_file():regular(p);paths.append(p.name);rows.append(p.name)
  assert rows,'MISSING_ROOT_PREFIX_'+prefix
 package=R/'controlled-product-package-first'
 for p in sorted(package.iterdir()):
  if p.is_symlink():links.append({'path':p.relative_to(R).as_posix(),'target':os.readlink(p),'followed':False})
  elif p.is_dir():
   if p.name=='payload':walk(p)
   else:pruned.append({'path':p.relative_to(R).as_posix(),'policy':'NOT_TRAVERSED_DUPLICATE_DERIVED_PACKAGE_TREE','identitySource':'controlled-product-package-first/report.json'})
  elif p.name in ('report.json','candidate.wxs','Setup.exe','FutureClassroom.msi') or p.suffix=='.idt' or p.name.endswith(('.stdout.log','.stderr.log')):regular(p);paths.append(p.relative_to(R).as_posix())
 assert len(paths)==len(set(paths))
 return sorted(paths),links,pruned

