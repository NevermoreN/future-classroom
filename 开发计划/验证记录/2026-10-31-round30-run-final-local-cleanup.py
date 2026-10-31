from pathlib import Path
import json,hashlib,shutil,stat,base64
root=Path.cwd();base=Path('/tmp/fc-round30-evidence');binding=json.loads((base/'final-artifact-source-binding.json').read_text())
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for row in binding['artifacts']:assert digest(Path(row['path']))==row['sha256']
for name,row in binding['inputs'].items():assert digest(root/name)==row['sha256']
package=Path('/mnt/data/FutureClassroom-0.1.0-streaming-tls-dev.zip');assert digest(package)=='03f1f4ca2fa098688dedad32c5e3a274a21baba313ad2d1ff29c089b141caad6'
roots=[Path(p) for p in ['/tmp/fc-round30-core-target','/tmp/fc-round30-win-target','/tmp/fc-round30-final-win7-target','/tmp/fc-round30-native-browser-windows-workspace-snapshot','/tmp/fc-round30-final-win7-snapshot','/tmp/fc-round30-web-verified-dist']]
assert all(p.parent==Path('/tmp') and p.is_dir() and not p.is_symlink() for p in roots)
for p in roots[:3]:assert (p/'CACHEDIR.TAG').read_bytes().startswith(b'Signature: 8a477f597d28d172789f06886806bc55')
extras={}
for name in ['build-budget.json','THIRD-PARTY-NOTICES.txt']:
 p=roots[-1]/name;b=p.read_bytes();extras[name]={'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'base64':base64.b64encode(b).decode()}
assert (roots[-1]/'THIRD-PARTY-NOTICES.txt').read_bytes()==(root/'web/THIRD-PARTY-NOTICES.txt').read_bytes()
with (base/'final-web-extra-originals.json').open('x') as f:json.dump({'schemaVersion':1,'scope':'Original verified Web build metadata outside dist, preserved before removal','files':extras},f,indent=2);f.write('\n')
records=[]
for p in roots:
 files=dirs=links=total=0
 for q in p.rglob('*'):
  s=q.lstat()
  if stat.S_ISLNK(s.st_mode):links+=1
  elif stat.S_ISREG(s.st_mode):files+=1;total+=s.st_size
  elif stat.S_ISDIR(s.st_mode):dirs+=1
  else:raise RuntimeError('Unexpected special entry in owned tree')
 records.append({'path':str(p),'files':files,'directories':dirs,'symlinksUnlinkedWithoutFollowing':links,'bytes':total})
for row in records:
 p=Path(row['path']);shutil.rmtree(p);assert not p.exists();row['status']='REMOVED';print(json.dumps(row),flush=True)
report={'schemaVersion':1,'status':'PASS','scope':'All current round own build caches, test executable snapshots and verified Web build removed after input/artifact and completed-runtime checks; evidence originals retained pending archival only','deleted':records,'totalFiles':sum(r['files'] for r in records),'totalBytes':sum(r['bytes'] for r in records),'retainedDevelopmentZip':{'path':str(package),'sha256':digest(package)},'publicEvidenceRoot':str(base),'persistentSshAuthorization':'UNTOUCHED','persistentPrivateKey':'UNTOUCHED','sourceWorktree':'UNTOUCHED','actualCompletedProcessEvidence':['Host runner exit0','Windows20 runner exit0','Native Edge owner/Node/CDP actual cleanup PASS','Win7 build exit0','Win7 TLS runner exit0','final Windows read-only audit PASS']}
with (base/'final-local-cleanup.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps({'status':'PASS','totalFiles':report['totalFiles'],'totalBytes':report['totalBytes']}),flush=True)
