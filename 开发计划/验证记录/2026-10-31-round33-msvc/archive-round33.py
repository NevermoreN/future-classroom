from pathlib import Path
import json,hashlib,subprocess,re,datetime,stat

repo=Path.cwd();base=Path('/tmp/fc-round33-msvc-evidence');dest=repo/'开发计划/验证记录';prefix='2026-10-31-round33-msvc';target=dest/prefix
summary_path=dest/'2026-10-31-round33-msvc-stage-summary.json';index_path=dest/'2026-10-31-round33-stage-progress-index.json'
assert not any(p.exists() for p in [target,summary_path,index_path])
sha=lambda b:hashlib.sha256(b).hexdigest()
load=lambda n:json.loads((base/n).read_bytes())
v=load('artifact-verification-first.json');w=load('windows-msvc-full-first.json');tls=load('windows-msvc-tls-first.json');pkg=load('development-package-first.json');after=load('windows-after.json');pure=load('win7-ci-powershell-first.json')['result']
assert v['status'].startswith('PASS_') and w['status']==w['cleanup']==tls['status']==tls['cleanup']==after['status']==pure['status']=='PASS'
assert pkg['status']=='PASS_DEVELOPMENT_PACKAGE_ONLY' and pkg['temporaryPackagingRootsRemoved']
counts=[]
for t in w['tests']:
 assert t['exitCode']==0
 lines=t['output'] if isinstance(t['output'],list) else [t['output']]
 matches=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', '\n'.join(lines))
 assert len(matches)==1 and matches[0][1:] == ('0','0')
 counts.append({'name':t['name'],'passed':int(matches[0][0]),'sha256':t['binarySha256']})
assert len(counts)==21 and len({t['name'] for t in counts})==21
binaries={Path(r['path']).name:r['sha256'] for r in v['binaryFacts'] if r['role']=='tests'}
assert {r['name']:r['sha256'] for r in counts}==binaries
old_indexes={'2026-10-31-round30-stage-progress-index.json':'7ad3e92fa452e00b3060a1eb1149934d84625abef0657372bde008edab600dfa','2026-10-31-round31-stage-progress-index.json':'10bbb497944bbd2a509622261d4e29f9ef5ed4240a21ec2bfb7bcf71b665b25e','2026-10-31-round32-stage-progress-index.json':'891087fb677173b8cfec9b0c43961ba28f6ba6ac5b95f8e9e43f24726e4385e7','2026-10-31-round30-late-originals-index.json':'75e94198db491ec4d89dc4c173ff906078073257b75b8f54a67b49b899c4ccd4'}
for n,h in old_indexes.items():assert sha((dest/n).read_bytes())==h
rows=[]
private=re.compile(rb'-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----\s+[A-Za-z0-9+/=\r\n]{32,}-----END (?:[A-Z]+ )?PRIVATE KEY-----')
credentials=re.compile(rb'(?:gh[pousr]_[A-Za-z0-9_]{25,}|github_pat_[A-Za-z0-9_]{30,}|sk-proj-[A-Za-z0-9_-]{25,}|[?&](?:sig|X-Amz-Signature)=[A-Za-z0-9%+/=]{20,})')
for p in sorted(base.rglob('*')):
 assert not p.is_symlink()
 if p.is_dir():continue
 assert p.is_file() and p.suffix.lower() not in ['.exe','.zip','.pyc','.pfx','.p12']
 data=p.read_bytes();assert len(data)<=64*1024*1024;data.decode('utf-8-sig');assert not private.search(data) and not credentials.search(data)
 relative=p.relative_to(base);rows.append({'source':str(p),'path':prefix+'/'+relative.as_posix(),'bytes':len(data),'sha256':sha(data)})
