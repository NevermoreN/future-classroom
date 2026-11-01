#!/usr/bin/env python3
"""Read-only original R59 runtime/public/visual evidence reconciliation; no native execution."""
from pathlib import Path
import hashlib,json,re,runpy,struct,subprocess
E=Path(__file__).resolve().parent
ROOT=Path('/workspace/共享/Eliora/未来教室计划')
SOURCE='6082dd0173a0b8884c27ac78fda5054275f99f14'
FACTS={};checks=[]
def need(condition,label):
 if not condition:raise RuntimeError(label)
 checks.append({'name':label,'status':'PASS'})
def fact(path,expected=None):
 p=Path(path);h=hashlib.sha256();size=0
 with p.open('rb') as f:
  for b in iter(lambda:f.read(256*1024),b''):h.update(b);size+=len(b)
 row={'path':str(p),'bytes':size,'sha256':h.hexdigest()}
 if expected:
  need(row['bytes']==expected['bytes'] and row['sha256']==expected['sha256'],'actual file bytes/SHA '+str(p))
 FACTS[str(p)]=row;return row

def load(name):
 p=E/name;fact(p);return json.loads(p.read_bytes())
validator=ROOT/'scripts/windows-probe.py';vf=fact(validator);namespace=runpy.run_path(str(validator))
pinned=subprocess.run(['git','show',SOURCE+':scripts/audit-pe.py'],cwd=ROOT,capture_output=True,check=True).stdout
pn={'__name__':'r59_pinned_parser','__file__':str(ROOT/'scripts/audit-pe.py')};exec(compile(pinned,'R59 audit-pe Git source','exec'),pn)
result={'schema':'future-classroom.r59-runtime-independent/v1','status':'REVIEWING','sourceSha':SOURCE,'scope':'Local read-only verification of original recorded Windows runs and exact artifact bytes; no new runtime/SSH/network/Cargo or CI.','sourceBindings':{'currentWindowsProbe':vf,'pinnedPeParserSha256':hashlib.sha256(pinned).hexdigest()},'baselines':[]}
for tag in ['modern','win7']:
 a=load(tag+'-original-independent-first.json');com=load(tag+'-com-import-independent-first.json')
 need(a['status']=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN' and a['sourceSha']==SOURCE,'original exact static audit '+tag)
 need(com['status']=='PASS_EXACT_MSVC_STATIC_ONLY' and com['sourceSha']==SOURCE,'original exact COM audit '+tag)
 need(com['artifactAuditSha256']==FACTS[str(E/(tag+'-original-independent-first.json'))]['sha256'],'COM audit binds original artifact report '+tag)
 need(a['peInspector']['parserSourceSha256']==hashlib.sha256(pinned).hexdigest(),'raw parser pinned Git source '+tag)
 producer={r['producer']['relative_path']:r for r in a['executables']}
 need(len(producer)==22 and len(com['programs'])==22,'22 original programs '+tag)
 com_by={r['program']:r for r in com['programs']};need(set(com_by)==set(producer),'complete COM program set '+tag)
 tests={};normals=[];pe_rows=[]
 for name,row in producer.items():
  p=row['producer'];local=Path(row['localPath']);actual=fact(local,{'bytes':p['bytes'],'sha256':p['sha256']})
  rawfact=fact(row['rawInspection']['path'],row['rawInspection']);raw=Path(rawfact['path']).read_text()
  parsed=pn['parse_objdump'](raw);need(not parsed['structural_errors'] and parsed['imports']==row['independentPe']['imports'],'independent raw CUI PE import parser '+tag+'/'+name)
  imports=parsed['imports'];unexpected=[d for d in imports if d=='combase.dll' or d=='ktmw32.dll' or 'winrt' in d]
  static_free=[d for d,symbols in imports.items() if 'CoTaskMemFree' in symbols]
  crt=[d for d in imports if d.startswith(('vcruntime','msvcp','api-ms-win-crt','libgcc_s_','libstdc++','libwinpthread')) or re.fullmatch(r'(?:msvcr(?:t|\d+)[a-z0-9_]*|ucrtbased?)\.dll',d)]
  need(not unexpected and not static_free and not crt,'no forbidden COM/WinRT/KTM/static task free/dynamic CRT '+tag+'/'+name)
  c=com_by[name];need(c['sha256']==p['sha256'] and c['unexpectedDlls']==unexpected and c['staticCoTaskMemFreeDlls']==static_free and c['subsystem']==3,'original COM claims equal actual imports '+tag+'/'+name)
  data=local.read_bytes();off=struct.unpack_from('<I',data,60)[0];subsystem=struct.unpack_from('<H',data,off+92)[0];version=list(struct.unpack_from('<HH',data,off+72))
  need(subsystem==3 and row['peHeader']['subsystem']==3,'actual R59 CUI header '+tag+'/'+name)
  if tag=='win7':need(version==[6,1] and row['peHeader']['subsystemVersion']==[6,1],'actual Win7 subsystem6.1 '+name)
  pe_rows.append({'path':name,'producer':p,'actualFile':actual,'rawObjdump':rawfact,'subsystem':subsystem,'version':version,'forbiddenDlls':unexpected,'staticCoTaskMemFreeDlls':static_free,'dynamicCrtDlls':crt})
  if p['role']=='tests':tests[local.name]=row
  else:normals.append(row)
 need(len(tests)==21 and len(normals)==1,'21 harnesses and 1 normal '+tag)
 normal=normals[0]['producer'];meta=a['testMetadata'];fact(meta['path'],meta)
 meta_rows=[json.loads(s) for s in Path(meta['path']).read_text().splitlines() if s.strip()]
 need(len(meta_rows)==21,'metadata exactly21 '+tag)
 by_meta={Path(v['executable']).name:v for v in meta_rows}
 need(set(by_meta)==set(tests),'metadata complete harness set '+tag)
 for name,row in tests.items():
  m=by_meta[name];need(m['reason']=='compiler-artifact' and m['profile']['test'] is True and m['executable']==row['localPath'] and m['target']['name']==row['producer']['target'],'metadata exact harness association '+tag+'/'+name)
 r=load(tag+'-msvc-runtime-first.json');cmd=load(tag+'-msvc-runtime-first.command.json')
 need(r['status']==r['cleanup']=='PASS' and cmd['status']=='PASS_COMMAND' and cmd['exitCode']==0 and cmd['actualExit'] is True and cmd['scriptUnchanged'] is True,'actual runtime controller exit and cleanup '+tag)
 need(cmd['scriptSha256']==vf['sha256'] and cmd['sourceSha']==SOURCE and cmd['metadataSha256']==meta['sha256'] and cmd['inputNormalSha256']==normal['sha256'],'runtime command source/metadata/normal binding '+tag)
 for stream in ['stdout','stderr']:fact(cmd[stream]['path'],cmd[stream])
 need(r['exeSha256']==normal['sha256'] and len(r['uploadedArtifacts'])==22,'runtime normal/upload count '+tag)
 uploads={x['name']:x['sha256'] for x in r['uploadedArtifacts']};expected_uploads={name:row['producer']['sha256'] for name,row in tests.items()};expected_uploads['fc-bootstrap.exe']=normal['sha256']
 need(uploads==expected_uploads,'actual uploaded22 SHAs equal original producer '+tag)
 need(r['platform']['build']=='26300' and r['platform']['osArchitecture']=='64-bit' and 'Windows 11' in r['platform']['caption'],'actual only Win11Pro26300 platform '+tag)
 need(len(r['tests'])==21 and {t['name'] for t in r['tests']}==set(tests),'actual runtime complete21 set '+tag)
 harness=[];total=0
 for t in r['tests']:
  name=t['name'];need(t['binarySha256']==tests[name]['producer']['sha256'] and t['exitCode']==0 and t['timeoutSeconds']==900,'actual harness SHA/exit/budget '+tag+'/'+name)
  out=t['output'];need(isinstance(out,list) and all(isinstance(x,str) for x in out),'complete output string array '+tag+'/'+name)
  running=[re.fullmatch(r'running ([0-9]+) tests?',x) for x in out];running=[m for m in running if m]
  summaries=[x for x in out if x.startswith('test result:')]
  need(len(running)==len(summaries)==1,'exact one running and summary '+tag+'/'+name)
  summary=re.fullmatch(r'test result: ok\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored; ([0-9]+) measured; ([0-9]+) filtered out; finished in ([0-9.]+)s',summaries[0]);need(summary is not None,'exact passed summary syntax '+tag+'/'+name)
  count=int(running[0][1]);passed=int(summary[1]);need(count==passed and all(int(summary[i])==0 for i in range(2,6)),'zero fail/ignored/measured/filtered exact count '+tag+'/'+name)
  lines=[re.fullmatch(r'test (.+) \.\.\. ok',x) for x in out];names=[m[1] for m in lines if m]
  need(len(names)==len(set(names))==count,'unique every test line equals running/summary '+tag+'/'+name)
  need(not any(re.fullmatch(r'test .+ \.\.\. (FAILED|ignored.*)',x) for x in out),'no hidden failing/ignored test lines '+tag+'/'+name)
  # This is a canonical digest of preserved line content, not original process bytes.
  line_json=json.dumps(out,ensure_ascii=False,separators=(',',':')).encode()
  harness.append({'name':name,'binarySha256':t['binarySha256'],'passed':passed,'failed':0,'ignored':0,'measured':0,'filtered':0,'uniqueTestNames':names,'preservedOutputLineCount':len(out),'canonicalLineArraySha256':hashlib.sha256(line_json).hexdigest()});total+=passed
 need(total==1489,'independent actual1489 reconciliation '+tag)
 need(r['selfCheck']['exitCode']==0 and r['system']['exitCode']==0 and r['native']['exitCode']==0 and r['native']['hostExitCode']==0,'actual selfcheck/system/native/hiddenhost exits '+tag)
 op=load(tag+'-public-visual-operation-first.json');need(op['status']=='PASS_SELECTED2_ORIGINAL_MSVC_PUBLIC_VISUAL_FLOWS' and op['scriptSha256']==vf['sha256'] and len(op['operations'])==2,'public/visual original controller2 '+tag)
 flow_rows=[]
 for short,field,validator_name,count in [('public','publicDesktop','validate_public_desktop_output',20),('visual','subjectVisual','validate_subject_visual_output',22)]:
  name=tag+'-msvc-'+short+'-native-first';flow=load(name+'.json');o=[o for o in op['operations'] if o['label']==name];need(len(o)==1,'exact operation '+name);o=o[0]
  need(flow['status']==flow['cleanup']=='PASS' and flow['exeSha256']==normal['sha256'] and flow['uploadedArtifacts']==[{'name':'fc-bootstrap.exe','sha256':normal['sha256']}],'flow original upload/status/cleanup '+name)
  need(o['actualExit'] is True and o['exitCode']==0 and o['resultStatus']==o['cleanup']=='PASS' and o['exeSha256']==normal['sha256'],'flow controller actual physical exit '+name)
  for stream in ['stdout','stderr']:fact(o[stream]['path'],o[stream])
  native=flow['native'];validated=namespace[validator_name](native[field+'ExitCode'],native[field+'Output'])
  need(native['exitCode']==0 and validated['steps']==count,'main exact ordered validator actual transcript '+name)
  need(validated['stepNames']==flow[field+'NativeStepNames'] and validated['elapsedMs']==flow[field+'ElapsedMs'],'validator parsed summary equals original fields '+name)
  need(flow['platform']==r['platform'],'flow same actual Win11 platform '+name)
  flow_rows.append({'label':name,'normalSha256':normal['sha256'],'controllerExitCode':o['exitCode'],'nativeExitCode':native[field+'ExitCode'],'cleanup':'PASS','validated':validated})
 result['baselines'].append({'baseline':a['baseline'],'officialRunId':a['runId'],'officialArtifactId':a['artifactId'],'platform':r['platform'],'executionEnvironment':'i686 x86 original PE on 64-bit Win11/WOW64 only','testHarnesses':harness,'totalPassed':total,'failed':0,'ignored':0,'filtered':0,'normal':normal,'normalSubsystem':3,'pePrograms':pe_rows,'flows':flow_rows,'recordedNativeScope':r['native']['output']})
result.update(status='PASS_ORIGINAL_R59_RUNTIME_PUBLIC_VISUAL_RECONCILIATION_ONLY',checks=checks,checkCount=len(checks),originalFileFacts=list(FACTS.values()),originalFileFactCount=len(FACTS),limitations=[
 'Both baselines executed only on recorded Microsoft Windows11Pro build26300 x64/WOW64. Win7 target execution here does not establish Win7OS, Win10 or visible Seewo compatibility.',
 'All44 PEs, including two normals, are CUI3; this provides no R61 GUI/noConsole or actual logon proof.',
 'Per-harness outputs in original runtimeJSON are complete PowerShell merged 2>&1 normalized string lines. They are not separately retained original stdout/stderr byte streams; canonicalLineArraySha256 is explicitly a content digest. Raw stdout/stderr SHA fields above refer to original local controller capture logs only.',
 'No source snapshot is fabricated for runtime. Exact original producer/EXE/upload/metadata SHAs bind the executed artifacts; original source capture limitations remain in the earlier original-artifact audits.',
 'COM import absence is static evidence; no universal OS API/SDK certification or MSI/installer lifecycle claim.'
],reviewerHelper=fact(__file__))
out=E/'runtime-public-visual-independent-first.json';need(not out.exists(),'fresh independent report path');out.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'status':result['status'],'checks':len(checks),'fileFacts':len(FACTS),'report':fact(out)},ensure_ascii=False))
