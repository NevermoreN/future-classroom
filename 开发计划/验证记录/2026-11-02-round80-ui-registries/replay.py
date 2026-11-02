"""Read-only standalone D12 byte replay. Optional current-original comparison."""
from pathlib import Path
import argparse,hashlib,json,tarfile,stat

def sha(raw):return hashlib.sha256(raw).hexdigest()
def file_sha(p):
 h=hashlib.sha256()
 with p.open('rb')as f:
  for b in iter(lambda:f.read(1048576),b''):h.update(b)
 return h.hexdigest()
def main():
 a=argparse.ArgumentParser();a.add_argument('--root',type=Path,default=Path(__file__).resolve().parent);a.add_argument('--freeze-sha256',required=True);a.add_argument('--compare-current',action='store_true');v=a.parse_args();O=v.root;freeze=O/'freeze-first.json';assert file_sha(freeze)==v.freeze_sha256;f=json.loads(freeze.read_bytes());actual={p.relative_to(O).as_posix()for p in O.rglob('*')if p.is_file()};assert actual=={x['path']for x in f['files']}|{'freeze-first.json'}
 for x in f['files']:
  p=O/x['path'];assert p.is_file()and not p.is_symlink()and p.stat().st_size==x['bytes']and file_sha(p)==x['sha256']
 idx=json.loads((O/'archive-index-first.json').read_bytes());archive=O/idx['archive']['path'];assert archive.stat().st_size==idx['archive']['bytes']and file_sha(archive)==idx['archive']['sha256'];expected={x['path']:x for x in idx['members']};assert len(expected)==len(idx['members']);seen=set()
 with tarfile.open(archive,'r:gz')as t:
  for m in t:
   assert m.isfile()and m.name not in seen and not m.name.startswith('/')and all(x not in['','.','..']for x in m.name.split('/'))and m.name in expected;seen.add(m.name);x=expected[m.name];b=t.extractfile(m).read();assert m.size==x['bytes']and len(b)==x['bytes']and sha(b)==x['sha256']
 assert seen==set(expected);sources=json.loads((O/'source-index-first.json').read_bytes());assert file_sha(O/'source-index-first.json')==idx['originalSourceIndexSha256'];text={x['path']:x for x in sources['files']if x['kind']=='ORIGINAL_TEXT'};assert set(text)==set(expected)
 for path,x in text.items():assert expected[path]['bytes']==x['bytes']and expected[path]['sha256']==x['sha256']
 if v.compare_current:
  for x in sources['files']:
   p=Path(x['originalPath']);s=p.lstat();assert stat.S_ISREG(s.st_mode)and not p.is_symlink()and s.st_size==x['bytes']
   if x['kind']!='SENSITIVE_METADATA_ONLY_NOT_READ':assert file_sha(p)==x['sha256']
 scanner=json.loads((O/'scanner-first.json').read_bytes());assert scanner['hitCount']==0 and scanner['hits']==[]
 print(json.dumps(dict(status='PASS_COMPLETE_ARCHIVE_FREEZE_ALL_MEMBERS_AND_IDENTITIES_REPLAY',archiveSha256=idx['archive']['sha256'],members=len(expected),binaryIdentities=len(idx['binaryIdentities']),sensitiveMetadataOnly=len(idx['sensitiveMetadataOnly']),currentOriginalsCompared=v.compare_current,sensitiveContentRead=False,allFrozenFiles=len(f['files']),nestedFreezeIncluded=True)))
if __name__=='__main__':main()
