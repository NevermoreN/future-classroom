from pathlib import Path
import json,hashlib,subprocess
p=Path('/tmp/fc-round29-evidence');repo=Path.cwd();dest=repo/'开发计划/验证记录';prefix='2026-10-31-round29-';records=[]
for source in sorted(p.rglob('*')):
 if source.is_dir():continue
 assert source.is_file() and not source.is_symlink(),source
 rel=source.relative_to(p);assert '__pycache__' not in rel.parts and source.suffix not in {'.key','.pyc'} and 'private-key' not in source.name
 data=source.read_bytes()
 if source.suffix=='.pem':assert source.name=='tls-public.pem' and data.startswith(b'-----BEGIN CERTIFICATE-----') and b'PRIVATE KEY' not in data
 target=dest/(prefix+rel.parts[0]);target=target.joinpath(*rel.parts[1:]);assert not target.exists(),target;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(data)
 assert hashlib.sha256(target.read_bytes()).hexdigest()==hashlib.sha256(data).hexdigest()
 records.append({'originalPath':str(source),'archivedPath':str(target.relative_to(repo)),'sizeBytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
index={'schemaVersion':1,'scope':'Round29 original logs including build/lint failures, final exact-binary evidence and helpers retained unchanged; raw TLS files include only public owned certificate/metadata, no credentials or private keys; final cleanup/staged review supplement appended later','parentCommit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'records':records};out=dest/(prefix+'evidence-index.json');assert not out.exists();out.write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'records':len(records),'bytes':sum(x['sizeBytes'] for x in records),'index':str(out.relative_to(repo))}))
