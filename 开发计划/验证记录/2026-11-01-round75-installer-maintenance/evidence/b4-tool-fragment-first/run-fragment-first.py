"""Actual local table-format fragment, never a product MSI or executable.
Uses a text Binary sentinel, independent Product/Upgrade/registry namespace.
No installer/EXE invocation, compiler producer, Cargo or Windows operation.
"""
from pathlib import Path
import hashlib,json,os,runpy,subprocess,time,uuid,xml.etree.ElementTree as ET
R=Path('/tmp/fc-round75-installer-maintenance');E=R/'evidence';D=E/'b4-tool-fragment-first';T=R/'tools'
M=runpy.run_path(str(R/'worktree/scripts/package-msi-candidate.py'))
PINS={'wrappers/wixl':'2ac0b4eebef87eb93298a0de3667ec7ad0e829e94c6e1e52b93518d50a400c60','root/usr/bin/msibuild':'87ee3aa0af25bb997646071a45e35ebae3e3ba5f26fe7b479df97aa31bd8a1eb','root/usr/bin/msiinfo':'50b25845c863756975a8523fe1b370c4bf5dd1812a2a0229cf22139aa4b5ea81'}
def f(p):
 b=p.read_bytes();return dict(path=str(p),bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def main():
 (D/'.owner').write_bytes(b'future-classroom-owned-r75-b4-tool-fragment/v1\n')
 for n,s in PINS.items():assert f(T/n)['sha256']==s
 started=time.monotonic();report=dict(status='RUNNING',scope='ACTUAL_LOCAL_FORMAT_FRAGMENT_ONLY_NOT_PRODUCT_NOT_WINDOWS',commands=[],tools=[f(T/n) for n in PINS],overallSeconds=120,perCommandSeconds=20)
 env=os.environ.copy();env['LD_LIBRARY_PATH']=str(T/'root/usr/lib/x86_64-linux-gnu')
 def save():(D/'fragment-first.json').write_text(json.dumps(report,indent=2)+'\n')
 def run(label,cmd):
  out=D/(label+'.stdout.log');err=D/(label+'.stderr.log');beg=time.monotonic();remaining=120-(beg-started);assert remaining>0
  with out.open('xb') as so,err.open('xb') as se:
   child=subprocess.Popen(cmd,cwd=D,env=env,stdin=subprocess.DEVNULL,stdout=so,stderr=se);timed=False
   try:code=child.wait(timeout=min(20,remaining))
   except subprocess.TimeoutExpired:timed=True;child.kill();code=child.wait()
  report['commands'].append(dict(label=label,command=cmd,pid=child.pid,actualExit=True,exitCode=code,timedOut=timed,elapsedSeconds=time.monotonic()-beg,stdout=f(out),stderr=f(err)));save()
  assert not timed and code==0,label
  return out.read_bytes()
 try:
  (D/'payload').mkdir();(D/'payload/LICENSE').write_bytes(b'R75 TABLE FORMAT SENTINEL ONLY, NOT PRODUCT PAYLOAD.\n')
  (D/'NOT-AN-EXECUTABLE.txt').write_bytes(b'TEXT SENTINEL FOR MSI BINARY FORMAT ONLY. NEVER EXECUTE.\n')
  tree=ET.fromstring(M['authoring']('0.0.1',['LICENSE']));ns='{'+M['WIX_NS']+'}';product=tree.find(ns+'Product');nonce=uuid.uuid4().hex
  product.set('Id',str(uuid.uuid4()).upper());product.set('UpgradeCode',str(uuid.uuid4()).upper());product.set('Name','FutureClassroom test table fragment '+nonce)
  for node in tree.iter():
   if node.tag==ns+'Binary':node.set('SourceFile','NOT-AN-EXECUTABLE.txt')
   elif node.tag==ns+'RegistryValue':node.set('Key','Software\\FutureClassroom-Test-TableFragment-'+nonce)
   elif node.tag==ns+'Directory' and node.get('Id')=='FcInstallDir':node.set('Name','FutureClassroom-Test-TableFragment-'+nonce)
   elif node.tag==ns+'Component':node.set('Guid',str(uuid.uuid4()).upper())
  ET.indent(tree);(D/'fragment.wxs').write_bytes(ET.tostring(tree,encoding='utf-8',xml_declaration=True)+b'\n')
  report['author']=f(D/'fragment.wxs');report['fixtureNamespace']=nonce;save()
  wixl=T/'wrappers/wixl';build=T/'root/usr/bin/msibuild';info=T/'root/usr/bin/msiinfo'
  run('01-compile',[str(wixl),'-a','x86','-o','fragment.msi','fragment.wxs'])
  assert (D/'01-compile.stderr.log').stat().st_size==0,'compiler_stderr_must_remain_empty'
  run('02-copy-msi', ['/bin/cp','--no-clobber','fragment.msi','fragment-intermediate.msi'])
  ca=run('03-before-ca',[str(info),'export','fragment.msi','CustomAction']);seq=run('04-before-sequence',[str(info),'export','fragment.msi','InstallExecuteSequence'])
  assert ca.decode('ascii').splitlines()[:3]==['Action\tType\tSource\tTarget\tExtendedType','s72\ti2\tS72\tS255\tI4','CustomAction\tAction'] and M['idt_rows'](ca,'CustomAction')==[]
  (D/'CustomAction.idt').write_bytes(M['lifecycle_custom_action_idt']());(D/'InstallExecuteSequence.idt').write_bytes(M['lifecycle_execute_sequence_idt'](seq));(D/'MsiEmbeddedChainer.idt').write_bytes(M['chainer_idt']())
  run('05-import',[str(build),'fragment.msi','-q','DROP TABLE `CustomAction`','-q','DROP TABLE `InstallExecuteSequence`','-i','CustomAction.idt','-i','InstallExecuteSequence.idt','-i','MsiEmbeddedChainer.idt'])
  afterca=run('06-final-ca',[str(info),'export','fragment.msi','CustomAction']);afterseq=run('07-final-sequence',[str(info),'export','fragment.msi','InstallExecuteSequence']);chainer=run('08-final-chainer',[str(info),'export','fragment.msi','MsiEmbeddedChainer'])
  assert M['idt_rows'](afterca,'CustomAction')==M['idt_rows']((D/'CustomAction.idt').read_bytes(),'CustomAction')
  assert M['idt_rows'](afterseq,'InstallExecuteSequence')==M['idt_rows']((D/'InstallExecuteSequence.idt').read_bytes(),'InstallExecuteSequence')
  assert M['idt_rows'](chainer,'MsiEmbeddedChainer')==M['idt_rows'](M['chainer_idt'](),'MsiEmbeddedChainer')
  assert M['idt_rows'](afterseq,'InstallExecuteSequence')[:len(M['idt_rows'](seq,'InstallExecuteSequence'))]==M['idt_rows'](seq,'InstallExecuteSequence')
  report.update(status='PASS_ACTUAL_LOCAL_IMPORT_EXPORT_FRAGMENT_NOT_PRODUCT',finalTablesExact=True,originalStandardSequenceRowsPreserved=True,originalCompilerWarningsRejected=True,normalHelperProvenance='NOT_PRESENT_TEXT_SENTINEL_NOT_AN_EXECUTABLE',windowsExecuted=False,msiExecuted=False,productMSIValidated=False,finalMSI=f(D/'fragment.msi'),intermediateMSI=f(D/'fragment-intermediate.msi'))
 except Exception as error:report.update(status='FAIL_ACTUAL_LOCAL_FORMAT_FRAGMENT',failureType=type(error).__name__,failure=str(error))
 finally:report['elapsedSeconds']=time.monotonic()-started;save()
 print(json.dumps(dict(status=report['status'],report=str(D/'fragment-first.json'))))
 return 0 if report['status'].startswith('PASS_') else 1
if __name__=='__main__':raise SystemExit(main())
