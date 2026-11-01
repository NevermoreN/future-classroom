from pathlib import Path
import json,hashlib,re
P=Path(__file__).parent;O=P/'windows-original-first'
def info(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
j=json.loads((O/'native-first.json').read_bytes());raw=(O/'native-first.stdout.log').read_bytes();assert hashlib.sha256(raw).hexdigest()==j['stdoutSha256'] and len(raw)==j['stdoutBytes']
assert (O/'native-first.stderr.log').read_bytes()==b'' and j['expectedExeSha256']==info(P/'source-final/registry-value-intent.exe')['sha256']
assert j['physicalExit'] is True and j['exitCode']==72
parsed=[]
for line,txt in enumerate(raw.decode('ascii').splitlines(),1):
 m=re.fullmatch(r'RWI_EVENT pid=(\d+) seq=(\d+) tick_ms=(\d+) name=([A-Za-z0-9_]+) code=(\d+) detail=(\d+)',txt)
 if m:parsed.append({'line':line,'pid':int(m[1]),'sequence':int(m[2]),'tickMillis':int(m[3]),'name':m[4],'code':int(m[5]),'detail':int(m[6])})
assert len(parsed)==len(j['events'])
root=j['pid'];starts=[e for e in parsed if e['pid']==root and e['name']=='supervisor_case_begin'];assert [chr(e['detail']) for e in starts]==list('ABCDEFGH')
cases=[];drows=[]
for i,e in enumerate(starts):
 end=starts[i+1]['line'] if i+1<len(starts) else len(raw.splitlines())+1
 group=[r for r in parsed if e['line']<=r['line']<end];cp=next(r['detail'] for r in group if r['pid']==root and r['name']=='create_child');own=[r for r in group if r['pid']==cp]
 tags=[r for r in own if r['name'] in ['NO_COMPETITOR_EXTERNAL_COMMITTED_READ_PROGRESS_OBSERVED','INITIAL_NONMATCH_PRESERVED_BY_ROLLBACK','CANCEL_OR_ORIGINAL_TIMEOUT_PRESERVED_ORIGINAL','FAIL_OR_INCONCLUSIVE_NATIVE_RESULT_NO_CAS_QUALIFICATION','BASELINE_NO_VALUE_INTENT_LOST_DISTINCT_REPLACEMENT']]
 assert len(tags)==1
 cases.append({'case':chr(e['detail']),'originalCasePid':cp,'classification':tags[0]['name'],'finalKind':[r for r in own if r['name']=='final_target_kind'],'nativeChildExit':[r for r in own if r['name']=='owned_child_exit']})
 if chr(e['detail'])=='D':drows=group;dpid=cp
own=[r for r in drows if r['pid']==dpid]
def one(n,rows=own):
 x=[r for r in rows if r['name']==n];assert len(x)==1;return x[0]
children=[r['detail'] for r in own if r['name']=='create_child'];assert len(children)==2
reader,writer=children;readRows=[r for r in drows if r['pid']==reader];writerRows=[r for r in drows if r['pid']==writer]
ordered=[one('tx_stage_expected_return'),one('ordinary_committed_read_return',readRows),one('ordinary_committed_compare_kind',readRows),one('reader_actual_exit_BEFORE_decision'),one('ordinary_set_return',writerRows),one('writer_close_case',writerRows),one('ordinary_actual_exit_BEFORE_target_delete'),one('tx_delete_target_return'),one('final_original_tx_rollback'),one('native_transaction_outcome'),one('final_query_target'),one('final_target_kind')]
assert all(a['line']<b['line'] for a,b in zip(ordered,ordered[1:]))
assert one('tx_delete_target_return')['code']==6701 and one('final_original_tx_rollback')['code']==0 and one('native_transaction_outcome')['code']==0 and one('native_transaction_outcome')['detail']==3 and one('final_target_kind')['detail']==3
assert not any(r['name'].startswith('tx_commit_') for r in own)
assert one('ordinary_actual_exit_BEFORE_target_delete')['detail']==0 and one('reader_actual_exit_BEFORE_decision')['detail']==0
# Bind the exact successful final bytes from the actual raw wire.
lines=raw.decode('ascii').splitlines();finalRow=one('final_query_target');valueLine=lines[finalRow['line']]
m=re.fullmatch(r'RWI_VALUE pid=(\d+) seq=(\d+) tick_ms=(\d+) type=(\d+) bytes=(\d+) hex=([a-f0-9]+)',valueLine);assert m and int(m[1])==dpid and int(m[4])==1
value=bytes.fromhex(m[6]);assert len(value)==int(m[5])==44 and value=='synthetic-replacement\0'.encode('utf-16le')
zeros=[r for r in parsed if r['pid']==root and r['name']=='supervisor_actual_job_active_zero'];assert len(zeros)==10
assert any(r['name']=='cleanup_verify_absent' and r['code']==2 for r in parsed) and j['cleanup']=='OBSERVED_NATIVE_EXACT_LEAF_ABSENT'
s=(P/'source-final/registry-value-intent.c').read_text().splitlines()
sourceProof=[{'line':i,'text':l} for i,l in enumerate(s,1) if 'tx_delete_target_return' in l or 'FAIL_OR_INCONCLUSIVE_NATIVE_RESULT' in l or 'DISTINCT_REPLACEMENT_RETAINED' in l]
r={'status':'CONFIRMED_D_REJECTED_INACTIVE_TRANSACTION_AND_PRESERVED_REPLACEMENT_ORIGINAL72_UNCHANGED','scope':'Only original Win11/WOW64 eight-case synthetic execution; no new Windows run/source mutation and no general CAS proof.',
'inputs':[info(O/'native-first.json'),info(O/'native-first.stdout.log'),info(O/'native-first.stderr.log'),info(P/'source-final/registry-value-intent.c'),info(P/'source-final/registry-value-intent.exe')],
'originalWholeResult':{'nativeExitCode':72,'runnerStatus':j['status'],'originalPhysicalExit':True,'pid':root,'creationFileTime':j['creationFileTime'],'allCases':'ABCDEFGH','actualJobZero':10,'cleanup':j['cleanup'],'elapsedMillis':j['elapsedMillis'],'cases':cases},
'D':{'casePid':dpid,'independentReaderPid':reader,'ordinaryWriterPid':writer,'causalRawEvents':ordered,'actualDeleteError':{'decimal':6701,'hex':'0x1a2d','symbol':'ERROR_TRANSACTION_NOT_ACTIVE','notObservedError6800':True},'rollbackReturn':0,'actualTransactionOutcome':{'code':0,'value':3,'sdkMeaning':'TransactionOutcomeAborted'},'commitAttempted':False,'final':{'type':1,'bytes':44,'hex':value.hex(),'decodedSynthetic':'synthetic-replacement'},'interpretation':'Ordinary replacement completed and original writer exit0 was observed before deletion. The requested delete was rejected because that transaction was inactive, then rollback succeeded and the exact replacement remained. This is an observed non-destructive refusal in the race, unlike the prior child-intent lost update. It does not directly identify the internal phase or why the transaction became inactive.'},
'whyOriginalFixtureFailed':{'sourceProof':sourceProof,'finding':'Line256 treats every nonzero RegDeleteValueW return as known=false; line279 then returns31 before reaching the later replacement-retained classification. The native supervisor records child31 and returns whole72. Those original failures are retained, not relabeled PASS.'},
'primaryErrorContract':{'url':'https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes--6000-8199-','6701':'Operation requested in a transaction that is no longer active.','6800':'A name is reserved for use by another transaction; this error was not the observed delete return.','authorityLimit':'Neither error name alone proves that a specific ordinary writer caused invalidation or supplies universal lock/serialization guarantees.'},
'nextCandidateDecision':{'continueScratchOnly':True,'productionQualified':False,'reason':'A made real external-read progress; B/C saw committed nonmatches and retained original type/bytes; D refused deletion and retained replacement; E red control lost replacement. This supports a narrower next falsification fixture, not adoption.',
'conditionalConflictResult':'In a new independent fixture, separate NativeFailure from ObservedRaceConflict. Only with recorded original writer success/exit, preimage comparison, no own COMMIT, confirmed aborted/rollback result, exact retained replacement and physically returned owners may 6701/6704/6800 be described as the observed non-destructive race result. Preserve original native code; never map every6701 to Conflict or mutation=false after unknown COMMIT.',
'requiredNextCases':['Preopened ordinary KEY_SET_VALUE handle in a real waiting process before target intent; exact original HANDLE/creation/deadline retained.','Writer release immediately before value-intent write, after intent but before external read, after external compare before delete, after staged delete before commit, and after closing transacted HKEY before commit.','Distinct REG_SZ and distinct REG_BINARY writers; initial incompatible type preservation remains.','Same-value write and real A->B->A sequence; raw write statuses and actual writer exit distinguish observed mutations from merely equal final bytes.','Cancel before/after stage and original deadline; if prior stage was not reached it is NOT_EXERCISED.','A real rollback-failure/unknown-path fixture can use a restricted duplicate transaction HANDLE lacking rollback rights while retaining original privileged owner for negative cleanup. Record access-denied raw failure and classify operation Unknown; later coordinator rollback must not retroactively turn that Unknown into positive success.']},
'limitations':['No claim observed error was6800; actual6701 preserved.','No new fixture/code/build was performed by this read-only report.','No retest or whole72-to-PASS change.','Source-runner instrumentation e5b8... is parent-owned; this audit binds actual native EXE42082... and original wire rather than inventing the runner source hash.']}
f=P/'native-independent-first.json'
with f.open('x') as out:json.dump(r,out,ensure_ascii=False,indent=2);out.write('\n')
print(json.dumps({'report':info(f),'deleteCode':6701,'rollback':0,'txOutcome':3,'finalReplacementBytes':44,'nativeOriginalExit':72,'jobZero':10},ensure_ascii=False))
