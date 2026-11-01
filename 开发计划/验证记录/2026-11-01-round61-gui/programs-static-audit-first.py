#!/usr/bin/env python3
"""Read-only actual GNU artifact inspection; never execute Windows images."""
from pathlib import Path
import hashlib,json,subprocess,sys,struct,shutil
E=Path('/tmp/fc-round61-gui/evidence'); W=Path('/tmp/fc-round61-gui/worktree')
def ident(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def write(n,b):
 p=E/n
 with p.open('xb') as f:f.write(b)
 return ident(p)
def cmd(name,args):
 q=subprocess.run(args,cwd=W,capture_output=True,timeout=60)
 assert len(q.stdout)<=16777216 and len(q.stderr)<=16777216
 r={'argv':args,'exitCode':q.returncode,'actualExit':True,'stdout':write(name+'.stdout.log',q.stdout),'stderr':write(name+'.stderr.log',q.stderr)}
 return r,q.stdout
report={'status':'FAIL','scope':'Actual local GNU artifacts, static inspection only; no Windows execution or MSVC GUI proof.','helper':ident(Path(__file__))}
try:
 orig=E/'integrated-checks-first.json'; o=json.loads(orig.read_bytes()); report['originalCompileReport']=ident(orig)
 assert o['sourceUnchanged'] is True
 assert all(x['status']=='PASS' and x['exitCode']==0 and x['actualExit'] for x in o['commands'])
 subset={k:v for k,v in o['sourceBefore'].items() if k.endswith('.rs') or k.endswith('Cargo.toml') or k=='Cargo.lock' or k.startswith('.cargo/')}
 current={k:{'bytes':(W/k).stat().st_size,'sha256':ident(W/k)['sha256']} for k in subset}
 assert current==subset
 report['sourceBinding']={'sourceBase':o['sourceBase'],'count':len(subset),'domain':'All original .rs, Cargo.toml, Cargo.lock and .cargo/* snapshot entries; CI/PowerShell and docs excluded deliberately.','sourceBefore':subset,'currentMatchesBefore':True,'ciScriptMayChangeIndependently':True}
 tool=shutil.which('i686-w64-mingw32-objdump');assert tool
 ver,_=cmd('programs-objdump-version-first',[tool,'--version']);assert ver['exitCode']==0
 report['objdump']={'executable':ident(Path(tool).resolve()),'version':ver}
 report['auditor']=ident(W/'scripts/audit-pe.py')
 report['programs']=[]
 for role,log,command,expected,package in [('normal','windows-product-first.stdout.log','windows-product','gui','fc-bootstrap'),('test','windows-adapter-compile-first.stdout.log','windows-adapter-compile','console','fc-windows')]:
  p=E/log;lines=[json.loads(s) for s in p.read_text().splitlines()]
  artifacts=[a for a in lines if a.get('reason')=='compiler-artifact' and a.get('executable')]
  assert len(artifacts)==1
  a=artifacts[0];assert a['profile']['test']==(role=='test')
  assert a['manifest_path']==str(W/'crates'/package/'Cargo.toml')
  assert a['target']['kind']==(['lib'] if role=='test' else ['bin'])
  assert a['target']['name']==('fc_windows' if role=='test' else 'fc-bootstrap')
  assert len([x for x in lines if x.get('reason')=='build-finished' and x.get('success') is True])==1
  c=next(x for x in o['commands'] if x['name']==command)
  assert ident(p)['sha256']==c['stdout']['sha256'] and ident(p)['bytes']==c['stdout']['bytes']
  exe=Path(a['executable']);before=ident(exe)
  dump,raw=cmd('programs-'+role+'-objdump-first',[tool,'-p',str(exe)]);assert dump['exitCode']==0
  audit,encoded=cmd('programs-'+role+'-pe-audit-first',[sys.executable,'-B','scripts/audit-pe.py','--exe',str(exe),'--objdump',tool,'--expected-subsystem',expected]);assert audit['exitCode']==0
  pe=json.loads(encoded);assert pe['structural_errors']==[] and pe['format']=='PE32-x86'
  binary=exe.read_bytes();off=struct.unpack_from('<I',binary,0x3c)[0];assert binary[off:off+4]==b'PE\0\0'
  machine=struct.unpack_from('<H',binary,off+4)[0];magic=struct.unpack_from('<H',binary,off+24)[0];sub=struct.unpack_from('<H',binary,off+24+68)[0]
  assert machine==0x14c and magic==0x10b and sub==(2 if role=='normal' else 3)
  imports=pe['imports'];bad=[d for d in imports if d=='combase.dll' or 'winrt-error' in d or d=='ktmw32.dll'];assert not bad
  required={'kernel32.dll':['GetConsoleWindow'],'user32.dll':['MessageBoxW']} if role=='normal' else {}
  for d,names in required.items():
   assert all(s in imports.get(d,[]) for s in names)
  assert before==ident(exe)
  report['programs'].append({'role':role,'identity':before,'producer':a,'producerLog':ident(p),'producerCommand':c,'objdump':dump,'audit':audit,'pe':pe,'directPeHeader':{'machine':machine,'optionalMagic':magic,'subsystem':sub},'forbiddenDllsPresent':bad,'requiredImports':required,'unchangedAcrossInspection':True})
  if role=='test':
   report['testMetadata']=write('windows-test-metadata-static-first.jsonl',p.read_bytes())
   report['testMetadata']['format']='Exact original Cargo JSON stream; only one executable with profile.test=true, fc_windows. No executed or enumerated Windows test-count claim.'
 report['limitations']=['PASS_STATIC_ONLY_RUNTIME_NOT_RUN','No executable was launched, including --list; the expected Windows test count remains a runtime assertion, not derived from Cargo metadata.','Modern GNU subsystem version is recorded, not required to equal Win7 6.1.','No MSVC artifact or Win7/10/11 execution is established.','CI rustc/RUSTFLAGS ordering fix occurs outside the frozen Rust/Cargo/config artifact-input subset.']
 report['status']='PASS_STATIC_ONLY_RUNTIME_NOT_RUN'
except Exception as ex:
 report['error']={'kind':type(ex).__name__,'message':str(ex)}
r=write('programs-static-first.json',(json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode())
print(json.dumps({'report':r,'status':report['status'],'programs':[{'role':p['role'],**p['identity']} for p in report.get('programs',[])],'metadata':report.get('testMetadata')},ensure_ascii=False))
raise SystemExit(0 if report['status']=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN' else 1)
