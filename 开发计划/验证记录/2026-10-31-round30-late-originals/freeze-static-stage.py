import pathlib,json,hashlib,shutil
root=pathlib.Path.cwd();ev=pathlib.Path('/tmp/fc-round30-evidence');prefix='modern-static-integration-second'
report=json.loads((ev/(prefix+'.json')).read_text())
assert report['exitCode']==0 and report['changedInputs']==[]
for rel,sha in report['sourceAfter'].items(): assert hashlib.sha256((root/rel).read_bytes()).hexdigest()==sha,rel
out=pathlib.Path('/tmp/fc-round30-windows-static-stage-snapshot');out.mkdir(exist_ok=False)
rows=[];artifacts=[];normal=None
for line in (ev/(prefix+'.jsonl')).read_text().splitlines():
 row=json.loads(line)
 if row.get('reason')!='compiler-artifact' or not row.get('executable'):continue
 src=pathlib.Path(row['executable']); expected=next(a for a in report['artifacts'] if a['executable']==str(src))
 assert hashlib.sha256(src.read_bytes()).hexdigest()==expected['sha256']
 test=row['profile']['test'];kind=row['target']['kind']
 if test and (kind not in (['lib'],['bin']) or row['target']['name'] not in ('fc_bootstrap','fc_windows','fc-bootstrap')):continue
 if not test and kind!=['bin']:continue
 dst=out/('fc-bootstrap.exe' if not test else src.name);shutil.copyfile(src,dst)
 assert hashlib.sha256(dst.read_bytes()).hexdigest()==expected['sha256']
 if test:
  row['executable']=str(dst);rows.append(row)
 else:normal=str(dst)
 artifacts.append({'path':str(dst),'sha256':expected['sha256'],'bytes':dst.stat().st_size,'test':test,'target':row['target']['name']})
assert len(rows)==3 and normal
(out/'tests.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
snapshot={'sourceCompile':prefix+'.json','changedInputs':[],'normal':normal,'testsMetadata':str(out/'tests.jsonl'),'artifacts':artifacts}
(ev/'modern-static-stage-snapshot.json').write_text(json.dumps(snapshot,indent=2)+'\n');print(json.dumps(snapshot))
