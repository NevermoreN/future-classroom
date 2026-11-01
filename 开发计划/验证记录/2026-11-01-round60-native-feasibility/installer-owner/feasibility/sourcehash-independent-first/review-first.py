from pathlib import Path
import json,hashlib
O=Path(__file__).parent;P=O.parent

def info(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def load(p):return json.loads(p.read_bytes())
def lines(p):
 b=p.read_bytes()
 return (b.decode('utf-16') if b.startswith((b'\xff\xfe',b'\xfe\xff')) else b.decode('utf-8')).splitlines()
fixture=load(P/'build-fifth/fixture.json');product=fixture['productCode'];native=P/'windows-original-second';backup=native/'sourcehash-owned-original.bin';old=info(backup)
assert old['bytes']==20480 and old['sha256']=='f4beae102ae1cc02fdf9583ed7f21a34258310ab73b077862fab47d11a05b79e'
assert backup.read_bytes()[:8]==bytes.fromhex('d0cf11e0a1b11ae1')
expected='C:\\Windows\\Installer\\SourceHash'+product
pre=load(P/'root-native-postcheck-second-first.json');f=pre['original']['files'];regs=pre['original']['registry'];present=[r for r in f if r['present']]
assert len(present)==1 and present[0]['path']==expected and present[0]['sha256']==old['sha256'] and present[0]['bytes']==old['bytes'] and present[0]['reparsePoint'] is False
assert all(not r['present'] for r in regs)
evidence=[]
for name in ['install-commit.msi.log','repair-commit.msi.log','uninstall-commit.msi.log','install-rollback.msi.log']:
 p=native/name;a=lines(p)
 chosen=[{'line':i,'text':v} for i,v in enumerate(a,1) if ('SECREPAIR' in v and ('Hash Database:' in v or 'SourceHash database' in v)) or 'Product Code from property table after transforms:' in v or ('Product:' in v and fixture['nonce'] in v)]
 evidence.append({'identity':info(p),'encoding':'UTF16_BOM' if p.read_bytes().startswith((b'\xff\xfe',b'\xfe\xff')) else 'UTF8','matches':chosen})
assert any(expected in row['text'] for e in evidence for row in e['matches'])
commands=load(O/'commands-first.json')+[load(O/'sourcehash-command-first.json')]
for c in commands:
 assert c['cwd']==str(O)
cmdraw=[]
for n in ['tables','streams','suminfo','sourcehash-table']:
 cmdraw.extend([info(O/(n+'.stdout.log')),info(O/(n+'.stderr.log'))])
assert (O/'tables.stdout.log').read_text().splitlines()==['_SummaryInformation','_ForceCodepage','SourceHash']
assert (O/'streams.stdout.log').read_bytes()==b''
assert commands[-1]['exitCode']==1 and b'Table size is invalid 8/6' in (O/'sourcehash-table.stderr.log').read_bytes()
backupMeta=load(P/'root-native-sourcehash-backup-first.json');assert backupMeta['sha256']==old['sha256'] and backupMeta['bytes']==20480 and backupMeta['path']==expected
cleanup=load(P/'root-native-sourcehash-cleanup-first.json');assert cleanup['sshExitCode']==0 and cleanup['original']['status']=='PASS_EXACT_OWNED_SOURCEHASH_REMOVED' and cleanup['original']['beforeSha256']==old['sha256'] and cleanup['original']['absent'] is True
final=load(P/'root-native-postcheck-second-final-first.json');finalOriginal=final['original'];finalFiles=finalOriginal['files'];finalRegs=finalOriginal['registry']
report={'status':'PASS_ATTRIBUTION_TO_EXACT_SYNTHETIC_FIXTURE_ONLY','scope':'Only local backed-up SourceHash CFB and existing original logs/reports. Root alone performed actual Windows cleanup. No remote/Windows execution by this reviewer.',
'fixture':info(P/'build-fifth/fixture.json'),'productCode':product,'nonce':fixture['nonce'],'backedOriginal':old,'oleCfbHeader':'d0cf11e0a1b11ae1','originalWindowsPath':expected,
'rawContentObservations':{'msiinfoTables':['_SummaryInformation','_ForceCodepage','SourceHash'],'msiinfoStreams':[],'suminfoOutputBytes':0,'asciiFixtureMsiOffset':backup.read_bytes().find(b'fixture.msi'),'internalProductCodeAsciiOffset':backup.read_bytes().find(product.encode('ascii')),'sourceHashTableExport':'FAIL_ACTUAL_EXIT1 Table size is invalid8/6; table contents NOT_DECODED. No corruption or successful hash-row validation claim follows from this library error.'},
'commands':commands,'rawToolStreams':cmdraw,'explicitOwnedCwd':str(O),'backupMetadataOriginal':{'identity':info(P/'root-native-sourcehash-backup-first.json'),'original':backupMeta},
'logBinding':evidence,'ownershipConclusion':'The exact random fixture ProductCode appears in the original Installer-generated Hash Database pathname, from the owned fixture MSI log. Original create timestamp follows that log by about1ms, the cached content has SourceHash table and fixture.msi string, and the final actual registration/payload/cache postcheck isolated this as the only present exact-address residue. This is sufficient attribution for this already-byte-backed single synthetic test file; internal table decode is not asserted.',
'originalPostcheck':{'identity':info(P/'root-native-postcheck-second-first.json'),'exactAddresses':len(f)+len(regs),'presentCount':len(present),'present':present,'allExactRegistrationAddressesAbsent':True},
'cleanupBoundary':{'actionSupported':'Root may remove ONLY this exact non-reparse file after rechecking its saved20480B/SHA identity and absent own product registrations. Preserve original backup and original logs. This is reversible test cleanup, not a generic installer-cache operation.','forbiddenExpansion':['No wildcard or folder cleanup in Windows Installer cache','No foreign MSI/MSP/SourceHash files','No manual Installer registry editing','No restarting/killing shared msiserver','No cache schema mutation/rebuild'], 'rootActualCleanupOriginal':{'identity':info(P/'root-native-sourcehash-cleanup-first.json'),'status':cleanup['original']['status'],'exactBeforeSha256':cleanup['original']['beforeSha256'],'afterAbsent':True},'rootActualPostcheckFinal':{'identity':info(P/'root-native-postcheck-second-final-first.json'),'status':finalOriginal.get('status'),'exactAddresses':len(finalFiles)+len(finalRegs),'presentCount':sum(bool(v['present']) for v in finalFiles+finalRegs),'note':'This is a read of root-produced original audit, not a new Windows observation by this helper.'}},
'primarySources':[{'url':'https://support.microsoft.com/en-us/servicing/os/windows/2020/04/unexpected-uac-prompt-after-you-install-update-2918614-in-windows','publisher':'Microsoft Support','supports':'Windows Installer uses a hash cache to validate installation files during repair.','doesNotSupport':'No published SourceHash internal table layout or general deletion authorization was obtained.'},{'url':'https://learn.microsoft.com/en-us/troubleshoot/windows-client/application-management/missing-windows-installer-cache','publisher':'Microsoft Learn','supports':'Installer cache loss can disrupt servicing of real installed products; this review does not propose broad cache cleanup.'}],
'backupUnchangedAfterRead':info(backup)==old,'limitations':['SourceHash row export failed in owned libmsi; failure stdout/stderr preserved.','Filename, exact original native log path, random fixture identity and timestamps supply attribution; no fabricated embedded ProductCode property.', 'No general SourceHash format or complete Installer internal-state enumeration proof.', 'MSI lifecycle feasibility/rollback limitations remain separate.']}
assert info(backup)==old
out=O/'review-first.json'
with out.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'report':info(out),'status':report['status'],'exactPath':expected,'originalPostcheckAddresses':len(f if False else regs)+len(present),'rootCleanupObserved':cleanup['original']['status'],'finalRemaining':report['cleanupBoundary']['rootActualPostcheckFinal']['presentCount']},ensure_ascii=False))
