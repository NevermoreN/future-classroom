"""Build and inspect the isolated observer fixture locally; never execute MSI."""
from pathlib import Path
import datetime, hashlib, json, os, subprocess, time
R=Path('/tmp/fc-round75-installer-maintenance');E=R/'evidence';W=R/'worktree';T=R/'tools'
P=E/'parent-context-probe-fourth/context-probe.py'
assert (R/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n'
assert hashlib.sha256(P.read_bytes()).hexdigest()=='93c87fef4ca04524e47c77c96d200274d2ebb6a3e918977244fea4ecf6bde979'
proof=E/'gui-cli-list-first.json';cli=json.loads(proof.read_bytes())
assert cli['status']=='ACTUAL_HARNESS_CLI_PARSE_PASS_NOT_OBSERVER_RUNTIME' and cli['cleanup']=='PASS'
nonce=cli['nonce'];directory=R/('parent-context-fixture-'+nonce);report=E/'root-build-context-probe-first.json'
assert not directory.exists() and not report.exists()
ledger=R/'gui-test-fixture-f8bd373d36224240a4aeb4ae49b76f96/fixture-ledger.json'
joint=json.loads((E/'root-joint-source-freeze-second.json').read_bytes())
def guards():
 rows=[]
 for row in joint['sources']:
  data=(W/row['path']).read_bytes();x=dict(path=row['path'],bytes=len(data),sha256=hashlib.sha256(data).hexdigest());assert x==row;rows.append(x)
 return rows
before=guards();v=dict(status='RUNNING',scope='Independent GUI test-only Type2 observer then intentional Type19 abort before Initialize; local build/static only, no Windows/production MSI/Pending.',nonce=nonce,directory=str(directory),sourceBefore=before,commands=[],startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
env=os.environ.copy();env['LD_LIBRARY_PATH']=str(T/'root/usr/lib/x86_64-linux-gnu');start=time.monotonic()
def save():report.write_text(json.dumps(v,indent=2)+'\n')
def run(name,cmd,cwd,binary=None):
 assert time.monotonic()-start<120
 t=time.monotonic();c=subprocess.run(cmd,cwd=cwd,env=env,capture_output=True,timeout=min(20,120-(time.monotonic()-start)))
 row=dict(name=name,command=cmd,cwd=str(cwd),actualExit=True,exitCode=c.returncode,elapsedSeconds=time.monotonic()-t)
 for key,data in [('stdout',c.stdout),('stderr',c.stderr)]:
  path=binary if key=='stdout' and binary else E/('context-probe-'+name+'-first.'+key+'.log')
  assert not path.exists();path.write_bytes(data);row[key]=dict(path=str(path),bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
 v['commands'].append(row);save();assert c.returncode==0,(name,c.returncode);return c.stdout
base=['python3','-B',str(P),'--observer-test-name',cli['selectedTests'][0],'--windows-report-root',cli['remoteRoot'],'--nonce',nonce]
fixture=json.loads(ledger.read_bytes())['guiFixture']
bindings=['--observer-sha256',fixture['sha256'],'--fixture-ledger',str(ledger),'--fixture-ledger-sha256',hashlib.sha256(ledger.read_bytes()).hexdigest()]
save()
try:
 run('author',base+['author','--observer-exe',fixture['path'],*bindings,'--actual-cli-proof',str(proof),'--cli-proof-sha256',hashlib.sha256(proof.read_bytes()).hexdigest(),'--output-dir',str(directory)],R)
 run('build',[str(T/'wrappers/wixl'),'-a','x86','-o','context-probe.msi','context-probe.wxs'],directory)
 info=str(T/'root/usr/bin/msiinfo');msi=str(directory/'context-probe.msi')
 tablelist=run('tables',[info,'tables',msi],directory);run('summary',[info,'suminfo',msi],directory)
 tables=tablelist.decode('ascii').splitlines();assert 1<=len(tables)<=64 and len(tables)==len(set(tables))
 exports=directory/'exports';exports.mkdir()
 for table in tables:
  assert table and all(c.isascii() and (c.isalnum() or c=='_') for c in table)
  if table in {'_Tables','_Columns','_Validation','_SummaryInformation'}:continue
  data=run('export-'+table,[info,'export',msi,table],directory);(exports/(table+'.idt')).write_bytes(data)
 extracted=directory/'extracted-observer.exe'
 run('extract-observer',[info,'extract',msi,'Binary.FcReadonlyObserver'],directory,binary=extracted)
 checked=run('check',base+['check-exports','--exports',str(exports),'--tables-list',str(E/'context-probe-tables-first.stdout.log'),'--extracted-observer',str(extracted),*bindings],directory)
 v['tableCheck']=json.loads(checked);assert v['tableCheck']['status']=='PASS_EXPORTED_PROBE_TOPOLOGY_ONLY'
 v['sourceAfter']=guards();assert v['sourceAfter']==before
 data=Path(msi).read_bytes();v['msi']=dict(path=msi,bytes=len(data),sha256=hashlib.sha256(data).hexdigest());v['status']='PASS_LOCAL_CONTEXT_OBSERVER_MSI_STATIC_ONLY_NOT_WINDOWS'
except Exception as error:
 v.update(status='FAIL_LOCAL_CONTEXT_OBSERVER_BUILD_OR_TABLES',errorKind=type(error).__name__,errorCode=str(error) if isinstance(error,AssertionError) else 'LOCAL_COMMAND_OR_INPUT_FAILURE')
finally:v['elapsedSeconds']=time.monotonic()-start;save()
print(json.dumps({k:v.get(k) for k in ('status','nonce','directory','msi','elapsedSeconds','errorKind','errorCode')}));raise SystemExit(0 if v['status'].startswith('PASS') else 1)
