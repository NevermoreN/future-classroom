from pathlib import Path
import base64,datetime,hashlib,json,re,stat

BASE=Path('/tmp/fc-round45-ci-input-domains-evidence')
ROOT=Path('/workspace/共享/Eliora/未来教室计划')
OUT=BASE/'archive-candidate-manifest-first.json'
EXPECTED='ed26c9932325d377bbbf4eaf63aad29d75fc0cc8ad77561b1121f3389024c418'
sha=lambda b:hashlib.sha256(b).hexdigest()
load=lambda n:json.loads((BASE/n).read_text(encoding='utf-8-sig'))
assert not OUT.exists()
checks=[]
def check(name,ok):
 checks.append({'name':name,'status':'PASS' if ok else 'FAIL'});assert ok,name
source=ROOT/'scripts/windows-msvc-ci.ps1';original_source=source.read_bytes()
check('current main exact source ed26',sha(original_source)==EXPECTED)
check('preserved frozen after source same ed26',(BASE/'windows-msvc-ci.after.ps1').read_bytes()==original_source)
model=load('input-domain-model-final.json');ps=load('windows-input-helpers-first.json');after1=load('windows-input-helpers-after-first.json');after2=load('windows-input-helpers-after-second.json');freeze=load('windows-helpers-freeze-first.json');review=load('service-readonly-review-first.json');gap=load('product-gap-readonly-first.json');integration=load('root-main-integration-first.json')
check('42 original models source binding and pass',model['source_sha256']==EXPECTED and model['status']=='PASS_PYTHON_MODEL_AND_LOCAL_GIT_ONLY' and len(model['checks'])==42 and all(x['passed'] is True for x in model['checks']))
check('41 actual Windows extracted helpers no CI/compiler execution',ps['status']=='PASS' and ps['exitCode']==0 and ps['result']['source_sha256']==EXPECTED and ps['result']['full_source_ast_errors']==0 and len(ps['result']['checks'])==41 and all(x['passed'] is True for x in ps['result']['checks']) and ps['result']['ci_entrypoint_executed'] is False and ps['result']['compiler_executed'] is False)
check('PS actual owned fixture root removed',ps['result']['root_removed'] is True)
check('first after audit local failure retained',after1['status']=='FAIL' and 'localFirstParseFailure' in after1)
check('new second after audit actualexit0 same process absent roots0 sshd',after2['status']=='PASS' and after2['exitCode']==0 and after2['originalProcessOwner']==after1['originalProcessOwner'] and after2['observed']['originalProcessAlive'] is False and after2['observed']['matchingFixtureRootCount']==0 and after2['observed']['matchingFixtureRoots']==[] and after2['observed']['sshd']=='Running')
check('local owned sparse fixture removed original evidence',model['cleanup']['owned_sparse_fixture_removed'] is True)
check('frozen helper cleanup and original first after caveat retained',freeze['source_sha256']==EXPECTED and freeze['production_source_unchanged_since_freeze'] is True and 'reconstructed' in freeze['local_evidence_errors_preserved'])
check('root main integration exact ed26',integration['sha256']==EXPECTED)
check('readonly service review exact original22checks/9validatorareas',review['sourceSha256']==EXPECTED and review['status']=='READ_ONLY_REVIEW_NO_CONFIRMED_BLOCKER_FOUND' and len(review['checks'])==22 and len(review['validatorMigrationChecklist'])==9)
check('readonly product gap original17distinct source bindings',gap['status']=='READONLY_PRODUCT_GAP_AUDIT_COMPLETE_NOT_RUNTIME_VALIDATION' and gap['source_read_before_after_unchanged'] is True and len(gap['evidence_bindings'])==len({x['path'] for x in gap['evidence_bindings']})==17)
patterns={
 'privateKey':re.compile(rb'-----BEGIN (?:[A-Z0-9 ]* )?PRIVATE KEY-----'),
 'githubToken':re.compile(rb'\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})'),
 'openaiToken':re.compile(rb'\bsk-(?:proj-)?[A-Za-z0-9_-]{40,}'),
 'literalAuthorization':re.compile(rb'(?im)\bAuthorization[\x22\x27]?\s*[:=]\s*[\x22\x27]?(?:Bearer|Basic)\s+[A-Za-z0-9+/=_-]{20,}'),
 'literalStopCapability':re.compile(rb'"stopCapability"\s*:\s*"[0-9a-f]{64}"'),
 'signedExportUrl':re.compile(rb'https://[^\s"<>]*oaiusercontent\.com/[^\s"<>]*[?&]sig='),
}
findings=[];decoded=[];decoded_seen=set()
ps_literal=re.compile(r"FromBase64String\s*\(\s*'([A-Za-z0-9+/=\r\n]+)'\s*\)",re.I)

