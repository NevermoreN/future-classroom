#!/usr/bin/env python3
"""Local-only build. Requires exact owned pinned tools, never installs/runs MSI."""
import argparse,hashlib,json,os,pathlib,subprocess,time,uuid,xml.etree.ElementTree as ET
ROOT=pathlib.Path(__file__).resolve().parent
MAX_STREAM=16*1024*1024
NS='http://schemas.microsoft.com/wix/2006/wi'
def sha(b): return hashlib.sha256(b).hexdigest()
def info(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':sha(b)}
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--output',required=True,type=pathlib.Path);args=ap.parse_args();out=args.output.resolve();out.mkdir()
 (out/'.owner').write_text('R60 synthetic MSI build only; no Windows invocation\n')
 report={'status':'RUNNING_LOCAL_STATIC_BUILD','commands':[],'sources':[info(ROOT/'guardian.c'),info(pathlib.Path(__file__))]}
 def run(name,argv,env=None):
  t=time.monotonic();cp=subprocess.run(argv,cwd=out,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=120,check=False)
  assert len(cp.stdout)<=MAX_STREAM and len(cp.stderr)<=MAX_STREAM
  row={'name':name,'argv':list(map(str,argv)),'actualExit':True,'exitCode':cp.returncode,'elapsedSeconds':time.monotonic()-t}
  for k,b in [('stdout',cp.stdout),('stderr',cp.stderr)]:
   p=out/(name+'.'+k+'.log');p.write_bytes(b);row[k]=info(p)
  report['commands'].append(row)
  if cp.returncode:raise RuntimeError('bounded local build command failed: '+name)
  return cp.stdout
 try:
  nonce=uuid.uuid4().hex;product=str(uuid.uuid4()).upper();upgrade=str(uuid.uuid4()).upper();component=str(uuid.uuid4()).upper()
  registry='Software\\FutureClassroom-MsiGuardian-Test-'+nonce
  fixture={'schemaVersion':1,'nonce':nonce,'productCode':'{'+product+'}','upgradeCode':'{'+upgrade+'}','componentCode':'{'+component+'}','registryRoot':registry,'payloadKey':registry+'\\Payload','payloadValue':'OwnedPayload','guardianHandshakeMillis':10000,'perOperationMillis':60000,'status':'SYNTHETIC_FIXTURE_NOT_INSTALLED','rollbackDecision':0,'commitDecision':1}
  (out/'fixture.json').write_text(json.dumps(fixture,indent=2)+'\n')
  header='#define FIXTURE_NONCE L"'+nonce+'"\n#define FIXTURE_ROOT L"'+registry.replace('\\','\\\\')+'"\n'
  header+='#define FIXTURE_PRODUCT L\"{'+product+'}\"\n'
  (out/'fixture.h').write_text(header)
  run('gcc-version',['/usr/bin/i686-w64-mingw32-gcc','--version'])
  guardian=out/'guardian.exe'
  run('guardian-compile',['/usr/bin/i686-w64-mingw32-gcc','-municode','-mwindows','-Os','-Wall','-Wextra','-Werror','-static-libgcc','-Wl,--nxcompat,--dynamicbase','-I',str(out),str(ROOT/'guardian.c'),'-o',str(guardian),'-lmsi','-ladvapi32','-lshell32'])
  run('guardian-imports',['/usr/bin/i686-w64-mingw32-objdump','-p',str(guardian)])
  ET.register_namespace('',NS)
  def el(parent,name,**attrs):return ET.SubElement(parent,'{'+NS+'}'+name,attrs)
  wix=ET.Element('{'+NS+'}Wix');prod=el(wix,'Product',Id=product,Name='FutureClassroom synthetic transaction feasibility '+nonce,Language='1033',Codepage='1252',Version='0.0.1',Manufacturer='FutureClassroom Tests',UpgradeCode=upgrade)
  el(prod,'Package',InstallerVersion='500',Compressed='yes',InstallScope='perUser',Description='Owned synthetic MSI transaction observer; no product runtime.')
  el(prod,'Property',Id='ARPSYSTEMCOMPONENT',Value='1');el(prod,'Property',Id='REBOOT',Value='ReallySuppress');el(prod,'Property',Id='MSIRESTARTMANAGERCONTROL',Value='Disable')
  el(prod,'Condition',Message='Only current user context is allowed.').text='NOT ALLUSERS'
  el(prod,'Condition',Message='Rollback must remain enabled.').text='NOT RollbackDisabled AND NOT DISABLEROLLBACK'
  el(prod,'Binary',Id='Guardian',SourceFile=str(guardian))
  directory=el(prod,'Directory',Id='TARGETDIR',Name='SourceDir');local=el(directory,'Directory',Id='LocalAppDataFolder');folder=el(local,'Directory',Id='ProbeDir',Name='FutureClassroomMsiFeasibility-'+nonce)
  comp=el(folder,'Component',Id='OwnedMarker',Guid=component,Win64='no')
  el(comp,'RegistryValue',Root='HKCU',Key=fixture['payloadKey'],Name='OwnedPayload',Type='integer',Value='1',KeyPath='yes')
  feature=el(prod,'Feature',Id='Probe',Title='Synthetic registry marker only',Level='1');el(feature,'ComponentRef',Id='OwnedMarker')
  wxs=out/'fixture.wxs';wxs.write_bytes(ET.tostring(wix,encoding='utf-8',xml_declaration=True))
  tools=ROOT/'tools/root/usr/bin';env=os.environ.copy();env['LD_LIBRARY_PATH']=str(ROOT/'tools/root/usr/lib/x86_64-linux-gnu')
  for name in ['wixl','msibuild','msiinfo']:
   if not (tools/name).is_file():raise RuntimeError('required pinned owned MSI tool absent')
  msi=out/'fixture.msi';run('wixl-build',[str(tools/'wixl'),'-a','x86','-o',str(msi),str(wxs)],env)
  # wixl0.103 has no MsiEmbeddedChainer XML element. Explicitly author only the
  # official five-column table; stream executable remains ordinary Binary.Guardian.
  idt='MsiEmbeddedChainer\tCondition\tCommandLine\tSource\tType\r\ns72\tS255\tS255\ts72\ti2\r\nMsiEmbeddedChainer\tMsiEmbeddedChainer\r\nGuardian\tNOT (ACTION = "ADVERTISE")\t\tGuardian\t2\r\n'
  (out/'MsiEmbeddedChainer.idt').write_bytes(idt.encode('ascii'))
  run('msibuild-import',[str(tools/'msibuild'),str(msi),'-i',str(out/'MsiEmbeddedChainer.idt')],env)
  table_names=run('msi-tables',[str(tools/'msiinfo'),'tables',str(msi)],env).decode('utf8').splitlines()
  exports={}
  for table in table_names:
   if not table:continue
   data=run('table-'+table,[str(tools/'msiinfo'),'export',str(msi),table],env)
   exports[table]={'bytes':len(data),'sha256':sha(data),'path':str(out/('table-'+table+'.stdout.log'))}
  run('msi-summary',[str(tools/'msiinfo'),'suminfo',str(msi)],env)
  binary=run('guardian-binary-extract',[str(tools/'msiinfo'),'extract',str(msi),'Binary.Guardian'],env)
  if binary!=guardian.read_bytes():raise RuntimeError('embedded guardian binary bytes differ')
  report.update(status='PASS_LOCAL_STATIC_BUILD_ONLY_NOT_INSTALLED',fixture=fixture,guardian=info(guardian),msi=info(msi),tables=exports,embeddedGuardianByteIdentity=True)
 except BaseException as error:
  report['status']='FAIL_LOCAL_STATIC_BUILD';report['error']={'kind':type(error).__name__,'message':'Local build failed; exact command stdout/stderr and partial original files retained.'}
  raise
 finally:
  (out/'build-report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
if __name__=='__main__':main()
