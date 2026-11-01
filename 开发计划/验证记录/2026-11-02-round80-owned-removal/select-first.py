from pathlib import Path
import hashlib,json,stat
R=Path('/tmp/fc-round80-controlled-repair')
TREES=['explicit-removal-preparation-first','explicit-removal-preparation-second','pending-readonly-preparation-first','pending-readonly-runtime-first','pending-readonly-inventory-review-first','pending-readonly-retrieval-first','owned-removal-implementation-first','owned-removal-semantic-review-first','owned-removal-semantic-review-third','owned-removal-lint-fix-first','owned-removal-structural-checks-first','owned-removal-source-preservation-first','guard-owned-removal-build-first','guard-owned-removal-build-second','guard-owned-removal-clippy-feature-first','guard-owned-removal-clippy-feature-second','guard-owned-removal-clippy-default-first','owned-removal-native-preparation-first','owned-removal-native-runtime-first','owned-removal-fixture-preparation-first','owned-removal-auxiliary-first','owned-removal-runtime-preparation-first','owned-removal-runtime-first','owned-removal-cleanup-preparation-first','owned-removal-cleanup-first','controlled-product-producer-second','controlled-product-bindings-second','controlled-product-runtime-second','setup-msvc-monitor-first','setup-msvc-monitor-second']
SINGLES=['check-owned-removal-candidate-first.py','check-pending-snapshot-ps-first.py','retrieve-pending-snapshot-first.py','build-controlled-product-second.py','package-controlled-product-second.py','owned-removal-rust-inputs-first.json','owned-removal-rust-inputs-second.json','owned-removal-fixture-inputs-first.json']
PREFIXES=['root-owned-removal-','root-pending-readonly-runtime-operation-first','root-pending-snapshot-ps-parse-first','root-controlled-product-build-second','root-controlled-product-package-second','root-controlled-product-second-final-binding-first','root-controlled-product-second-ps-parse-first','root-controlled-product-runtime-operation-second','owned-removal-candidate-sync-first','guard-owned-removal-build-first.outer','guard-owned-removal-build-second.outer','guard-owned-removal-clippy-feature-first.outer','guard-owned-removal-clippy-feature-second.outer','guard-owned-removal-clippy-default-first.outer']
PACKAGES=['owned-removal-package-first','controlled-product-package-second']
CI=['setup-msvc-modern-download-first/download-report.json','setup-msvc-modern-download-first/zip-inventory.json','setup-msvc-modern-download-first/reports/result.json','setup-msvc-modern-download-first/reports/host.json','setup-msvc-modern-download-first/reports/cleanup.json','setup-msvc-win7-download-first/download-report.json','setup-msvc-win7-download-first/zip-inventory.json','setup-msvc-win7-download-first/diagnostics/logs/clippy.active.json','setup-msvc-win7-download-first/diagnostics/logs/clippy.result.json','setup-msvc-win7-download-first/diagnostics/logs/clippy.stdout.log','setup-msvc-win7-download-first/diagnostics/logs/clippy.stderr.log']
DENY_DIRS={'target','target-first','deps','tools','node_modules','__pycache__','.git','checkout-52d6392-first','checkout-normal-33c11-first'}
def collect():
 paths=set();links=[];pruned=[];excluded=[]
 def one(p):
  if p.is_symlink():links.append({'path':p.relative_to(R).as_posix(),'target':str(p.readlink())});return
  assert p.is_file() and p.resolve()==p;paths.add(p.relative_to(R).as_posix())
 def walk(p):
  assert p.is_dir() and not p.is_symlink()
  for q in sorted(p.iterdir()):
   rel=q.relative_to(R).as_posix()
   if q.is_symlink():links.append({'path':rel,'target':str(q.readlink())})
   elif q.is_dir():
    if q.name in DENY_DIRS:pruned.append({'path':rel,'reason':'BUILD_CACHE_OR_TOOLS_NOT_ENTERED'})
    else:walk(q)
   elif rel=='setup-msvc-monitor-first/win7-job-125005771547-logs-first.json':excluded.append({'path':rel,'bytes':q.stat().st_size,'sha256':hashlib.sha256(q.read_bytes()).hexdigest(),'reason':'DUPLICATE_API_ANSI_LOG_OFFICIAL_COMPLETE_CLIPPY_DIAGNOSTIC_PRESERVED'})
   elif q.name in ['scope.bin','scope.hex']:excluded.append({'path':rel,'bytes':q.stat().st_size,'reason':'COMPILED_MACHINE_SCOPE_CONTENT_NOT_READ_OR_ARCHIVED'})
   else:one(q)
 for n in TREES:walk(R/n)
 for n in SINGLES:one(R/n)
 for q in sorted(R.iterdir()):
  if q.is_file() and any(q.name.startswith(v) for v in PREFIXES):one(q)
 for n in PACKAGES:
  p=R/n
  for q in sorted(p.iterdir()):
   if q.is_dir():pruned.append({'path':q.relative_to(R).as_posix(),'reason':'DERIVED_DUPLICATED_PAYLOAD_LIFECYCLE_BINARY_EXTRACTED_NOT_ENTERED'})
   elif q.is_file():one(q)
 for n in CI:one(R/n)
 # Normal source ZIP has an exact binary identity, never a tar payload.
 for q in (R/'controlled-product-input-second').iterdir():
  if q.is_file():one(q)
 # Large official CI ZIPs are not opened: official download-report has exact SHA.
 for name in ['setup-msvc-modern-download-first','setup-msvc-win7-download-first']:
  d=json.loads((R/name/'download-report.json').read_bytes());z=R/name/'artifact.zip';assert z.is_file() and not z.is_symlink() and z.stat().st_size==d['bytes'];excluded.append({'path':z.relative_to(R).as_posix(),'bytes':d['bytes'],'sha256':d['sha256'],'reason':'OFFICIAL_DOWNLOAD_REPORT_IDENTITY_ZIP_NOT_OPENED_OR_ARCHIVED'})
 return sorted(paths),links,pruned,excluded