def examine(raw,path,kind,depth=0):
 assert depth<=12,'unexpected embedded data recursion'
 for name,pattern in patterns.items():
  if pattern.search(raw):findings.append({'path':path,'kind':kind,'pattern':name})
 try:text=raw.decode('utf-8-sig')
 except UnicodeError:
  try:text=raw.decode('utf-16-le')
  except UnicodeError:return
 try:value=json.loads(text)
 except ValueError:value=None
 if value is not None:json_fields(value,path,depth)
 for match in ps_literal.finditer(text):
  literal=re.sub(r'\s+','',match[1])
  if len(literal)<100:continue
  data=base64.b64decode(literal,validate=True)
  decode(data,path,'PowerShell single-quoted FromBase64String embedded source',depth+1)

def decode(data,path,kind,depth):
 digest=sha(data);decoded.append({'container':path,'kind':kind,'decodedBytes':len(data),'decodedSha256':digest})
 if digest not in decoded_seen:
  decoded_seen.add(digest);examine(data,path,kind,depth)

def json_fields(x,path,depth):
 if isinstance(x,dict):
  for key,value in x.items():
   if isinstance(value,str) and ('base64' in key.lower() or key.lower()=='encodedcommand'):
    if len(value)>=16 and re.fullmatch(r'[A-Za-z0-9+/=\r\n]*',value):
     data=base64.b64decode(re.sub(r'\s+','',value),validate=True)
     decode(data,path,'JSON '+key,depth+1)
   elif isinstance(value,(dict,list)):json_fields(value,path,depth)
 elif isinstance(x,list):
  for value in x:json_fields(value,path,depth)
files=[];excluded=[]
for p in sorted(BASE.rglob('*')):
 if p.is_dir():continue
 relative=p.relative_to(BASE).as_posix()
 check('ordinary owned evidence file '+relative,stat.S_ISREG(p.lstat().st_mode) and not p.is_symlink())
 if relative=='.owner':excluded.append({'path':relative,'reason':'Temporary owned directory marker, not archive evidence'});continue
 check('no ZIP EXE private control expected '+relative,p.suffix.lower() not in ['.zip','.exe','.dll','.pdb'] and not p.name.endswith('.control.json'))
 raw=p.read_bytes();check('no executable magic '+relative,not raw.startswith(b'MZ'))
 examine(raw,relative,'original bytes')
 row={'path':relative,'bytes':len(raw),'sha256':sha(raw)}
 if p.suffix=='.bin':row.update(kind='raw-original-Git-NUL-metadata',nulCount=raw.count(b'\0'),normalization='NONE')
 files.append(row)
