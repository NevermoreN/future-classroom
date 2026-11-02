from pathlib import Path
import argparse,hashlib,json,subprocess,tarfile,time,zipfile
R=Path('/tmp/fc-round80-controlled-repair')
def sha(b):return hashlib.sha256(b).hexdigest()
def identity(b):return dict(bytes=len(b),sha256=sha(b))
def safe(n):
 p=Path(n);assert not p.is_absolute()and '..'not in p.parts and p.as_posix()==n and n!='.';assert not any(ord(c)<32 for c in n);return p
ap=argparse.ArgumentParser();ap.add_argument('--archive',choices=['D16'],required=True);args=ap.parse_args()
name,pin='password-cng-archive-staged-first','d62e95105aabf2716bcca57411326807f0c6a4ac10eeacaccddaf282e491819c'
D=R/name;F=D/'freeze-first.json';assert sha(F.read_bytes())==pin;fr=json.loads(F.read_bytes());expected={x['path']:x for x in fr['files']}
actual={p.relative_to(D).as_posix()for p in D.rglob('*')if p.is_file()and p!=F};assert actual==set(expected)
for n,x in expected.items():
 p=D/safe(n);assert not p.is_symlink()and p.resolve().is_relative_to(D)and identity(p.read_bytes())=={k:x[k]for k in('bytes','sha256')}
manifest=json.loads((D/'source-manifest-first.json').read_bytes());index=json.loads((D/'archive-index-first.json').read_bytes());rows=manifest['originals'];assert len({x['path']for x in rows})==len(rows);zips={}
for x in manifest['zips']:
 p=R/safe(x['path']);assert not p.is_symlink()and p.resolve().is_relative_to(R)and identity(p.read_bytes())=={k:x[k]for k in('bytes','sha256')};zips[x['path']]=zipfile.ZipFile(p)
archive=D/index['archive']['path'];assert identity(archive.read_bytes())=={k:index['archive'][k]for k in('bytes','sha256')};total=0
with tarfile.open(archive,'r:gz')as tf:
 members=tf.getmembers();assert [x.name for x in members]==[x['path']for x in rows]
 for m,row in zip(members,rows):
  safe(m.name);assert m.isfile()and m.mode==0o600 and m.uid==m.gid==m.mtime==0 and not m.uname and not m.gname;raw=tf.extractfile(m).read();assert identity(raw)=={k:row[k]for k in('bytes','sha256')};source=row['source']
  if 'file'in source:
   p=R/safe(source['file']);assert p.is_file()and not p.is_symlink()and p.resolve().is_relative_to(R);original=p.read_bytes()
  else:safe(source['member']);original=zips[source['zip']].read(source['member'])
  assert original==raw;total+=len(raw)
  if row['path']in manifest['promote']:assert (D/row['path']).read_bytes()==raw
assert total==manifest['originalTextBytes']==index['originalTextBytes'];assert len(rows)==index['archive']['members'];assert len(manifest['binaryIdentities'])==index['binaryIdentityCount']
for z in zips.values():z.close()
# Use the archived frozen strict scanner for all UTF/JSON/JSONL/CLIXML/base64 views; log its actual run separately.
label='root-'+args.archive.lower()+'-strict-verify-first';argv=['python3','-B',str(D/'archive.py'),'--archive',args.archive,'--verify-only','--directory',str(D),'--expected-freeze-sha256',pin];t=time.monotonic()
with (R/(label+'.stdout.log')).open('xb')as so,(R/(label+'.stderr.log')).open('xb')as se:
 proc=subprocess.Popen(argv,stdout=so,stderr=se);exitcode=proc.wait(timeout=900)
assert exitcode==0;v=json.loads((R/(label+'.stdout.log')).read_bytes());assert v['status']=='PASS_VERIFY_ONLY_NO_WRITES'and v['credentialFindings']==0
out=dict(status='PASS_ROOT_FULL_ORIGINAL_EQUALITY_FROZEN_MEMBERS_AND_STRICT_SCAN',archive=args.archive,stagingRoot=str(D),freezeSha256=pin,freezeFiles=len(expected),members=len(rows),textBytes=total,binaryIdentities=len(manifest['binaryIdentities']),zipInputs=len(zips),tar=dict(path=str(archive),**identity(archive.read_bytes())),strictVerify=dict(argv=argv,actualExit=exitcode,originalReaped=proc.poll()is not None,seconds=time.monotonic()-t,stdout=dict(path=str(R/(label+'.stdout.log')),**identity((R/(label+'.stdout.log')).read_bytes())),stderr=dict(path=str(R/(label+'.stderr.log')),**identity((R/(label+'.stderr.log')).read_bytes()))),identityScope='Eight public synthetic fixture text copies are identity-only, not retained credentials. Original host binary identity comes from report; target was deleted and not read.',sourceDeletion=False,Windows=False,Git=False)
(R/('root-'+args.archive.lower()+'-archive-replay-first.json')).write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out))
