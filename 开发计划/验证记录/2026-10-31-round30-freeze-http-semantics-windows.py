from pathlib import Path
import hashlib,json,shutil
root=Path.cwd();ev=Path('/tmp/fc-round30-evidence');source=ev/'http-semantics-windows-workspace-first.json';report=json.loads(source.read_text())
assert report['exitCode']==0 and report['changedInputs']==[]
for rel,sha in report['sourceAfter'].items():assert hashlib.sha256((root/rel).read_bytes()).hexdigest()==sha,rel
rows=[json.loads(line) for line in (ev/'http-semantics-windows-workspace-first.jsonl').read_text().splitlines()]
snapshot=Path('/tmp/fc-round30-windows-http-semantics-snapshot');snapshot.mkdir(exist_ok=False)
metadata=[];bindings=[];normal=None
for row in rows:
 if row.get('reason')!='compiler-artifact' or not row.get('executable'):continue
 is_test=row.get('profile',{}).get('test')
 is_normal=row['target']['name']=='fc-bootstrap' and row['target']['kind']==['bin'] and not is_test
 if not is_test and not is_normal:continue
 path=Path(row['executable']);sha=hashlib.sha256(path.read_bytes()).hexdigest();frozen=snapshot/path.name
 assert not frozen.exists();shutil.copyfile(path,frozen);assert hashlib.sha256(frozen.read_bytes()).hexdigest()==sha
 bindings.append({'original':str(path),'frozen':str(frozen),'bytes':frozen.stat().st_size,'sha256':sha,'test':is_test,'target':row['target']})
 if is_test:metadata.append(dict(row,executable=str(frozen)))
 else:normal=str(frozen)
assert len(metadata)==20 and normal is not None,(len(metadata),normal)
(snapshot/'tests.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in metadata))
binding={'scope':'Frozen current HTTP semantics compile; runtime not inferred','compileReport':str(source),'compileReportSha256':hashlib.sha256(source.read_bytes()).hexdigest(),'normal':normal,'testMetadata':str(snapshot/'tests.jsonl'),'files':bindings}
(ev/'http-semantics-windows-snapshot.json').write_text(json.dumps(binding,indent=2)+'\n');print(json.dumps({'normal':normal,'testPrograms':len(metadata)}))
