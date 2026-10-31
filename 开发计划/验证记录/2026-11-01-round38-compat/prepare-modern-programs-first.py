from pathlib import Path,PureWindowsPath
import json,zipfile,hashlib,subprocess,runpy,re
root=Path.cwd();ev=Path('/tmp/fc-round37-ci-evidence');base=ev/'modern-artifact-raw';dest=Path('/tmp/fc-round37-modern-programs');out=ev/'modern-programs-independent-first.json';assert not dest.exists() and not out.exists();dest.mkdir()
sha=lambda data:hashlib.sha256(data).hexdigest();rows=json.loads((base/'executables.json').read_text(encoding='utf-8-sig'));assert len(rows)==22
r={'status':'RUNNING','scope':'Original source b76ad73 modern MSVC CI all22EXEs independently SHA/PE32/static CRT audited before supplemental Win11 runtime. Not Win7/Win10 runtime evidence or R38 post-fix binary.','sourceSha':'b76ad738100a722ee5c23804a489644a93593a1f','artifacts':[]};out.write_text(json.dumps(r,indent=2)+'\n')
metadata=[];audit=runpy.run_path(str(root/'scripts/audit-pe.py'))['audit_pe']
with zipfile.ZipFile('/tmp/fc-round37-modern-original.zip') as z:
 for row in rows:
  name=Path(row['relative_path']).name;assert re.fullmatch(r'[A-Za-z0-9_-]+\.exe',name);p=dest/name;assert not p.exists();data=z.read(row['relative_path']);assert len(data)==row['bytes'] and sha(data)==row['sha256'];p.write_bytes(data)
  pe=audit(p);assert not pe['structural_errors'];dlls=list(pe['imports']);assert not any(d.startswith(('vcruntime','msvcp','api-ms-win-crt','libgcc_s_','libstdc++','libwinpthread')) or d=='ucrtbase.dll' or re.fullmatch(r'msvcr[0-9]+\.dll',d) for d in dlls)
  r['artifacts'].append({**row,'localPath':str(p),'independentPeAudit':pe})
byname={Path(x['relative_path']).name:x for x in rows if x['role']=='tests'}
for line in (base/'logs/workspace-test-compile.stdout.log').read_text(encoding='utf-8-sig').splitlines():
 try:row=json.loads(line)
 except json.JSONDecodeError:continue
 if row.get('reason')!='compiler-artifact' or not row.get('executable') or not row.get('profile',{}).get('test'):continue
 name=PureWindowsPath(row['executable']).name;assert name in byname;row['executable']=str(dest/name);metadata.append(row)
assert len(metadata)==len(byname)==21
p=ev/'modern-test-metadata-first.jsonl';assert not p.exists();p.write_text(''.join(json.dumps(x)+'\n' for x in metadata))
inputs=json.loads((base/'input-sha256.json').read_text(encoding='utf-8-sig'));head=r['sourceSha'];paths={p.decode() for p in subprocess.check_output(['git','ls-tree','-r','--name-only','-z',head]).split(b'\0') if p};assert {x['path'] for x in inputs}==paths
p=subprocess.Popen(['git','cat-file','--batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE);direct=0;crlf=0
for x in inputs:
 p.stdin.write((head+':'+x['path']+'\n').encode());p.stdin.flush();h=p.stdout.readline().decode().strip().split();assert len(h)==3 and h[1]=='blob';body=p.stdout.read(int(h[2]));assert p.stdout.read(1)==b'\n'
 if len(body)==x['bytes'] and sha(body)==x['sha256']:direct+=1;continue
 assert b'\r' not in body and b'\0' not in body;body=body.replace(b'\n',b'\r\n');assert len(body)==x['bytes'] and sha(body)==x['sha256'],x['path'];crlf+=1
p.stdin.close();assert p.wait()==0
r.update(status='PASS',testExecutables=21,totalExecutables=22,completeCiInputSet=len(inputs),directGitBlobMatches=direct,exactWindowsCrlfCheckoutMatches=crlf,sourceComparison='Exact Git blob or exact LF-to-CRLF Windows checkout only; originals unchanged, converted text not called raw Git-blob identity.');out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:r[k] for k in ['status','testExecutables','totalExecutables','completeCiInputSet','directGitBlobMatches','exactWindowsCrlfCheckoutMatches']}))
