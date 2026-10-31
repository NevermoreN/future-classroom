from pathlib import Path
import json,hashlib,shutil
src=Path('/tmp/fc-round30-web-upgrade/dist'); out=Path('/tmp/fc-round30-browser-upgrade');out.mkdir(exist_ok=False)
manifest=json.loads((src/'fc-assets.json').read_text()); assert len(manifest['assets'])<=64
renames={a['path']:a['path'].replace('assets/','assets/previous-',1) for a in manifest['assets'] if a['path']!='index.html'}
for deployment in ['a','b']:
 root=out/deployment;root.mkdir();assets=[]
 for asset in manifest['assets']:
  data=(src/asset['path']).read_bytes();assert len(data)==asset['sizeBytes'] and hashlib.sha256(data).hexdigest()==asset['sha256']
  name=renames.get(asset['path'],asset['path']) if deployment=='a' else asset['path']
  if deployment=='a' and asset['path'].endswith(('.html','.js','.css')):
   for previous,current in renames.items():data=data.replace(Path(previous).name.encode(),Path(current).name.encode())
  dest=root/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
  assets.append({'path':name,'sizeBytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
 doc={**manifest,'assets':assets};(root/'fc-assets.json').write_text(json.dumps(doc,indent=2)+'\n')
 (Path('/tmp/fc-round30-evidence')/('browser-upgrade-'+deployment+'-manifest.json')).write_text(json.dumps(doc,indent=2)+'\n')
assert set(renames).isdisjoint(renames.values())
report={'scope':'Synthetic two-deployment static HTTPS fixture; a renames real production asset URLs (only byte substitutions) to previous-* and recomputes manifest, b is exact new production build; no real MSI or Rust static host acceptance','productionManifestSha256':hashlib.sha256((src/'fc-assets.json').read_bytes()).hexdigest(),'renamedPreviousPaths':renames,'source':str(src),'ownedRoot':str(out)}
Path('/tmp/fc-round30-evidence/browser-upgrade-fixture-binding.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
