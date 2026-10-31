from pathlib import Path
import subprocess,json,hashlib,zipfile
repo=Path('/workspace/共享/Eliora/未来教室计划');p=Path('/tmp/fc-round29-evidence')
exe=Path('/tmp/fc-round29-win/i686-pc-windows-gnu/release/fc-bootstrap.exe');dist=Path('/tmp/fc-round29-web-dist')
out=Path('/mnt/data/FutureClassroom-0.1.0-http1-tls-dev.zip');repeat=Path('/tmp/fc-round29-repack');repeat.mkdir(exist_ok=False)
assert (repo/'web/THIRD-PARTY-NOTICES.txt').read_bytes()==Path('/tmp/fc-round29-web-dist/THIRD-PARTY-NOTICES.txt').read_bytes()
for path,log in [(out,'package-build-final.log'),(repeat/out.name,'package-repack-final.log')]:
 with (p/log).open('w') as f:
  subprocess.run(['python3',str(repo/'scripts/package-probe.py'),'--exe',str(exe),'-o',str(path),'--target','i686-pc-windows-gnu','--toolchain','1.97.1','--web-dist',str(dist)],stdout=f,stderr=subprocess.STDOUT,check=True,cwd=repo)
assert out.read_bytes()==(repeat/out.name).read_bytes()
with zipfile.ZipFile(out) as z:
 assert z.testzip() is None
 manifest=json.loads(z.read('manifest.json'));assert len(z.namelist())==13 and len(manifest['files'])==12
 assert set(z.namelist())=={'manifest.json',*[x['path'] for x in manifest['files']]}
 for f in manifest['files']:
  data=z.read(f['path']);assert len(data)==f['size'] and hashlib.sha256(data).hexdigest()==f['sha256']
 assert z.read('fc-bootstrap.exe')==exe.read_bytes()
 for f in manifest['files']:
  if f['path'].startswith('web/') and f['path']!='web/THIRD-PARTY-NOTICES.txt':assert z.read(f['path'])==(dist/f['path'][4:]).read_bytes()
 report={'status':'PASS','path':str(out),'sizeBytes':out.stat().st_size,'sha256':hashlib.sha256(out.read_bytes()).hexdigest(),'exeSha256':hashlib.sha256(exe.read_bytes()).hexdigest(),'members':13,'payloads':12,'crc':'PASS','payloadSizesAndSha256':'PASS','independentSameBasenameRepack':'byte-identical','sourceInputSha256':json.loads((p/'source-inputs-final.json').read_text())['inputSha256'],'frontendManifestSha256':manifest['frontend']['manifest_sha256'],'files':manifest['files']}
(p/'package-verification-final.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k!='files'}))
