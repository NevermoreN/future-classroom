from pathlib import Path
import json,hashlib,base64,re
E=Path(__file__).parent
checks=[]; files={}
def fact(p):
 p=Path(p); b=p.read_bytes(); out={'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()};files[str(p)]=out;return out
def check(name,ok):
 checks.append({'name':name,'passed':bool(ok)})
 if not ok: raise AssertionError(name)
def load(name):
 p=E/name;fact(p);return json.loads(p.read_text(encoding='utf-8-sig'))
source=E/'implementation-owner-fifth/run-windows-probe.ps1';src=fact(source)
check('Exact fifth runner',src['sha256']=='b342315ac86a603407814818eeffd6f6b9fb37e7a5859883c8dfdc33034191b5')
for mode in ['preflight','upload','execute','collect']:
 r=load('root-native-'+mode+'-second.json')
 check(mode+' original exit0/source binding',r['sshExitCode']==0 and r['sourceSha256']==src['sha256'])
 for stream in ['stdout','stderr']:
  f=fact(E/('root-native-'+mode+'-second.'+stream));check(mode+' '+stream+' actual bytes/hash',f['bytes']==r[stream+'Bytes'] and f['sha256']==r[stream+'Sha256'])
collect=json.loads((E/'root-native-collect-second.stdout').read_bytes())
for row in collect['files']:
 raw=base64.b64decode(row['base64'],validate=True);f=fact(E/'windows-original-second'/row['name'])
 check(row['name']+' captured base64/remote bytes/hash/local original',raw==(E/'windows-original-second'/row['name']).read_bytes() and len(raw)==row['bytes']==f['bytes'] and row['sha256']==f['sha256'])
upload=json.loads((E/'root-native-upload-second.stdout').read_bytes())
for row in upload['files']:
 if row['name']=='.owner':continue
 p=source if row['name']=='run-windows-probe.ps1' else E/'build-fifth'/row['name']
 f=fact(p);check('Uploaded '+row['name']+' bytes/hash',row['bytes']==f['bytes'] and row['sha256']==f['sha256'])
d=load('windows-original-second/windows-observations-first.json')
check('Unique four operation order',[x['name'] for x in d['operations']]==['install-commit','repair-commit','uninstall-commit','install-rollback'] and len(set(x['operation'] for x in d['operations']))==4)
check('Original overall narrow contract and cleanup',d['status']=='OBSERVED_FOUR_DIRECT_OPERATIONS_NOT_PRODUCT_ACCEPTANCE' and d['directSinglePackageContractSatisfied'] and d['cleanup']=='PASS_OWN_PRODUCT_ABSENT_AND_NONCE_REGISTRY_REMOVED' and d['unknownPhysicalOperation'] is False)
check('NEW root same markers',d['registryCreation']['disposition']==1 and d['registryCreation']['actualNew'] and d['registryCreation']['markersComplete'] and d['registryCreation']['parentCloseError']==0)
summary=[];warning=None
for idx,o in enumerate(d['operations']):
 g=o['guardianEvidence'][0];p=o['guardianObserved'][0]
 check(o['name']+' exact single guardian',len(o['guardianEvidence'])==len(o['guardianObserved'])==1 and g['Pid']==p['pid'] and g['CreationFileTime']==p['creationFileTime'] and g['key']=='Guardian-'+str(p['pid']) and g['ActualSid']==d['expectedSid'] and p['actualImageSha256']==d['guardianSha256'])
 check(o['name']+' original handles actual exit and no substituted result',o['physicalMsiExit'] is True and o['msiExitCode']==0 and o['msiPid']>0 and o['msiStartFileTime']>0 and p['originalProcessHandleRetained'] is True and p['actualExit'] is True and p['exitCode']==0 and o['completion']=='OBSERVED_ORIGINALS_ONLY')
 check(o['name']+' exact Join/End and decision',g['JoinReturn']==g['EndReturn']==g['AboutToReturnCode']==0 and g['EndEntered']==g['EndReturned']==1 and g['RequestedDecision']==o['decision']==(0 if idx==3 else 1) and g['Stage']=='EndReturned' and g['ArgCount']==2 and g['TransactionId']>0)
 check(o['name']+' original state/payload',o['productBefore']['productState']==[-1,5,5,-1][idx] and o['productAfter']['productState']==[5,5,-1,-1][idx] and o['payloadAfter']['valuePresent']==(idx<2))
 for k in ['productBefore','productAfter']:
  q=o[k];j=json.loads(q['stdout']);check(o['name']+' '+k+' physical query/identity',q['actualExit'] is True and q['exitCode']==0 and q['stderr']=='' and j['productCode']==d['productCode'] and j['nonce']==d['nonce'] and j['productState']==q['productState'])
 logfile=E/'windows-original-second'/(o['name']+'.msi.log');f=fact(logfile);raw=logfile.read_bytes();lines=raw.decode('utf-16').splitlines()
 check(o['name']+' original UTF16 log bound',raw.startswith(b'\xff\xfe') and f['bytes']==o['logBytes'] and f['sha256']==o['logSha256'] and 'MainEngineThread is returning 0' in '\n'.join(lines))
 summary.append({'name':o['name'],'operation':o['operation'],'msiPid':o['msiPid'],'msiCreationFileTime':o['msiStartFileTime'],'guardianPid':p['pid'],'guardianCreationFileTime':p['creationFileTime'],'sid':g['ActualSid'],'transactionId':g['TransactionId'],'decision':o['decision'],'msiExitCode':o['msiExitCode'],'guardianExitCode':p['exitCode'],'joinReturn':g['JoinReturn'],'endReturn':g['EndReturn'],'productBefore':o['productBefore']['productState'],'productAfter':o['productAfter']['productState'],'log':f,'logLineCount':len(lines)})
 if idx==3:
  at=[i for i,l in enumerate(lines) if 'Error in rollback skipped.' in l];check('Rollback error preserved exactly once',len(at)==1 and 'Return: 5' in lines[at[0]])
  i=at[0];warning={'line':i+1,'context':[{'line':j+1,'text':lines[j]} for j in range(i-4,min(i+8,len(lines)))],'conclusion':'Native rollback reported a skipped error after ComponentUnregister and End. It is not silently treated as clean rollback. The log alone does not identify the failing primitive or prove registration/cache absence; parent product state -1 and fixture payload absence are narrower observations.'}
cleanup=load('root-native-successful-cleanup-first.json');c=cleanup['original']
check('Root subsequent owned cleanup original',cleanup['sshExitCode']==0 and c['directoryAbsent'] and c['registryAbsent'] and c['serviceKilled'] is False and c['productQuery']['actualExit'] and c['productQuery']['exitCode']==0 and json.loads(c['productQuery']['stdout'])['productState']==-1)
ids={(x['pid'],x['created']) for x in c['originalProcessesAbsent']['value']};expect={(x['msiPid'],x['msiStartFileTime']) for x in d['operations']}|{(x['guardianObserved'][0]['pid'],x['guardianObserved'][0]['creationFileTime']) for x in d['operations']};check('Exact eight original PID/creation absence receipts',ids==expect and len(ids)==8)
for row in c['deletedFiles']['value']:
 if row['name']=='.owner':continue
 target=source if row['name']=='run-windows-probe.ps1' else E/'windows-original-second'/row['name'] if row['name'].endswith('.log') or row['name']=='windows-observations-first.json' else E/'build-fifth'/row['name']
 check('Deletion identity '+row['name'],fact(target)['sha256']==row['sha256'])
for n in ['postcheck-owned-msi-second-first.ps1','postcheck-owned-msi-paths-second-first.json',Path(__file__).name]:fact(E/n)
report={'schemaVersion':1,'status':'PASS_NARROW_FOUR_OPERATIONS_WITH_UNRESOLVED_ROLLBACK_DIAGNOSTIC','scope':'Independent local original-byte verification only. No new Windows execution, MSI installation, production owner integration or whole-product lifecycle acceptance.','checks':checks,'checkCount':len(checks),'operations':summary,'rollbackDiagnostic':warning,'cleanupDistinction':{'alreadyObserved':'Final product query -1, own NEW registry root absent, exact eight original MSI/guardian identities absent, ten exact owned input/log/report files deleted by root.','notYetObserved':'Original Installer cache/script/SourceHash and exact product/component/upgrade registration addresses; supplied read-only postcheck is PREPARED_NOT_RUN.','notClaimed':'Rollback error-free; every extracted embedded executable/temp path accounted; no stale system MSI registration; production maintenance gate, multi-package Join/End, full startup protection, Run ownership, Win7/Win10 or actual product upgrade/uninstall.'},'followup':{'script':'postcheck-owned-msi-second-first.ps1','paths':'postcheck-owned-msi-paths-second-first.json','scope':'11 exact file/directory addresses and 13 exact registry addresses in both views; no shared-state enumeration/deletion. No dependency on deleted fixture binaries.'},'files':list(files.values())}
out=E/'independent-native-second-first.json';out.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'report':fact(out),'checks':len(checks),'scope':report['status']}))
