from pathlib import Path
import json,hashlib,subprocess
root=Path('/tmp/fc-round47-https-settings-worktree');ev=Path('/tmp/fc-round50-https-integration-evidence');ui=Path('/tmp/fc-round49-https-settings-ui-evidence')
assert json.loads((ev/'native-service-second.json').read_text())['status']!='RUNNING'
freeze=ui/'ui-source-freeze-first.json';assert hashlib.sha256(freeze.read_bytes()).hexdigest()=='1c4906caf79f20224751a6c1e8b0bf754bcb5488168824380d4815778a92442e'
d=json.loads(freeze.read_text());snapshot=Path(d['snapshot']);assert len(d['ownedFiles'])==9
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==d['base']
def sha(b):return hashlib.sha256(b).hexdigest()
def snap():
 names=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=root).decode().split('\0')
 return {n:{'bytes':(root/n).stat().st_size,'sha256':sha((root/n).read_bytes())} for n in names if n and n.startswith('crates/') and (n.endswith('.rs') or n.endswith('/Cargo.toml'))}
before=snap()
for row in d['ownedFiles']:
 p=root/row['path'];source=snapshot/row['path'];b=source.read_bytes();assert len(b)==row['bytes'] and sha(b)==row['sha256']
 if row['kind']=='modified':assert p.read_bytes()==subprocess.check_output(['git','show','HEAD:'+row['path']],cwd=root)
 else:assert row['kind']=='new' and not p.exists()
for row in d['ownedFiles']:
 p=root/row['path'];p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes((snapshot/row['path']).read_bytes())
after=snap();changed=sorted(n for n in before.keys()|after.keys() if before.get(n)!=after.get(n));assert changed==sorted(row['path'] for row in d['ownedFiles'])
result={'status':'SOURCE_INTEGRATED_FINAL_MATRIX_REQUIRED','root':str(root),'base':d['base'],'uiFreezeSha256':sha(freeze.read_bytes()),'ownedFiles':d['ownedFiles'],'changedExactlyOwned9':changed,'otherCoreNativeSourceUnchanged':True,'rustInputsBefore':len(before),'rustInputsAfter':len(after),'before':before,'after':after,'scope':'Final R47 core fourth + R48 native/service second + R50 one-method Windows binding fix + frozen R49 UI9. No readonly dependency copies imported.'}
p=ev/'ui-integration-first.json';assert not p.exists();p.write_text(json.dumps(result,indent=2)+'\n');print('UI_INTEGRATED_EXACT_OWNED9')
