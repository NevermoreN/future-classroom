"""Default inert. Collect only completed frozen reader/probe objects; 74 run is deferred."""
from pathlib import Path
import argparse,hashlib,json,stat
R=Path('/tmp/fc-round80-controlled-repair');O=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-02-round80-installed-release-reader')
FIXED_DIRS=['installed-release-reader-implementation-first','installed-release-db-implementation-first','installed-release-db-implementation-second','installed-release-db-runtime-preparation-first','installed-release-db-runtime-first','installed-release-db-native-preparation-first','installed-release-db-native-preparation-second','installed-release-db-native-local-checks-first','installed-release-db-native-local-checks-second','installed-release-db-native-runtime-first']
ROOT_FILES=['root-installed-release-db-ps-parse-first.json','root-installed-release-db-ps-parse-first.stdout.log','root-installed-release-db-ps-parse-first.stderr.log','root-installed-release-db-native-ps-parse-first.json','root-installed-release-db-native-ps-parse-first.py','root-installed-release-db-native-ps-parse-first.stdin.ps1','root-installed-release-db-native-ps-parse-first.stdout.log','root-installed-release-db-native-ps-parse-first.stderr.log','root-installed-release-db-native-two-runs-review-first.json','installed-release-db-archive-preparation-first/d5-independent-review-first.json','installed-release-db-archive-preparation-first/fixed-source-manifest-first.json','installed-release-db-archive-preparation-first/scope-first.json','installed-release-db-archive-preparation-first/select-fixed-first.py']
FROZEN_SHA={'installed-release-reader-implementation-first':'bfa939335ba85600337eb4f443ee2128e4dd08e7a71c656d2b55b41f17937cb3','installed-release-db-implementation-first':'77d1c55d428e5bd8fbcf165a386afd967c453833aaf334e27862d359111f16ed','installed-release-db-implementation-second':'706931ddb5b6bb92c747a7c495513a4143f8f4f7d261fe68d210727cb7be7030'}
def sha(b):return hashlib.sha256(b).hexdigest()
def need(v,m):
 if not v:raise AssertionError(m)
def regular(p):
 need(p.is_relative_to(R) and p.resolve()==p,'EXACT_ROOT_REGULAR_PATH')
 for q in (p,*p.parents):
  if q==R or q.is_relative_to(R):need(not q.is_symlink(),'NO_SYMLINK')
 need(stat.S_ISREG(p.lstat().st_mode),'REGULAR');return p

def collect():
 rows=[];ids=[];excluded=[]
 def add(p):
  p=regular(p);rel=str(p.relative_to(R))
  need(not any(part in ['.git','target','target-first','sourcecopy.git','node_modules'] for part in p.relative_to(R).parts),'NO_WORKTREE_OR_CACHE_READ')
  if p.suffix.lower()=='.bin' or any(k in p.name.lower() for k in ['private-key','private_key','scope-raw','scope.raw','machine-scope','scope.bin']):
   excluded.append(dict(path=rel,policy='SECRET_OR_MACHINE_SCOPE_NOT_READ_OR_HASHED'));return
  b=p.read_bytes();need(len(b)<=96*1024*1024,'BOUNDED_FILE');r=dict(path=rel,bytes=len(b),sha256=sha(b));binary=p.suffix.lower() in ['.exe','.dll','.msi','.cab','.zip','.pdb','.gz','.tar'] or b.startswith((b'MZ',b'MSCF',b'PK\x03\x04',b'\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1'))
  (ids if binary else rows).append(r)
 def descend(p):
  need(p.is_dir() and not p.is_symlink(),'ONLY_EXPLICIT_COMPLETED_DIR')
  for q in sorted(p.iterdir()):
   need(not q.is_symlink(),'NO_SYMLINK')
   if q.is_dir():descend(q)
   else:add(q)
 for name in FIXED_DIRS:
  p=R/name
  if name in FROZEN_SHA:need(sha(regular(p/'freeze-first.json').read_bytes())==FROZEN_SHA[name],'FROZEN_IMPLEMENTATION_IDENTITY')
  elif 'native-local-checks' in name:pass
  elif '-preparation-' in name:need(json.loads(regular(p/'freeze-first.json').read_bytes())['status'].startswith('FROZEN_'),'FROZEN_RUNTIME_SOURCE_ONLY')
  elif name=='installed-release-db-runtime-first':need(sha(regular(p/'report.json').read_bytes())=='c477f6777ba72ac9d100370602fadc09e770b167e206818b0d539485ee25ce95','FIXED_ACTUAL_POSITIVE_REPORT')
  elif name=='installed-release-db-native-runtime-first':need(sha(regular(p/'report.json').read_bytes())=='353637662fd9fbb3ed02d90608439ab3678c172e5ac7d2317a36d69bf422f823','FIXED_ACTUAL_REGRESSION_REPORT')
  else:raise AssertionError('UNEXPECTED_SOURCE')
  descend(p)
 for name in ROOT_FILES:add(R/name)
 need(len(rows)+len(ids)<1024 and len({r['path'] for r in rows+ids})==len(rows)+len(ids),'FINITE_UNIQUE_SELECTION')
 return dict(status='FINITE_FROZEN_READER_AND_TERMINAL_POSITIVE_74_SOURCE_SELECTION',originals=sorted(rows,key=lambda r:r['path']),identityOnly=sorted(ids,key=lambda r:r['path']),excluded=excluded,deferred='NONE: root and controlled_owner terminal/stop-writing confirmations received; final runtime report fixed353637...',scope=dict(W3='frozen typed installed-release parser first failures and corrections',W4='frozen actual cache database reader first compiler/clippy failures and corrected checks;8pure rules',W5='cfgtest native probe two actual GNU producers first PE preserved; Summary buffer correction and corrected PE source; no normal product replacement',positive='fixed a470 official Win7MSVC normal product + new GNU cfgtest reader on actualWin11; only41actualMSIclose; noUpgrade authority',Win7OS='NOT_RUN',Win10OS='NOT_RUN',newMSVCReaderSource='NOT_VALIDATED'),sourceBase='a4702b56bff66da5f5cbe74551d8751eb33fa520',Windows=False,Cargo=False,Git=False)
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--collect-fixed',action='store_true');a=ap.parse_args()
 if not a.collect_fixed:print(json.dumps(dict(status='PREPARED_NOT_EXECUTED',Windows=False,Cargo=False,Git=False)));return
 value=collect();out=O/'source-manifest-first.json';need(not out.exists(),'NO_OVERWRITE');out.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n');print(json.dumps(dict(status=value['status'],originals=len(value['originals']),identityOnly=len(value['identityOnly']),scopeExcluded=len(value['excluded']),manifestSha256=sha(out.read_bytes()))))
if __name__=='__main__':main()
