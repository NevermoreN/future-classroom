from pathlib import Path
import json,hashlib,re
P=Path(__file__).parent;O=P/'windows-original-first';j=json.loads((O/'native-first.json').read_bytes());raw=(O/'native-first.stdout.log').read_bytes()
def info(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
assert info(O/'native-first.stdout.log')['sha256']==j['stdoutSha256'] and len(raw)==j['stdoutBytes']
assert (O/'native-first.stderr.log').read_bytes()==b'' and j['stderrBytes']==0
assert j['physicalExit'] is True and j['exitCode']==0 and j['expectedExeSha256']==info(P/'source-final/registry-write-intent.exe')['sha256']
events=[];values=[]
for index,line in enumerate(raw.decode('ascii').splitlines()):
 m=re.fullmatch(r'RWI_EVENT pid=(\d+) seq=(\d+) tick_ms=(\d+) name=([A-Za-z0-9_]+) code=(\d+) detail=(\d+)',line)
 if m:events.append(dict(zip(['pid','sequence','tickMillis','name','code','detail'],[int(m[1]),int(m[2]),int(m[3]),m[4],int(m[5]),int(m[6])]),line=index+1))
 elif line.startswith('RWI_VALUE '):values.append(line)
 else:assert line=='RWI_OWNED_NONCE '+j['registryNonce']
assert len(events)==len(j['events'])
root=j['pid'];start=[e for e in events if e['pid']==root and e['name']=='supervisor_case_begin'];assert [chr(e['detail']) for e in start]==list('ABCDEFGH')
summary=[]
for i,e in enumerate(start):
 end=start[i+1]['line'] if i+1<len(start) else len(raw.splitlines())+1
 r=[v for v in events if e['line']<=v['line']<end];case=chr(e['detail']);cp=next(v['detail'] for v in r if v['pid']==root and v['name']=='create_child')
 own=[v for v in r if v['pid']==cp];terminal=[v['name'] for v in own if v['name'].startswith(('SAME_VALUE_','BASELINE_DISTINCT_','CANDIDATE_FALSIFIED_','NO_COMPETITOR_','INCONCLUSIVE_'))]
 assert len(terminal)==1
 summary.append({'case':case,'originalCasePid':cp,'classification':terminal[0],'commit':[v for v in own if v['name']=='tx_commit_return'],'final':[v for v in own if v['name']=='final_target_kind']})
f=next(x for x in summary if x['case']=='F');pid=f['originalCasePid'];own=[e for e in events if e['pid']==pid]
def one(name):
 a=[e for e in own if e['name']==name];assert len(a)==1;return a[0]
writer=one('create_child')['detail'];we=[e for e in events if e['pid']==writer];write=next(e for e in we if e['name']=='ordinary_set_return');close=next(e for e in we if e['name']=='writer_close_case')
causal=[one('tx_create_intent_return'),one('tx_read_target'),write,close,one('ordinary_actual_exit_BEFORE_target_delete'),one('tx_delete_target_return'),one('tx_delete_intent_AFTER_target_stage'),one('tx_commit_return'),one('final_query_target'),one('final_target_kind')]
assert all(causal[i]['line']<causal[i+1]['line'] for i in range(len(causal)-1))
assert all(x['code']==0 for x in causal if x['name']!='final_query_target') and one('final_query_target')['code']==2 and one('final_target_kind')['detail']==1 and one('ordinary_actual_exit_BEFORE_target_delete')['detail']==0
zeros=[e for e in events if e['pid']==root and e['name']=='supervisor_actual_job_active_zero'];assert len(zeros)==10 and all(e['code']==0 for e in zeros)
cl=[e for e in events if e['name']=='CLEANUP_COMPLETE_EXACT_OWNED_LEAF_ABSENT'];assert len(cl)==1
assert any(e['name']=='cleanup_verify_absent' and e['code']==2 for e in events)
r={'status':'CONFIRMED_ACTUAL_CHILD_INTENT_COUNTEREXAMPLE_NOT_PRODUCTION_QUALIFIED','inputs':[info(O/'native-first.json'),info(O/'native-first.stdout.log'),info(O/'native-first.stderr.log'),info(P/'source-final/registry-write-intent.c'),info(P/'source-final/registry-write-intent.exe')],'actual':{'outerOriginalPid':root,'outerCreationFileTime':j['creationFileTime'],'originalHandleRetained':j['originalProcessHandleRetained'],'physicalExit':True,'exitCode':0,'elapsedMillis':j['elapsedMillis'],'nativeCases':summary,'actualJobZero':10,'cleanup':'Original native lookup returned FILE_NOT_FOUND for exact own leaf after all owned Job actual zeros; only own synthetic scope.','registryNonce':j['registryNonce']},'F':{'casePid':pid,'writerPid':writer,'exactCausalEvents':causal,'classification':f['classification'],'predicate':'Different replacement actually set/closed and its original process exit0 observed before target delete; own tx delete0+commit0 followed by absent. Temporary child namespace write supplied no parent-value conflict in this actual execution.'},'conclusion':'Child-intent candidate is falsified for this concrete Win11/WOW64 build; do not advance it into production. Same-value A..E outcomes do not substitute for this distinct-value counterexample. H proves only no-competitor progress.','scope':'Only eight actual synthetic cases and exact-own cleanup; not a full product test or universal proof for a new mechanism.'}
f=P/'native-independent-first.json'
with f.open('x') as out:json.dump(r,out,ensure_ascii=False,indent=2);out.write('\n')
print(json.dumps({'report':info(f),'cases':len(summary),'actualJobZero':len(zeros),'conclusion':r['status']}))
