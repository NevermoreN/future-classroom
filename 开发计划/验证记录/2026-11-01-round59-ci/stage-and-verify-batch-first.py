from pathlib import Path
import json,hashlib,subprocess
R=Path.cwd();E=Path('/tmp/fc-round59-ci-evidence');m=json.loads((E/'batch-inventory-second.json').read_text());expected={x['path'] for x in m['files']};assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==m['sourceHead']
raw=subprocess.check_output(['git','status','--porcelain','-z','--untracked-files=all']);changed={x[3:].decode() for x in raw.split(b'\0') if x};unexpected=changed-expected
assert not unexpected,('unexpected paths',sorted(unexpected)[:8])
missing=expected-changed
checked=subprocess.run(['git','check-ignore','-z','--stdin'],input=b''.join(x.encode()+b'\0' for x in sorted(missing)),capture_output=True);ignored={x.decode() for x in checked.stdout.split(b'\0') if x}
assert ignored==missing,('unexplained missing paths',len(missing-ignored))
assert not subprocess.check_output(['git','diff','--cached','--name-only','-z'])
first={'status':'FIRST_GUARD_REJECTED_IGNORED_ARCHIVE_LOGS_BEFORE_ANY_STAGING','observedStatusPaths':len(changed),'manifestPaths':len(expected),'explicitlyIgnoredArchivePaths':len(ignored),'scope':'Inherited *.log ignore rules excluded original evidence from git status. No source/compiler/runtime failure; no mutation occurred in the first guard. Next step force-adds only exact pre-reviewed manifest paths.'}
with (E/'staging-guard-first-failure.json').open('x') as f:json.dump(first,f,indent=2);f.write('\n')
subprocess.run(['git','add','-f','--',*[x['path'] for x in m['files']]],check=True)
staged={x.decode() for x in subprocess.check_output(['git','diff','--cached','--name-only','-z']).split(b'\0') if x};assert staged==expected,('staged mismatch',len(staged),len(expected))
subprocess.run(['git','diff','--cached','--check'],check=True)
entries={}
for row in subprocess.check_output(['git','ls-files','-s','-z']).split(b'\0'):
 if not row:continue
 info,name=row.split(b'\t',1);mode,oid,stage=info.split();assert stage==b'0';entries[name.decode()]=oid.decode()
proc=subprocess.Popen(['git','cat-file','--batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE);count=0;total=0
for row in m['files']:
 proc.stdin.write((entries[row['path']]+'\n').encode());proc.stdin.flush();header=proc.stdout.readline().split();assert len(header)==3 and header[1]==b'blob';size=int(header[2]);b=proc.stdout.read(size);assert len(b)==size and proc.stdout.read(1)==b'\n';assert size==row['bytes'] and hashlib.sha256(b).hexdigest()==row['sha256'],row['path'];count+=1;total+=size
proc.stdin.close();assert proc.wait(timeout=10)==0
r={'status':'PASS_EXACT_STAGED_BLOBS','base':m['sourceHead'],'count':count,'bytes':total,'sourceInventorySha256':hashlib.sha256((E/'batch-inventory-second.json').read_bytes()).hexdigest(),'explicitForceAddedIgnoredOriginalLogs':len(ignored),'scope':'All825 staged source/config/plan/archive blobs match captured original bytes. No binary or credential is included. No commit/push/CI yet.'}
with (E/'staged-bytes-first.json').open('x') as f:json.dump(r,f,indent=2);f.write('\n')
print(json.dumps(r))
