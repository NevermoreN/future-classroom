from pathlib import Path
import hashlib,json,shutil,os
root=Path.cwd();ev=Path('/tmp/fc-round36-public-desktop-evidence');out=ev/'local-owned-build-cleanup-final.json';assert not out.exists() and root==Path('/tmp/fc-round35-visuals-worktree')
for name in ['windows-integrated-ninth.json','windows-integrated-runtime-fourth.json','windows-public-native-third.json','windows-visual-native-integrated-third.json','windows-integrated-browser-continuous-third.json','package-independent-final.json']:
 d=json.loads((ev/name).read_text());assert d['status']=='PASS',name
 if 'cleanup' in d:assert d['cleanup']=='PASS',name
package=Path('/mnt/data/FutureClassroom-0.1.0-public-desktop-dev.zip');expected=json.loads((ev/'package-independent-final.json').read_text())['archive'];assert hashlib.sha256(package.read_bytes()).hexdigest()==expected['sha256']
owned=['fc-round35-msvc-original.zip','fc-round35-msvc-programs','fc-round35-msvc-reprobe','fc-round35-visuals-node-cache','fc-round35-visuals-programs','fc-round35-visuals-programs-second','fc-round35-visuals-target','fc-round35-visuals-web-fixture-second.zip','fc-round36-public-desktop-focused-native','fc-round36-public-desktop-native-first','fc-round36-public-desktop-native-second','fc-round36-public-desktop-native-third','fc-round36-public-desktop-programs-fourth','fc-round36-public-desktop-programs-fifth','fc-round36-public-desktop-programs-sixth','fc-round36-public-desktop-programs-seventh','fc-round36-public-desktop-programs-eighth','fc-round36-public-desktop-programs-ninth']
paths=[Path('/tmp')/n for n in owned]+[root/'web/node_modules',root/'web/dist',root/'web/build-budget.json']
r={'scope':'Remove only exact owned finished test/build paths, after whole Windows validation and package independent proof. Retain original evidence until committed byte verification, retain user SSH authorization/key and preexisting main target/global caches/intentional development ZIPs.','removed':[],'artifactIdentities':[],'keptDevelopmentPackage':expected}
for p in paths:
 assert not p.is_symlink(),str(p)
 if p.exists():
  files=[p] if p.is_file() else [x for x in p.rglob('*') if x.is_file() and not x.is_symlink()]
  for f in files:
   if f.suffix.lower() in {'.exe','.zip'}:
    r['artifactIdentities'].append({'path':str(f),'bytes':f.stat().st_size,'sha256':hashlib.sha256(f.read_bytes()).hexdigest()})
  r['removed'].append({'path':str(p),'fileCount':len(files),'bytes':sum(x.stat().st_size for x in files)})
  if p.is_dir():shutil.rmtree(p)
  else:p.unlink()
 assert not p.exists()
r['status']='PASS';out.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'status':'PASS','removedPaths':len(r['removed']),'removedBytes':sum(x['bytes'] for x in r['removed'])}))
