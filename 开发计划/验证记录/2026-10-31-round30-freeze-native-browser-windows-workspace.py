from pathlib import Path
import json,hashlib,shutil
root=Path.cwd();ev=Path('/tmp/fc-round30-evidence');source=ev/'native-browser-windows-workspace-first.json'
report=json.loads(source.read_text());assert report['exitCode']==0 and not report['changedInputs']
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for name,sha in report['sourceAfter'].items():assert digest(root/name)==sha,name
frozen=Path('/tmp/fc-round30-native-browser-windows-workspace-snapshot');frozen.mkdir()
rows=[];bindings=[]
for line in (ev/'native-browser-windows-workspace-first.jsonl').read_text().splitlines():
 row=json.loads(line)
 if row.get('reason')!='compiler-artifact' or not row.get('profile',{}).get('test') or not row.get('executable'):continue
 p=Path(row['executable']);out=frozen/p.name
 assert not out.exists()
 original=next(a for a in report['artifacts'] if a['executable']==str(p));assert digest(p)==original['sha256']
 shutil.copyfile(p,out);assert digest(out)==original['sha256']==digest(p)
 bindings.append({'source':str(p),'frozen':str(out),'sha256':digest(out),'bytes':out.stat().st_size})
 row['executable']=str(out);rows.append(row)
assert len(rows)==20
normal=Path('/tmp/fc-round30-windows-head-stage-snapshot/fc-bootstrap.exe');assert digest(normal)=='bbfa4b3818ed4aa15abdfb879e4d2f8339f73b36730b8e8e83b6d8d926574d6a'
shutil.copyfile(normal,frozen/normal.name);assert digest(frozen/normal.name)==digest(normal)
with (frozen/'tests.jsonl').open('x') as f:
 for row in rows:f.write(json.dumps(row)+'\n')
with (ev/'native-browser-windows-workspace-frozen.json').open('x') as f:json.dump({'schemaVersion':1,'scope':'Frozen twenty current test programs; old frozen same-source normal diagnostic retained separately, not rebuilt here','compileReport':{'path':str(source),'sha256':digest(source)},'programs':bindings,'normal':{'path':str(frozen/normal.name),'sha256':digest(normal)},'testMetadataSha256':digest(frozen/'tests.jsonl'),'sourceBefore':report['sourceBefore'],'sourceAfter':report['sourceAfter']},f,indent=2);f.write('\n')
print(json.dumps({'programs':len(rows),'path':str(frozen),'normalSha256':digest(normal)}))
