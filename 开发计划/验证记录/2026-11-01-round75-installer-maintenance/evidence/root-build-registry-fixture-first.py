"""Build/export/check only the independent registry metadata fixture, locally."""
from pathlib import Path
import datetime,hashlib,json,os,subprocess,time,uuid
R=Path('/tmp/fc-round75-installer-maintenance');E=R/'evidence';W=R/'worktree';T=R/'tools';P=E/'registry-keypath-fixture-preparation-first/registry-fixture.py'
assert (R/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n'
assert hashlib.sha256(P.read_bytes()).hexdigest()=='8c8591573ac399f75782871c4594012c39e45ef7336a2882fe4f9e7d5f8ebce0'
assert hashlib.sha256((E/'msi-tools-preparation-first.json').read_bytes()).hexdigest()=='4d09e249e49591e251481c898e742164cee9a11041a5104f828bf9409292079a'
joint=json.loads((E/'root-joint-source-freeze-second.json').read_bytes())
def guards():
 out=[]
 for row in joint['sources']:
  data=(W/row['path']).read_bytes();x=dict(path=row['path'],bytes=len(data),sha256=hashlib.sha256(data).hexdigest());assert x==row;out.append(x)
 return out
before=guards();nonce=uuid.uuid4().hex;directory=R/('registry-keypath-fixture-'+nonce);report=E/'root-build-registry-fixture-first.json';assert not directory.exists() and not report.exists()
v=dict(status='RUNNING',scope='Independent synthetic registry-only metadata fixture. No Windows/MSI install, production gate/Run or normal product action.',nonce=nonce,directory=str(directory),sourceBefore=before,startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),commands=[])
def save():report.write_text(json.dumps(v,indent=2)+'\n')
env=os.environ.copy();env['LD_LIBRARY_PATH']=str(T/'root/usr/lib/x86_64-linux-gnu');start=time.monotonic();save()
def run(name,cmd,cwd):
 assert time.monotonic()-start<120
 t=time.monotonic();p=subprocess.run(cmd,cwd=cwd,env=env,capture_output=True,timeout=min(20,120-(time.monotonic()-start)))
 row=dict(name=name,command=cmd,cwd=str(cwd),actualExit=True,exitCode=p.returncode,elapsedSeconds=time.monotonic()-t)
 for key,data in [('stdout',p.stdout),('stderr',p.stderr)]:
  path=E/('registry-fixture-'+name+'-first.'+key+'.log');assert not path.exists();path.write_bytes(data);row[key]=dict(path=str(path),bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
 v['commands'].append(row);save();assert p.returncode==0,(name,p.returncode);return p.stdout
try:
 run('author',['python3','-B',str(P),'--nonce',nonce,'author','--output-dir',str(directory)],R)
 run('build',[str(T/'wrappers/wixl'),'-a','x86','-o','fixture.msi','fixture.wxs'],directory)
 info=str(T/'root/usr/bin/msiinfo');msi=str(directory/'fixture.msi');tablelist=run('tables',[info,'tables',msi],directory);summary=run('summary',[info,'suminfo',msi],directory)
 tables=tablelist.decode().splitlines();assert len(tables)==len(set(tables)) and 1<=len(tables)<=64
 exports=directory/'exports';exports.mkdir()
 for table in tables:
  assert table and all(c.isascii() and (c.isalnum() or c=='_') for c in table)
  if table.startswith('_'):continue
  data=run('export-'+table,[info,'export',msi,table],directory);(exports/(table+'.idt')).write_bytes(data)
 check=run('check',['python3','-B',str(P),'--nonce',nonce,'check-exports','--exports',str(exports),'--tables-list',str(E/'registry-fixture-tables-first.stdout.log'),'--summary',str(E/'registry-fixture-summary-first.stdout.log')],directory)
 v['tableCheck']=json.loads(check);assert v['tableCheck']['status']=='PASS_STATIC_REGISTRY_ONLY_FIXTURE_TABLES'
 v['sourceAfter']=guards();assert v['sourceAfter']==before
 v['msi']={};data=(directory/'fixture.msi').read_bytes();v['msi']=dict(path=msi,bytes=len(data),sha256=hashlib.sha256(data).hexdigest());v['status']='PASS_LOCAL_REGISTRY_ONLY_FIXTURE_BUILD_AND_TABLES_RUNTIME_NOT_RUN'
except Exception as error:
 v['status']='FAIL_LOCAL_FIXTURE_BUILD_OR_TABLES';v['errorKind']=type(error).__name__;v['errorCode']=str(error) if isinstance(error,AssertionError) else 'LOCAL_COMMAND_OR_INPUT_FAILURE'
finally:v['elapsedSeconds']=time.monotonic()-start;save()
print(json.dumps({k:v.get(k) for k in ('status','nonce','directory','msi','elapsedSeconds','errorKind','errorCode')}));raise SystemExit(0 if v['status'].startswith('PASS') else 1)
