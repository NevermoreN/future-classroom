from pathlib import Path
import json,hashlib,subprocess,argparse,datetime

p=argparse.ArgumentParser()
p.add_argument('mode',choices=['staged','committed'])
p.add_argument('--commit')
args=p.parse_args()
root=Path('/workspace/共享/Eliora/未来教室计划')
osroot=root/'开发计划/验证记录'
prefix='2026-11-01-round41-45-'
out=osroot/(prefix+args.mode+'-proof.json')
assert not out.exists()
def git(*s):return subprocess.check_output(['git',*s],cwd=root)
def sha(b):return hashlib.sha256(b).hexdigest()
required=set()
indexes=[]
for name in ['2026-11-01-round41-frontdoor-deadline','2026-11-01-round45-ci-input-domains']:
    base=osroot/name
    idx=json.loads((base/'INDEX.json').read_text())
    actual=set(x.relative_to(base).as_posix() for x in base.rglob('*') if x.is_file())
    expected={x['path'] for x in idx['files']}|{'INDEX.json'}
    assert actual==expected,(name,actual^expected)
    for row in idx['files']:
        b=(base/row['path']).read_bytes()
        assert len(b)==row['bytes'] and sha(b)==row['sha256'],row['path']
    required|={str((base/x).relative_to(root)) for x in expected}
    indexes.append({'path':str((base/'INDEX.json').relative_to(root)),'originals':len(idx['files']),'sha256':sha((base/'INDEX.json').read_bytes())})
if args.mode=='staged':
    assert not args.commit
    paths=git('diff','--cached','--name-only','-z','--diff-filter=ACMRT').decode().rstrip('\0').split('\0')
    assert required<=set(paths),required-set(paths)
    revision=''
else:
    assert args.commit and len(args.commit)==40
    staged=json.loads((osroot/(prefix+'staged-proof.json')).read_text())
    paths=[x['path'] for x in staged['files']]+[prefix+'staged-proof.json']
    paths[-1]='开发计划/验证记录/'+paths[-1]
    revision=args.commit
    assert required<=set(paths)
rows=[]
for name in paths:
    b=git('show',revision+':'+name)
    local=root/name
    assert local.is_file() and not local.is_symlink() and local.read_bytes()==b,name
    blob=git('rev-parse',revision+':'+name).decode().strip()
    rows.append({'path':name,'bytes':len(b),'sha256':sha(b),'gitBlob':blob})
r={'status':'PASS','recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'mode':args.mode,'commit':args.commit,'requiredArchiveFiles':len(required),'indexes':indexes,'files':rows,'scope':'Every INDEX member and INDEX itself is explicitly present in the staged/committed Git blobs, including ignored original logs and owner markers. Byte proof precedes removal of owned working copies.'}
out.write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'status':'PASS','mode':args.mode,'files':len(rows),'requiredArchiveFiles':len(required)}))
