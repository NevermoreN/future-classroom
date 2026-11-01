"""Only the finite completed post-write step; no W/target/tools/scope traversal."""
from pathlib import Path
import os,stat
R=Path('/tmp/fc-round80-controlled-repair')
DIRS=['controlled-post-write-preparation-first','controlled-post-write-operation-first','controlled-post-write-runtime-preparation-first','controlled-post-write-runtime-first']
FILES=['controlled-product-package-second/report.json','controlled-product-bindings-second/artifact-manifest.json','controlled-product-bindings-second/source-manifest.json','root-post-write-final-binding-first.json','root-post-write-ps-parse-first-local-failure.json','root-post-write-runtime-operation-first.json','root-post-write-runtime-operation-first.stdout.log','root-post-write-runtime-operation-first.stderr.log','root-post-write-runtime-ps-parse-second.json','root-post-write-runtime-ps-parse-second.stdin.ps1','root-post-write-runtime-ps-parse-second.stdout.log','root-post-write-runtime-ps-parse-second.stderr.log']
def regular(p):
 assert p.is_absolute() and p.is_relative_to(R) and p.resolve()==p and stat.S_ISREG(p.lstat().st_mode)
 assert not any(v in p.parts for v in ('target','target-first','deps','tools','root','node_modules','__pycache__','checkout-52d6392-first'))
 assert p.name not in ('scope.hex','scope.bin') and p.suffix.lower() not in ('.key','.pfx','.p12','.pem','.crt','.cer','.der')
 return p

def collect():
 text_candidates=[];identity_payloads=[]
 def walk(p,only_identity=False):
  for e in sorted(os.scandir(p),key=lambda e:e.name):
   q=Path(e.path);assert not e.is_symlink()
   if e.is_dir(follow_symlinks=False):walk(q,only_identity)
   else:
    regular(q);(identity_payloads if only_identity else text_candidates).append(q.relative_to(R).as_posix())
 for name in DIRS:walk(R/name)
 for name in FILES:regular(R/name);text_candidates.append(name)
 package=R/'controlled-post-write-package-first'
 for p in sorted(package.iterdir()):
  assert not p.is_symlink()
  if p.is_dir():
   assert p.name in ('Binary','extracted');walk(p,True)
  else:
   assert p.name in ('.owner','FutureClassroom.msi','Setup.exe','witness.c','witness-ledger.h','witness.dll','Binary.idt','CustomAction.idt','InstallExecuteSequence.idt','derivation-report.json') or p.name.endswith(('.stdout.log','.stderr.log'))
   regular(p);text_candidates.append(p.relative_to(R).as_posix())
 assert len(text_candidates)==len(set(text_candidates)) and len(identity_payloads)==len(set(identity_payloads))
 return sorted(text_candidates),sorted(identity_payloads)
