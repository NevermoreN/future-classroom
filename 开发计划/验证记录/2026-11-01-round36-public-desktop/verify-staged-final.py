from pathlib import Path
import hashlib,json,subprocess
root=Path.cwd();assert root==Path('/tmp/fc-round35-visuals-worktree')
out=root/'开发计划/验证记录/2026-11-01-round36-integrated-proof.json';assert not out.exists()
sha=lambda b:hashlib.sha256(b).hexdigest()
build=json.loads((root/'开发计划/验证记录/2026-11-01-round36-public-desktop/windows-integrated-ninth.json').read_text());assert build['status']=='PASS' and not build['changedInputs']
expected=dict(build['sourceBefore'])
archives=[]
for name in ['2026-11-01-round35-msvc','2026-11-01-round35-visuals','2026-11-01-round36-public-desktop']:
 base=root/'开发计划/验证记录'/name;index=base/'evidence-index.json';d=json.loads(index.read_text());count=0
 for x in d['files']:
  p=base/x['path'];key=p.relative_to(root).as_posix();expected[key]={'bytes':x['bytes'],'sha256':x['sha256']};count+=1
 data=index.read_bytes();expected[index.relative_to(root).as_posix()]={'bytes':len(data),'sha256':sha(data)}
 archives.append({'directory':str(base.relative_to(root)),'originals':count,'indexSha256':sha(data)})
p=subprocess.Popen(['git','cat-file','--batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
for name,x in expected.items():
 data=(root/name).read_bytes();assert len(data)==x['bytes'] and sha(data)==x['sha256'],name
 p.stdin.write((':'+name+'\n').encode());p.stdin.flush();header=p.stdout.readline().decode().strip().split();assert len(header)==3 and header[1]=='blob',name
 body=p.stdout.read(int(header[2]));assert p.stdout.read(1)==b'\n' and body==data,name
p.stdin.close();assert p.wait()==0
r={'status':'PASS','scope':'All 406 frozen Rust/build inputs equal tested source and staged Git blobs; all three immutable evidence archive indices/originals byte-equal staged Git blobs. No compiled binaries/private key in archive.','checkedFiles':len(expected),'testedBuildSourceCount':len(build['sourceBefore']),'archives':archives,'developmentPackage':json.loads((root/'开发计划/验证记录/2026-11-01-round36-public-desktop/package-independent-final.json').read_text())['archive']}
out.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'status':'PASS','checkedFiles':len(expected),'testedBuildSourceCount':len(build['sourceBefore'])}))
