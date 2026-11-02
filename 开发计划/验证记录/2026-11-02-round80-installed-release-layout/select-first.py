"""Default inert; explicit finished W6 domains only. No active cache or worktree scan."""
from pathlib import Path
import argparse,hashlib,json,stat
R=Path('/tmp/fc-round80-controlled-repair');D=Path(__file__).resolve().parent
DIRS=['installed-release-layout-implementation-first','installed-release-layout-runtime-preparation-first','installed-release-layout-runtime-first','installed-release-layout-native-preparation-first','installed-release-layout-native-local-checks-first','installed-release-layout-native-runtime-first','installed-release-layout-source-preservation-first']
ROOT_FILES=['root-installed-release-layout-ps-parse-first.json','root-installed-release-layout-ps-parse-first.py','root-installed-release-layout-ps-parse-first.stdin.ps1','root-installed-release-layout-ps-parse-first.stdout.log','root-installed-release-layout-ps-parse-first.stderr.log','root-installed-release-layout-native-ps-parse-first.json','root-installed-release-layout-native-ps-parse-first.py','root-installed-release-layout-native-ps-parse-first.stdin.ps1','root-installed-release-layout-native-ps-parse-first.stdout.log','root-installed-release-layout-native-ps-parse-first.stderr.log','installed-release-layout-archive-preparation-first/native-independent-review-first.json','installed-release-layout-archive-preparation-first/.owner','installed-release-layout-archive-preparation-first/positive-independent-review-first.json','installed-release-layout-archive-preparation-first/native-source-declarations-first.json']
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def regular(p):
 need(p.is_absolute() and p.is_relative_to(R) and p.resolve()==p,'CANONICAL_FINITE_R_PATH')
 for q in (p,*p.parents):
  if q==R or q.is_relative_to(R):need(not q.is_symlink(),'NO_SYMLINK')
 need(stat.S_ISREG(p.lstat().st_mode),'ONLY_REGULAR_SOURCE');return p

def collect(expected_native_sha):
 rows=[];ids=[];excluded=[]
 report=regular(R/'installed-release-layout-native-runtime-first/report.json');b=report.read_bytes();need(sha(b)==expected_native_sha and json.loads(b)['status']=='PASS_GNU_DEFAULT_CONTROLLED6_LIFECYCLE77_NATIVE_REGRESSION_WIN11_ONLY','ROOT_PINNED_TERMINAL77_REQUIRED')
 def add(p):
  p=regular(p);rel=str(p.relative_to(R));need(not any(x in p.relative_to(R).parts for x in ['.git','target','target-first','sourcecopy.git','node_modules']),'NO_BROAD_CACHE_OR_WORKTREE')
  if p.suffix.lower()=='.bin' or any(x in p.name.lower() for x in ['private-key','private_key','scope-raw','scope.raw','machine-scope','scope.bin']):excluded.append(dict(path=rel,policy='SECRET_OR_MACHINE_SCOPE_NOT_READ_OR_HASHED'));return
  b=p.read_bytes();need(len(b)<=96*1024*1024,'FINITE_FILE_BOUND');r=dict(path=rel,bytes=len(b),sha256=sha(b))
  binary=p.suffix.lower() in ['.exe','.dll','.msi','.cab','.zip','.pdb','.tar','.gz'] or b.startswith((b'MZ',b'MSCF',b'PK\x03\x04',b'\x7fELF',b'\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1'))
  (ids if binary else rows).append(r)
 def descend(p):
  need(p.is_dir() and not p.is_symlink(),'EXACT_FINISHED_DIRECTORY')
  for q in sorted(p.iterdir()):
   need(not q.is_symlink(),'NO_SYMLINK')
   if q.is_dir():descend(q)
   else:add(q)
 for name in DIRS:descend(R/name)
 for name in ROOT_FILES:add(R/name)
 need(len(rows)+len(ids)<1024 and len({r['path']for r in rows+ids})==len(rows)+len(ids),'UNIQUE_FINITE_SELECTION')
 return dict(status='FINITE_FINISHED_W6_LAYOUT_SOURCE_POSITIVE_AND77_SELECTION',expectedNativeReportSha256=expected_native_sha,sourceBase='a4702b56bff66da5f5cbe74551d8751eb33fa520',originals=sorted(rows,key=lambda r:r['path']),identityOnly=sorted(ids,key=lambda r:r['path']),excluded=excluded,scope='W6 compile/runtime originally independent a470WT+6frozen files;exact6 later sourcebranch saved1be5,parenta470;mainmerge/MSVCnewsource notclaimed;fixed a470MSVCproduct/newGNUcfgtest reader;positive74MSIcloses+separatelifecycle77 includes6;no upgrade/newMSVC/Win7OS/Win10OS claim',notTraversed=['W6/other worktrees or .git/sourcecopy','targets/tools/global caches/privatekey/secret machine scope','upstream D6 full tar;referenced by frozen identity','host/Reader active objects'])
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--collect',action='store_true');ap.add_argument('--expected-native-sha256');a=ap.parse_args()
 if not a.collect:print(json.dumps(dict(status='PREPARED_NOT_EXECUTED_WAIT_TERMINAL77_PIN')));return
 need(isinstance(a.expected_native_sha256,str) and len(a.expected_native_sha256)==64,'EXPLICIT_ROOT_NATIVE_PIN');m=collect(a.expected_native_sha256);out=D/'source-manifest-first.json';need(not out.exists(),'NEW_MANIFEST_ONLY');out.write_text(json.dumps(m,indent=2)+'\n');print(json.dumps(dict(originals=len(m['originals']),originalBytes=sum(x['bytes']for x in m['originals']),binaryIdentityOnly=len(m['identityOnly']),manifestSha256=sha(out.read_bytes()))))
if __name__=='__main__':main()
