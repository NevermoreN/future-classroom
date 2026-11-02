from pathlib import Path
import hashlib,json,zipfile,stat
R=Path('/tmp/fc-round80-controlled-repair')
DIRECTORIES=['setup-msvc-a470-monitor-first','setup-msvc-a470-monitor-second','setup-msvc-dispatch-second','a470-native-preparation-first','a470-native-preparation-second','a470-modern-native-runtime-first','a470-modern-native-retrieval-first','a470-win7-native-runtime-second','a470-integration-review-first']
ROOTFILES=['a470-download-metadata-repair-first.json','root-a470-msvc-native-two-targets-first.json','root-a470-native-ps-parse-first.json','root-a470-native-ps-parse-first.stdout.log','root-a470-native-ps-parse-first.stderr.log','root-a470-native-ps-parse-second.json','root-a470-native-ps-parse-second.stdout.log','root-a470-native-ps-parse-second.stderr.log','root-a470-merge-admission-first.json','a470-integration-local-first/report.json','owned-removal-native-preparation-first/run-native.py','native-reference-first/run-negative-msi-r79-third.py','native-reference-first/Capture.cs']
ZIP_TOP=['artifact-sha256.json','bootstrap.json','cleanup.json','executables.json','host.json','input-sha256.json','input-sha256-after.json','result.json','run-start.json','msvc-sdk.json']
LOG_PREFIX=['cargo-metadata','product-compile','lifecycle-compile','guard-library-compile','workspace-test-compile','clippy','fmt','architecture','guard-exports','cargo-version','rust-version','rust-target-cfg','source-clean-before','source-clean-after','source-sha','source-sha-after','source-git-tree-id-before','source-git-tree-id-after','source-git-tree-before','source-git-tree-after','visual-studio','cl-version','rust-src-components','rust-sysroot']
def sha(b):return hashlib.sha256(b).hexdigest()
def regular(p):
 assert p.is_file() and not p.is_symlink() and stat.S_ISREG(p.stat().st_mode)
 assert all(not q.is_symlink() for q in [p]+list(p.parents) if q==R or R in q.parents)
 return p

def collect():
 rows=[];identities=[]
 def addlocal(p,identity=False):
  p=regular(p);b=p.read_bytes();row={'path':str(p.relative_to(R)),'sourceType':'local','bytes':len(b),'sha256':sha(b)}
  (identities if identity or b.startswith(b'MZ') or p.suffix in ('.exe','.dll','.zip') else rows).append(row)
 for name in DIRECTORIES:
  root=R/name;assert root.is_dir() and not root.is_symlink()
  for p in sorted(root.iterdir()):
   if p.is_file():addlocal(p)
   elif p.name in ('inputs','upload','download'):
    for x in sorted(p.rglob('*')):
     assert not x.is_symlink()
     if x.is_file():addlocal(x)
   else:raise AssertionError('UNEXPECTED_SELECTED_DIR '+str(p))
 for name in ROOTFILES:addlocal(R/name)
 for label in ['modern','win7']:
  zp=R/f'a470-{label}-download-first/artifact.zip';addlocal(zp,True)
  for name in ['download-report-second.json','zip-inventory-second.json']:addlocal(zp.parent/name)
  for name in ['download-report.json','zip-inventory.json']:addlocal(zp.parent/name,True)
  zsha=sha(zp.read_bytes())
  with zipfile.ZipFile(zp) as z:
   names=z.namelist();assert len(names)==len(set(names))
   for n in names:
    selected=(n in ZIP_TOP or n=='rust-src-provenance.json' or n.startswith('logs/') and (n.endswith('.result.json') or any(n.startswith('logs/'+v+'.') for v in LOG_PREFIX) or n.startswith('logs/before-') or n.startswith('logs/after-')))
    # Identity-only executable and omitted duplicate broad input text still bind the official ZIP.
    identity=n.endswith(('.exe','.dll')) or n in ('input-domains.json','input-domains-after.json','input-git-tree.json','input-git-tree-after.json')
    if not selected and not identity:continue
    b=z.read(n);i=z.getinfo(n);row={'path':'official-zip/'+label+'/'+n,'sourceType':'zip-member','zipPath':str(zp.relative_to(R)),'zipSha256':zsha,'zipBytes':zp.stat().st_size,'member':n,'crc32':i.CRC,'bytes':len(b),'sha256':sha(b)}
    (rows if selected else identities).append(row)
 assert len({x['path'] for x in rows+identities})==len(rows)+len(identities)
 rows.sort(key=lambda x:x['path']);identities.sort(key=lambda x:x['path'])
 return rows,identities
if __name__=='__main__':
 rows,ids=collect();canonical={};stored=[]
 for r in rows:
  key=(r['bytes'],r['sha256']);r['storedPath']=canonical.setdefault(key,r['path'])
  if r['storedPath']==r['path']:stored.append(r)
 result={'status':'FINITE_SELECTED_ORIGINALS_WITH_EXACT_CONTENT_ALIASING','sourceRoot':str(R),'originals':rows,'identityOnly':ids,'selectedOriginalCount':len(rows),'selectedOriginalBytes':sum(x['bytes'] for x in rows),'storedCount':len(stored),'storedBytes':sum(x['bytes'] for x in stored),'selectorSha256':sha(Path(__file__).read_bytes()),'identityOnlyReason':'PE/ZIP binaries and four invalid first local metadata JSONs plus redundant giant CI input-domain/tree JSONs are not active archived text; original identities preserved. No credentials/scope/private key paths selected.'}
 out=Path(__file__).parent/'source-manifest-first.json';out.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k not in ('originals','identityOnly')}))
