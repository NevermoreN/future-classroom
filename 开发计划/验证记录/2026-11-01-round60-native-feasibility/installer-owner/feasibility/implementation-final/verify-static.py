#!/usr/bin/env python3
"""Independently inspect the actual exported MSI and embedded PE, never install."""
import argparse,hashlib,json,pathlib,re,struct,subprocess,os,time
ROOT=pathlib.Path(__file__).resolve().parent
MAX_LOG=16*1024*1024
def info(path):
 b=path.read_bytes();return {'path':str(path),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--build',type=pathlib.Path,required=True);ap.add_argument('--report',type=pathlib.Path,required=True);args=ap.parse_args();build=args.build.resolve();out=args.report.resolve();assert not out.exists()
 fixture=json.loads((build/'fixture.json').read_text());report=json.loads((build/'build-report.json').read_text());checks=[]
 def check(value,name):
  checks.append({'name':name,'status':'PASS' if value else 'FAIL'})
  if not value:raise ValueError(name)
 commands=[]
 try:
  check(report['status']=='PASS_LOCAL_STATIC_BUILD_ONLY_NOT_INSTALLED','original local build status')
  for cmd in report['commands']:
   check(cmd['actualExit'] is True and cmd['exitCode']==0,'original command exit '+cmd['name'])
   for stream in ['stdout','stderr']:check(info(pathlib.Path(cmd[stream]['path']))==cmd[stream],'original stream bytes/SHA '+cmd['name']+' '+stream)
  check(info(build/'fixture.msi')==report['msi'] and info(build/'guardian.exe')==report['guardian'],'actual artifacts original bytes/SHA')
  b=(build/'guardian.exe').read_bytes();peoff=struct.unpack_from('<I',b,0x3c)[0];magic=struct.unpack_from('<H',b,peoff+24)[0];machine=struct.unpack_from('<H',b,peoff+4)[0];subsystem=struct.unpack_from('<H',b,peoff+24+68)[0]
  check(b[:2]==b'MZ' and b[peoff:peoff+4]==b'PE\0\0' and machine==0x14c and magic==0x10b and subsystem==2,'actual embedded guardian PE32 x86 GUI (no console subsystem)')
  tool=ROOT/'tools/root/usr/bin/msiinfo';env=os.environ.copy();env['LD_LIBRARY_PATH']=str(ROOT/'tools/root/usr/lib/x86_64-linux-gnu')
  def run(name,argv):
   before=time.monotonic();cp=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,timeout=120,check=False)
   check(len(cp.stdout)<=MAX_LOG and len(cp.stderr)<=MAX_LOG,'independent command bound '+name)
   row={'name':name,'argv':list(map(str,argv)),'actualExit':True,'exitCode':cp.returncode,'elapsedSeconds':time.monotonic()-before}
   for kind,data in [('stdout',cp.stdout),('stderr',cp.stderr)]:
    q=build/('independent-'+name+'.'+kind+'.log')
    with q.open('xb') as f:f.write(data)
    row[kind]=info(q)
   commands.append(row);check(cp.returncode==0,'independent command exit '+name);return cp.stdout
  def table(name):
   data=run('table-'+name,[str(tool),'export',str(build/'fixture.msi'),name]).decode('utf8','strict');lines=data.splitlines();columns=lines[0].split('\t');rows=[]
   check(len(lines)>=3 and lines[2].split('\t')[0]==name,'actual IDT table schema '+name)
   for line in lines[3:]:
    if not line:continue
    values=line.split('\t');check(len(values)==len(columns),'actual IDT row shape '+name);rows.append(dict(zip(columns,values)))
   return rows
  chainer=table('MsiEmbeddedChainer');check(chainer==[{'MsiEmbeddedChainer':'Guardian','Condition':'NOT (ACTION = "ADVERTISE")','CommandLine':'','Source':'Guardian','Type':'2'}],'one exact native embedded chainer row, no additional payload package')
  binary=table('Binary');check(binary==[{'Name':'Guardian','Data':'Binary.Guardian'}],'exact binary stream row')
  extracted=run('guardian-stream',[str(tool),'extract',str(build/'fixture.msi'),'Binary.Guardian']);check(extracted==b,'independent actual embedded binary byte identity')
  registry=table('Registry');check(len(registry)==1,'one installed registry value only');reg=registry[0]
  check(reg['Root']=='1' and reg['Key']==fixture['registryRoot']+'\\Payload' and reg['Name']=='OwnedPayload' and reg['Value']=='#1' and reg['Component_']=='OwnedMarker','HKCU only single-layer random Software leaf payload DWORD1; no Run/ProgramData')
  properties={x['Property']:x['Value'] for x in table('Property')}
  check(properties['ProductCode']==fixture['productCode'] and properties['UpgradeCode']==fixture['upgradeCode'] and properties['ARPSYSTEMCOMPONENT']=='1' and properties['REBOOT']=='ReallySuppress' and 'ALLUSERS' not in properties,'fresh random identity/per-user/no reboot/hiddenARP')
  check(fixture['productCode']!=fixture['upgradeCode'] and re.fullmatch(r'[a-f0-9]{32}',fixture['nonce']) is not None,'distinct random GUIDs and fixed nonce')
  comp=table('Component');check(len(comp)==1 and comp[0]['Component']=='OwnedMarker' and comp[0]['ComponentId']==fixture['componentCode'] and comp[0]['Attributes']=='4' and comp[0]['KeyPath']==reg['Registry'],'one registry-keypath component with exact ownership')
  check(table('FeatureComponents')==[{'Feature_':'Probe','Component_':'OwnedMarker'}],'single closed feature/component mapping')
  features=table('Feature');check(len(features)==1 and features[0]['Feature']=='Probe','one feature')
  conditions=table('LaunchCondition');check({r['Condition'] for r in conditions}=={'NOT ALLUSERS','NOT RollbackDisabled AND NOT DISABLEROLLBACK'},'per-user/rollback enabled guards')
  empty=['CustomAction','File','CreateFolder','Upgrade','Shortcut','ServiceInstall','ServiceControl','RemoveFile','IniFile','RemoveIniFile']
  for name in empty:check(table(name)==[],'no side-effect table rows '+name)
  seq=table('InstallExecuteSequence');check(not ({r['Action'] for r in seq}&{'ForceReboot','ScheduleReboot','InstallServices','StartServices','StopServices','DeleteServices','CreateShortcuts','RemoveShortcuts','InstallFiles','RemoveFiles','RemoveExistingProducts'}),'no file/service/shortcut/upgrade/reboot operations')
  summary=run('summary',[str(tool),'suminfo',str(build/'fixture.msi')]).decode('utf8','strict');check('Intel;1033' in summary and re.search(r'Version:\s*500',summary) is not None,'MSI x86/Installer5 summary')
  result={'schemaVersion':1,'status':'PASS_ACTUAL_MSI_STATIC_ONLY_WINDOWS_NOT_RUN','scope':'Actual locally built single synthetic MSI with Binary-table GUI native transaction guardian. No product installation or runtime result.','msi':info(build/'fixture.msi'),'guardian':info(build/'guardian.exe'),'fixture':fixture,'pe':{'machine':'0x014c','optionalMagic':'0x010b','subsystem':2},'chainer':chainer,'registry':registry,'checks':checks,'commands':commands,'windows':'NOT_RUN','limitations':['The existence of a valid authored table does not prove MsiJoinTransaction/End is called or supported for this single-package guardian.','Direct install/repair/uninstall, rollback and actual process identities remain pending real Windows execution.','GNU guardian PE32/GUI is for authorized Win11 feasibility only, not a formal Win7/MSVC baseline.','No physical maintenance broker or production startup gate implemented; no Run cleanup/CAS guarantee.']}
 except BaseException as error:
  result={'schemaVersion':1,'status':'FAIL_STATIC_REVIEW','checks':checks,'commands':commands,'error':{'kind':type(error).__name__,'message':str(error)}}
  raise
 finally:
  with out.open('x') as f:json.dump(result,f,ensure_ascii=False,indent=2);f.write('\n')
if __name__=='__main__':main()
