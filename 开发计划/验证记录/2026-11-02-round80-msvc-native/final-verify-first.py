"""Read-only full final archive replay; writes only its new verification receipt."""
from pathlib import Path
import hashlib,json,tarfile,runpy
D=Path(__file__).resolve().parent
ns=runpy.run_path(str(D/'archive-controller-first.py'))
sha=ns['sha'];need=ns['need'];M=ns['M'];BACKEND=ns['BACKEND'];BACKEND_SHA=ns['BACKEND_SHA']
with tarfile.open(M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz','r:gz') as t:b=t.extractfile(BACKEND).read()
need(sha(b)==BACKEND_SHA,'BACKEND_IDENTITY');sc={'__name__':'scanner_only'};exec(compile(b,'<committed-scanner>','exec'),sc)
base=sc['text_views'];manifest=json.loads((D/'source-manifest-first.json').read_bytes());rows=manifest['originals'];stored=[r for r in rows if r['path']==r['storedPath']]
forms={r['path']:r for r in rows if r['path'].endswith(('source-git-tree-before.stdout.log','source-git-tree-after.stdout.log'))}
# Reuse the original strict structural recognizer with exactly the same19297 records.
import re
from pathlib import PurePosixPath
def views(data,label=None):
 prefix='final-replay:'
 r=forms.get(label[len(prefix):]) if isinstance(label,str) and label.startswith(prefix) else None
 if r is None:return base(data,label)
 need(len(data)==3185229 and sha(data)==r['sha256']=='62aa49cf18e1bb784dae312a746a7cb920dd9a670f20e8e3f02b5ab11248a51f','EXACT_TREE_IDENTITY')
 text=data.decode('utf8');records=text.split('\0');need(len(records)==19298 and records[-1]=='','STRICT_TREE_RECORD_COUNT');paths=[]
 for rec in records[:-1]:
  m=re.fullmatch(r'(100644|100755|120000|040000|160000) (blob|tree|commit) ([0-9a-f]{40}) +([0-9]+|-)\t([^\x00]+)',rec);need(m is not None,'STRICT_TREE_LONG_RECORD');need((m[1] in ('100644','100755','120000') and m[2]=='blob') or(m[1]=='040000' and m[2]=='tree')or(m[1]=='160000' and m[2]=='commit'),'MODE_TYPE');need((m[2]=='blob' and m[4].isdigit() and str(int(m[4]))==m[4])or(m[2]in('tree','commit')and m[4]=='-'),'OBJECT_SIZE');q=PurePosixPath(m[5]);need(not q.is_absolute()and all(p not in('','.','..') for p in q.parts)and not any(ord(c)<32 for c in m[5]),'SAFE_PATH');paths.append(m[5])
 need(len(set(paths))==19297 and not any(ord(c)<32 and c not in ('\0','\t') for c in text),'EXACT_TREE_CONTROLS')
 return [('utf8-exact-tree',text),('utf8-visible-boundary',text.replace('\0','[U+0000]')),('utf8-control-concatenated',text.replace('\0',''))]
sc['text_views']=views;stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews')};findings=[]
def scan(b,label):
 sc['credential_scan'](b,label,stats,findings);need(not findings,'SCAN_FINDINGS_PATH_KIND_ONLY '+json.dumps(findings))
archive=D/'evidence-originals-first.tar.gz';index=json.loads((D/'archive-index-first.json').read_bytes());need(sha(archive.read_bytes())==index['archive']['sha256'],'FINAL_ARCHIVE_SHA');contents={}
with tarfile.open(archive,'r:gz') as t:
 members=t.getmembers();need([m.name for m in members]==[r['path'] for r in stored],'FULL_MEMBER_SET')
 for member,r in zip(members,stored):
  need(member.isfile()and member.mode==0o600 and member.uid==member.gid==member.mtime==0 and not member.uname and not member.gname,'DETERMINISTIC_REGULAR_METADATA');b=t.extractfile(member).read();need(len(b)==r['bytes']and sha(b)==r['sha256'],'REPLAY_BYTES');contents[r['path']]=b;scan(b,'final-replay:'+r['path'])
for r in rows:need(len(contents[r['storedPath']])==r['bytes']and sha(contents[r['storedPath']])==r['sha256'],'ALL_SOURCE_ALIAS_RECONSTRUCTION')
files=[]
for p in sorted(D.rglob('*')):
 need(not p.is_symlink(),'NO_DESTINATION_SYMLINK')
 if not p.is_file()or p==archive:continue
 b=p.read_bytes();scan(b,'final-destination:'+str(p.relative_to(D)));files.append({'path':str(p.relative_to(D)),'bytes':len(b),'sha256':sha(b)})
r={'status':'PASS_FINAL_ALL_DESTINATION_TEXT_SCAN_AND_FULL_TAR_REPLAY','archive':index['archive'],'originalSourceAliasesReconstructable':len(rows),'replayedMembers':len(stored),'destinationTextsScanned':files,'scannerBackendSha256':BACKEND_SHA,'credentialScan':{'findings':[],'statistics':stats,'extraControlForm':'Only exact Git tree source bytes; strict19297 full long records; full raw/visible/concatenated scan.'},'Windows':False,'Cargo':False,'Git':False,'sourceDeletion':False}
b=ns['raw'](r);scan(b,'final-verification-receipt');(D/'final-verification-first.json').write_bytes(b);print(json.dumps({'status':r['status'],'receiptBytes':len(b),'receiptSha256':sha(b),'destinationTexts':len(files),'archiveSha256':index['archive']['sha256']}))
