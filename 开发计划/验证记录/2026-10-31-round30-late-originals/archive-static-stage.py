from pathlib import Path
import json,hashlib,shutil
src=Path('/tmp/fc-round30-evidence'); dst=Path.cwd()/'开发计划/验证记录'; prefix='2026-10-31-round30-'
index_path=dst/(prefix+'stage-progress-index.json'); index=json.loads(index_path.read_text()); existing={x['path']:x for x in index['files']}
for row in existing.values():
 data=(dst/row['path']).read_bytes(); assert len(data)==row['bytes'] and hashlib.sha256(data).hexdigest()==row['sha256']
pairs={
 'root-static-protocol-host-first.log':'static-protocol-host-stage1.log',
 'root-static-bootstrap-host-first.log':'static-bootstrap-host-stage1.log',
 'root-static-bootstrap-host-second.log':'static-bootstrap-host-stage2.log',
 'root-static-business-pure-first.json':'static-business-pure-stage1.json',
 'root-static-business-pure-second.json':'static-business-pure-stage2.json',
 'root-static-business-pure-third.json':'static-business-pure-stage3.json',
 'root-static-clippy-all-first.log':'static-clippy-all-stage1.log',
 'root-static-fmt-first.log':'static-fmt-stage1.log',
 'root-static-fmt-second.log':'static-fmt-stage2.log',
 'root-static-integration-source-checkpoint.json':'static-initial-source-checkpoint.json',
 'root-static-current-source-checkpoint.json':'static-current-source-checkpoint.json',
 'root-static-web-bundle-reuse.json':'static-web-bundle-reuse.json',
 'root-static-windows-check-first.log':'static-windows-check-stage1.log',
 'root-static-prelisten-check-first.log':'static-prelisten-check-stage1.log',
 'core-static-protocol-source-freeze.json':'static-protocol-source-freeze.json',
 'modern-static-stage-snapshot.json':'static-snapshot-stage2.json',
 'windows-static-before.json':'windows-static-before.json',
 'windows-static-stage-first.json':'windows-static-stage1.json',
 'windows-static-tls-first.json':'static-tls-stage1.json',
 'windows-static-startup-negative-first.json':'static-startup-negative-stage1.json',
 'windows-static-final-after.json':'windows-static-final-after.json',
 'windows-static-all-temp-final.json':'windows-static-all-temp-final.json',
 'run-static-startup-negative.py':'static-startup-negative-runner.py',
 'static-stage-summary.json':'static-stage-summary.json',
}
for n,stage in [('first','stage1'),('second','stage2')]:
 for ext in ['json','jsonl','diagnostics.log','stderr.log']:
  pairs[f'modern-static-integration-{n}.{ext}']=f'static-compile-{stage}.{ext}'
for stem,target in [('windows-static-tls-first','static-tls-stage1-raw'),('windows-static-startup-negative-first','static-startup-negative-stage1-raw')]:
 report=json.loads((src/(stem+'.json')).read_text());assert report['status']==report['cleanup']=='PASS'
 raw=Path(report['rawPreservation']['directory'])
 for file in sorted(raw.iterdir()):
  assert file.is_file(); pairs[str(file.relative_to(src))]=target+'/'+file.name
added=[]
for original,name in pairs.items():
 file=src/original; relative=prefix+name; to=dst/relative; data=file.read_bytes(); row={'path':relative,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
 if to.exists():assert to.read_bytes()==data,relative
 else:to.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(file,to)
 assert to.read_bytes()==data
 if relative in existing:assert existing[relative]==row
 else:index['files'].append(row);existing[relative]=row;added.append(relative)
index_path.write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'added':len(added),'indexed':len(index['files']),'bytesVerified':sum(x['bytes'] for x in index['files'])}))
