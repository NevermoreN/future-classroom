from pathlib import Path
import subprocess,json,hashlib,base64,tempfile,zipfile,time,os
root=Path.cwd();ev=Path('/tmp/fc-round30-evidence');normal=Path('/tmp/fc-round30-native-browser-windows-workspace-snapshot/fc-bootstrap.exe');web=Path('/tmp/fc-round30-web-verified-dist/dist');output=Path('/mnt/data/FutureClassroom-0.1.0-streaming-tls-dev.zip')
def sha(b):return hashlib.sha256(b).hexdigest()
def snapshot():
 paths=[]
 for parent in ['crates','web','scripts','packaging','.github']:
  for p in (root/parent).rglob('*'):
   if p.is_file() and not any(x in p.parts for x in ['node_modules','target','dist','__pycache__']):paths.append(p)
 paths += [root/p for p in ['Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml','README.md','LICENSE','AGENTS.md']]
 return {str(p.relative_to(root)):{'bytes':p.stat().st_size,'sha256':sha(p.read_bytes())} for p in sorted(set(paths)) if p.is_file()}
before=snapshot();assert sha(normal.read_bytes())=='bbfa4b3818ed4aa15abdfb879e4d2f8339f73b36730b8e8e83b6d8d926574d6a'
command=['python3','-B','scripts/package-probe.py','--exe',str(normal),'--output',str(output),'--target','i686-pc-windows-gnu','--toolchain','Rust 1.97.1 / i686-pc-windows-gnu; exact build/runtime binding recorded separately','--web-dist',str(web)]
r=subprocess.run(command,capture_output=True,timeout=60);runs=[{'command':command,'exitCode':r.returncode,**{k:{'bytes':len(v),'sha256':sha(v),'base64':base64.b64encode(v).decode()} for k,v in [('stdout',r.stdout),('stderr',r.stderr)]}}]
assert r.returncode==0,r.stderr.decode()
with tempfile.TemporaryDirectory(prefix='fc-round30-final-repack-') as temporary:
 second=Path(temporary)/output.name;recommand=command.copy();recommand[recommand.index('--output')+1]=str(second)
 r=subprocess.run(recommand,capture_output=True,timeout=60);runs.append({'command':recommand,'exitCode':r.returncode,**{k:{'bytes':len(v),'sha256':sha(v),'base64':base64.b64encode(v).decode()} for k,v in [('stdout',r.stdout),('stderr',r.stderr)]}});assert r.returncode==0
 assert output.read_bytes()==second.read_bytes()
with zipfile.ZipFile(output) as z:
 assert z.testzip() is None;manifest=json.loads(z.read('manifest.json'));entries=manifest['files'];assert len(z.namelist())==len(entries)+1
 for e in entries: b=z.read(e['path']);assert len(b)==e['size'] and sha(b)==e['sha256']
 assert manifest['sha256']=='bbfa4b3818ed4aa15abdfb879e4d2f8339f73b36730b8e8e83b6d8d926574d6a'
 assert manifest['status']=='NOT_VALIDATED' and manifest['frontend']['asset_count']==6 and manifest['frontend']['asset_bytes']==452054
 assert manifest['frontend']['manifest_sha256']=='1dfa13904bbbdf661b182e055944c0b6ead4cd91f5c2618a6ee0b6fe9d1271ee'
after=snapshot();report={'schemaVersion':1,'status':'PASS','scope':'Minimal development probe ZIP with current exact tested modern EXE and verified Web payload; not full application or release','path':str(output),'bytes':output.stat().st_size,'sha256':sha(output.read_bytes()),'crc':'PASS','allPayloadBytesAndSha256':'PASS','members':len(entries)+1,'payloads':len(entries),'independentSameBasenameRepack':'BYTE_IDENTICAL','repackTemporaryDirectory':'REMOVED','sourceBefore':before,'sourceAfter':after,'changedInputs':[k for k in set(before)|set(after) if before.get(k)!=after.get(k)],'runs':runs,'manifest':manifest}
with (ev/'final-modern-package-first.json').open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
assert not report['changedInputs']
print(json.dumps({k:report[k] for k in ['status','path','bytes','sha256','members','payloads','changedInputs']}),flush=True)
