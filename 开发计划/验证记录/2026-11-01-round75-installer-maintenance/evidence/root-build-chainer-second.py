import os,json,time,hashlib,subprocess,shutil,re
from pathlib import Path
R=Path('/tmp/fc-round75-installer-maintenance');E=R/'evidence';T=R/'tools';D=E/'chainer-build-second';I=R/'chainer-probe-input-second-bd0f287d596c4a01bc2a120c3670afb8';C=R/'chainer-compile-output-bd0f287d596c4a01bc2a120c3670afb8';P=E/'chainer-probe-preparation-second/chainer-probe.py'
def need(v,m):
 if not v:raise AssertionError(m)
def fact(p):
 b=p.read_bytes();return dict(path=str(p),bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
need(not D.exists(),'FRESH_BUILD');D.mkdir();(D/'.owner').write_text('future-classroom-owned-r75-chainer-build-local/v1\n');t=time.monotonic();v=dict(status='RUNNING',commands=[],scope='Independent C# fixture static local build only, not product or Windows execution',overallSeconds=120)
env=os.environ.copy()
for k in ('LD_PRELOAD','LD_AUDIT','GIO_EXTRA_MODULES','GIO_MODULE_DIR'):env.pop(k,None)
env.update(LD_LIBRARY_PATH=str(T/'root/usr/lib/x86_64-linux-gnu'),LC_ALL='C',GIO_USE_VFS='local',GSETTINGS_BACKEND='memory')
report=D/'build-operation.json'
def save():report.write_text(json.dumps(v,indent=2)+'\n')
def run(name,args,out=None):
 remain=120-(time.monotonic()-t);need(remain>0,'ORIGINAL_BUDGET');a=time.monotonic();r=subprocess.run(args,cwd=I,env=env,capture_output=True,timeout=min(20,remain));row=dict(name=name,args=args,cwd=str(I),actualExit=True,exitCode=r.returncode,elapsedSeconds=time.monotonic()-a)
 for n,b in [('stdout',r.stdout),('stderr',r.stderr)]:
  p=out if n=='stdout' and out else D/(name+'.'+n+'.log');p.write_bytes(b);row[n]=fact(p)
 v['commands'].append(row);save();need(r.returncode==0,'ACTUAL_EXIT0_'+name);need(not r.stderr,'NO_TOOL_WARNINGS_'+name);return r.stdout
save()
try:
 need((R/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n','OWNER')
 pins={'tools/root/usr/bin/msibuild':'87ee3aa0af25bb997646071a45e35ebae3e3ba5f26fe7b479df97aa31bd8a1eb','tools/root/usr/bin/msiinfo':'50b25845c863756975a8523fe1b370c4bf5dd1812a2a0229cf22139aa4b5ea81','tools/root/usr/bin/wixl':'37fdfb01fb2e3fc752890f1e3eca4a161b5f41e251faa279ded0a9f907d73097','tools/wrappers/wixl':'2ac0b4eebef87eb93298a0de3667ec7ad0e829e94c6e1e52b93518d50a400c60'}
 for n,h in pins.items():need(fact(R/n)['sha256']==h,'TOOL_PIN')
 v['tools']=[fact(R/n) for n in pins];spec=json.loads((I/'spec.json').read_text());receipt=json.loads((C/'compiler-receipt.json').read_text());need(receipt['status']=='PASS_CSC_ACTUAL_EXIT0','ACTUAL_COMPILER')
 for n,h in [('ChainerProbe.cs',spec['source']['sha256']),('probe.wxs',spec['wxsSha256']),('MsiEmbeddedChainer.idt',spec['chainerIdtSha256'])]:need(fact(I/n)['sha256']==h,'INPUT_PIN_'+n)
 exe=I/'chainer-probe.exe';need(not exe.exists(),'FRESH_EXE_COPY');exe.write_bytes((C/'ChainerProbe.exe').read_bytes());need(fact(exe)['sha256']==receipt['exe']['sha256'],'ACTUAL_EXE_PIN')
 intermediate=D/'intermediate.msi';final=D/'chainer-probe-final.msi'
 run('01-wixl',[str(T/'wrappers/wixl'),'-a','x86','-o',str(intermediate),'probe.wxs']);v['intermediate']=fact(intermediate);final.write_bytes(intermediate.read_bytes())
 run('02-import-chainer',[str(T/'root/usr/bin/msibuild'),str(final),'-i',str(I/'MsiEmbeddedChainer.idt')]);v['finalMsi']=fact(final);h=v['finalMsi']['sha256'];info=str(T/'root/usr/bin/msiinfo')
 listed=D/'tables.stdout.log';raw=run('03-tables',[info,'tables',str(final)],listed);names=raw.decode('ascii').splitlines();need(len(names)==len(set(names)) and len(names)<=64 and all(re.fullmatch('[A-Za-z_][A-Za-z_0-9]*',n) for n in names),'CLOSED_TABLES');exports=D/'exports';exports.mkdir()
 for n in names:
  if n in {'_Tables','_Columns','_Validation','_SummaryInformation'}:continue
  run('04-export-'+n,[info,'export',str(final),n],exports/(n+'.idt'))
 summary=D/'summary.stdout.log';run('05-summary',[info,'suminfo',str(final)],summary)
 extracted=D/'extracted-probe.exe';run('06-binary',[info,'extract',str(final),'Binary.FcIndependentChainer'],extracted);need(extracted.read_bytes()==exe.read_bytes(),'BINARY_EXACT')
 args=['python3','-B',str(P),'check-exports','--spec',str(I/'spec.json'),'--spec-sha256',fact(I/'spec.json')['sha256'],'--compile-receipt',str(C/'compiler-receipt.json'),'--compile-receipt-sha256',fact(C/'compiler-receipt.json')['sha256'],'--tables-list',str(listed),'--exports',str(exports),'--extracted-probe',str(extracted),'--msi',str(final),'--msi-sha256',h,'--summary',str(summary)]
 checked=run('07-static-check',args);result=json.loads(checked);need(result['status']=='PASS_EXACT_STATIC_CHAINER_FIXTURE_ONLY','EXACT_STATIC_CHECK');need(fact(final)==v['finalMsi'],'FINAL_MSI_UNCHANGED');v.update(status='PASS_LOCAL_INDEPENDENT_CHAINER_BUILD_STATIC_ONLY',tableCheck=result,compilerReceipt=fact(C/'compiler-receipt.json'),spec=fact(I/'spec.json'))
except Exception as e:v.update(status='FAIL_LOCAL_CHAINER_BUILD',error=dict(type=type(e).__name__,message=str(e)))
v.update(elapsedSeconds=time.monotonic()-t,actualControllerReturned=True);save();print(json.dumps(dict(status=v['status'],report=fact(report))));raise SystemExit(0 if v['status']=='PASS_LOCAL_INDEPENDENT_CHAINER_BUILD_STATIC_ONLY' else 1)