check('no literal or decoded credential finding',not findings)
check('embedded actual CI source decoded and scanned',any(x['kind']=='PowerShell single-quoted FromBase64String embedded source' and x['decodedSha256']==EXPECTED and x['decodedBytes']==len(original_source) for x in decoded))
check('both Git NUL originals retained exact identical bytes',(BASE/'head-tree-original.bin').read_bytes()==(BASE/'head-tree-original-final.bin').read_bytes() and (BASE/'head-tree-original.bin').read_bytes().count(b'\0')==6940)
check('all snapshot originals remain unchanged',all((BASE/x['path']).stat().st_size==x['bytes'] and sha((BASE/x['path']).read_bytes())==x['sha256'] for x in files))
check('main source unchanged after archive preparation',source.read_bytes()==original_source)
assert {x['path'] for x in files}|{x['path'] for x in excluded}=={p.relative_to(BASE).as_posix() for p in BASE.rglob('*') if p.is_file()}
report={'schemaVersion':1,'status':'READY_FOR_ARCHIVE_REVIEW_NOT_ARCHIVED','observedAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Point-in-time complete R45 evidence inventory and secret-pattern scan, including actual embedded PS source. Main source unchanged; no archive copy, scratch/root cleanup, Windows, Cargo, Git or CI performed.','sourceRoot':str(BASE),'suggestedDestination':'开发计划/验证记录/2026-11-01-round45-ci-input-domains','files':files,'fileCount':len(files),'totalBytes':sum(x['bytes'] for x in files),'indexExcludedFromOwnLedger':True,'excluded':excluded,'productionSourceSha256':EXPECTED,
 'credentialScan':{'status':'NO_LITERAL_OR_DECODED_PATTERN_FINDINGS','patterns':list(patterns),'findings':findings,'decodedOccurrenceCount':len(decoded),'uniqueDecodedPayloads':len(decoded_seen),'decodedPayloadEvidence':decoded,'scope':'All original bytes, JSON base64/encodedCommand, recursively found single-quoted PowerShell FromBase64String source were scanned. Decoded source bytes were not written or printed. Pattern hygiene is not proof against arbitrary unknown secret formats.'},
 'layers':{'pythonModel':{'cases':42,'status':model['status'],'fixtureRemoved':True},'actualWindowsExtractedHelpers':{'cases':41,'status':ps['result']['status'],'powershell':ps['result']['powershell'],'fullASTErrors':0,'ciEntrypointExecuted':False,'compilerExecuted':False,'aggregate256MiBActualAllocation':'NOT_RUN_MODEL_PLUS_ORIGINAL_GUARD'},'firstAfterAudit':{'status':'FAIL_PRESERVED','exitCodeCaveat':'First report reconstructed an unsaved exitCode; first parsing typo and progress-CLIXML stderr criterion retained, not independent terminal proof.'},'secondAfterAudit':{'status':'PASS','actualExitCode':0,'processAbsent':True,'fixtureRootCount':0,'sshd':'Running'},'readonlyReview':{'checks':22,'validatorAreas':9,'status':review['status']},'readonlyProductGap':{'sourceBindings':17,'status':gap['status'],'scope':'Product planning/read-only gap audit, not implementation or runtime evidence.'}},
 'budgetFacts':{'ciInvokeLoggedDefaultSeconds':120,'ciWorkspaceTestCompileSeconds':2400,'ciProductBuildSeconds':1200,'ciClippySeconds':1500,'jobs':2,'localRustMatrixSeconds':900,'allOriginalCiCargoFlagsAndTimeoutsUnchanged':True,'note':'900 belongs to parent local Rust matrix, not a universal CI command timeout.'},
 'cleanupScope':{'ownedLocalSparseFixtureRemoved':True,'ownedWindowsFixtureRootsZero':True,'ownedOriginalWindowsHelperProcessAbsent':True,'sourceWorktreeRetainedForParent':True,'evidenceRetained':True,'rootOrScratchDeletedByThisTask':False,'windowsSshAndAuthorization':'UNTOUCHED'},
 'archivePerformed':False,'mainRepositoryModified':False,'checks':checks,'limitations':['This is not formal archive INDEX or exact-byte copy proof; parent must copy and independently rehash archived members.','No new full HostedPS7.2 CI/compiler/runtime validation is inferred from PS5.1 AST-extracted helper tests.','Historical Git metadata, actual SHA subset and current checkout lengths are different evidence domains; never reuse old allGit rawSHA language.','Original P0 strict5s/ordinary listeneroff/threeOS and visible device acceptance gates remain separate.']}
with OUT.open('x') as f:f.write(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':report['status'],'fileCount':report['fileCount'],'totalBytes':report['totalBytes'],'checks':len(checks),'decodedOccurrences':len(decoded),'uniqueDecoded':len(decoded_seen),'credentialFindings':len(findings),'reportSha256':sha(OUT.read_bytes())},ensure_ascii=False))
