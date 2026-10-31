from pathlib import Path
import json,hashlib,shutil,subprocess
p=Path('/tmp/fc-round28-evidence'); repo=Path('/workspace/共享/Eliora/未来教室计划');dest=repo/'开发计划/验证记录';prefix='2026-10-31-round28-'
records=[]
for source in sorted(p.iterdir()):
 if source.is_dir():
  assert source.name=='__pycache__',source
  continue
 assert source.is_file() and not source.is_symlink(),source
 assert 'private-key' not in source.name and not source.name.endswith(('.pem','.key','.pyc'))
 target=dest/(prefix+source.name);assert not target.exists(),target
 data=source.read_bytes();target.write_bytes(data)
 assert hashlib.sha256(target.read_bytes()).hexdigest()==hashlib.sha256(data).hexdigest()
 records.append({'originalPath':str(source),'archivedPath':str(target.relative_to(repo)),'sizeBytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
index={'schemaVersion':1,'scope':'Round28 original build/runtime/browser failures and final reports retained unchanged; source/input/binary SHA bindings, synthetic-only fixtures, no credentials or private keys; cleanup and staged review supplement separately','parentCommit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),'records':records}
path=dest/(prefix+'evidence-index.json');assert not path.exists();path.write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'records':len(records),'bytes':sum(r['sizeBytes'] for r in records),'index':str(path.relative_to(repo))}))
