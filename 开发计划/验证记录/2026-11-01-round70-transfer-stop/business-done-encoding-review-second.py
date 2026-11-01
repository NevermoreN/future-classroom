from pathlib import Path
import datetime,json,hashlib,re
r=Path('/tmp/fc-round70-ordinary-transfer-stop');e=r/'evidence';source=r/'worktree/scripts/windows-ordinary-https-probe.py'
source_before=hashlib.sha256(source.read_bytes()).hexdigest();assert source_before=='f831be629435f7caeca6126b6bf7642dd3906d5cf839ad53435eee835c67db1f'
def require(value,msg):
 if not value:raise RuntimeError(msg)
namespace={'require':require,'re':re}
exec((e/'business-done-value-proposed-second.txt').read_text(),namespace)
encode=namespace['business_done_value'];marker='FutureClassroom-Test-TLS-'+'a'*32
positive=0
for ids in [['1','2'],[str(2**64-2),str(2**64-1)]]:
 for cycle in [1,2]:
  text=encode(marker,17,cycle,ids);fields=text.split('\n')
  assert len(fields)==7 and fields[:4]==['FC_ORDINARY_DONE_V2',marker,'00000000000000000017',str(cycle)] and fields[6]==''
  assert [int(value) for value in fields[4:6]]==[int(value) for value in ids]
  assert all(len(value)==20 and value==f'{int(value):020d}' for value in fields[4:6]);positive+=1
negative=0
bad_ids=[['0','2'],['01','2'],[True,'2'],['1',2],['1','1'],['1',str(2**64)],['-1','2'],[' 1','2'],['1\n','2'],['1'],['1','2','3'],'12']
for ids in bad_ids:
 try:encode(marker,17,1,ids)
 except RuntimeError:negative+=1
 else:raise AssertionError('bad IDs accepted')
for args in [(marker,True,1,['1','2']),(marker,0,1,['1','2']),(marker,2**64,1,['1','2']),(marker,17,True,['1','2']),(marker,17,3,['1','2']),(marker+'\n',17,1,['1','2'])]:
 try:encode(*args)
 except RuntimeError:negative+=1
 else:raise AssertionError('bad marker/generation/cycle accepted')
assert hashlib.sha256(source.read_bytes()).hexdigest()==source_before
v={'status':'CONFIRMED_P1_PROPOSED_FIX_PURE_CHECK_PASS_SOURCE_UNCHANGED','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceSha256Unchanged':source_before,'actualBadFormat':{'string1':f'{"1":020}','string2':f'{"2":020}','integer1':f'{1:020d}'},'proposedFunction':{'path':'business-done-value-proposed-second.txt','sha256':hashlib.sha256((e/'business-done-value-proposed-second.txt').read_bytes()).hexdigest()},'positiveNativeFormatRoundtrips':positive,'negativeCasesRejected':negative,'worktreeEdited':False,'cargoInvoked':False,'windowsInvoked':False,'awaitingRootTerminalBuildAuthorization':True}
with (e/'business-done-encoding-review-second.json').open('x') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'status':v['status'],'positive':positive,'negative':negative,'reportSha256':hashlib.sha256((e/'business-done-encoding-review-second.json').read_bytes()).hexdigest()}))