assert sum(r['bytes'] for r in rows)<256*1024*1024
summary={'status':'PASS_STAGE_PROGRESS_NOT_FULL_PRODUCT','recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'goal':'ACTIVE: 按照计划完成全部开发。','sourceSha':v['sourceSha'],'scope':'Same original Round32 Rust source modern MSVC CI compile artifact, actual authorized Win11 runtime/TLS, isolated Win7 CI implementation pure checks and unvalidated development packaging; no Rust behavior change.','artifact':v['zip'],'originalCommands':len(v['originalResult']['commands']),'originalCommandDurationsSeconds':{r['name']:r['duration_seconds'] for r in v['originalResult']['commands']},'productionRustCargoInputs':v['gitInputVerification']['productionRustCargoCount'],'historicalEvidenceSourceInputs':v['gitInputVerification']['historicalEvidenceSourceCount'],'windows':{'platform':w['platform'],'testExecutables':len(counts),'testsPassed':sum(t['passed'] for t in counts),'failed':0,'ignored':0,'tests':counts,'voiceOutputSteps':w['voiceOutputNativeSteps'],'voiceOutputElapsedMs':w['voiceOutputElapsedMs'],'normalSha256':w['exeSha256'],'cleanup':w['cleanup']},'tls':{'status':tls['status'],'events':len(tls['events']),'normalSha256':tls['exeSha256'],'native':tls['native'],'cleanup':tls['cleanup'],'cleanupDetails':tls['cleanupDetails']},'win7CiPureChecks':{'cases':pure['cases'],'assertions':pure['assertions'],'powershell':pure['powershell'],'sourceSha256':pure['text_source_sha256'],'fullHelperStillRequires':'Hosted PowerShell>=7.2','hostedNewHelperCi':'NOT_RUN at archive time; dispatch only after final ordinary sync','win7Runtime':'NOT_RUN'},'developmentPackage':pkg['package'],'developmentPackageStatus':pkg['manifest']['status'],'originalFailuresRetained':['nightly fmt first: missing cargo-fmt component; installed component and second fmt passed','artifact download first:1200second budget exhausted at1205s, prefix retained','resume first:stdin EOF before any HTTP request','resume second:HTTPError; original signed link expired; exact HTTP code not captured'],'downloadRecovery':'Fresh same-artifact file reference; third separate report/file,206 exact tail, original prefix immutable and complete original artifact SHA matched; no CI redispatch.','windowsFinalAudit':{'path':prefix+'/windows-after.json','sha256':sha((base/'windows-after.json').read_bytes()),'status':after['status']},'unchangedOldIndexes':old_indexes,'remainingGates':['Original strict TCP five-second FAIL and accepted-clock question unresolved; production listener disabled','Actual Win7 SP1x86/Win10 and visible Seewo/touch/DPI/physical audio','New Win7/shared modern helper hosted CI','Installation/login/upgrade and72hour acceptance; no Release/tag'],'sourceMapping':rows}
for row in rows:
 p=base/Path(row['path']).relative_to(prefix);data=p.read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256'];q=dest/row['path'];q.parent.mkdir(parents=True,exist_ok=True)
 with q.open('xb') as f:f.write(data)
 assert q.read_bytes()==data
with summary_path.open('x') as f:f.write(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
entries=[{'path':r['path'],'bytes':r['bytes'],'sha256':r['sha256']} for r in rows];d=summary_path.read_bytes();entries.append({'path':summary_path.name,'bytes':len(d),'sha256':sha(d)})
index={'status':'PASS_IMMUTABLE_STAGE_INDEX','scope':'Explicit original Round33 source/evidence plus stage summary; historical errors and prepared/not-run observations retained byte-for-byte. No EXE/ZIP/private credentials.','files':entries,'fileCount':len(entries),'logicalBytes':sum(r['bytes'] for r in entries),'oldIndexes':old_indexes}
with index_path.open('x') as f:f.write(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':summary['status'],'files':len(entries),'bytes':index['logicalBytes'],'indexSha256':sha(index_path.read_bytes()),'windowsTests':summary['windows']['testsPassed'],'package':pkg['package']},ensure_ascii=False),flush=True)
