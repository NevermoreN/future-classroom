#!/usr/bin/env python3
"""Local negative contract checks; never evidence of Windows/teacher business execution."""
import argparse, ast, copy, hashlib, importlib.util, json, subprocess, sys
from pathlib import Path
sys.dont_write_bytecode=True
S=Path(__file__).resolve().parent

def load(name):
    spec=importlib.util.spec_from_file_location('g03_'+name.replace('-','_'),S/(name+'.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m

def main():
    p=argparse.ArgumentParser();p.add_argument('--support-root',type=Path,required=True);a=p.parse_args()
    checks=[]
    def ok(name,fn): fn();checks.append(name)
    def reject(fn):
        try: fn()
        except (ValueError,RuntimeError): return
        raise AssertionError('EXPECTED_REJECTION')
    v=load('verify-teacher-browser-report')
    pair={'commandId':'1','commandTicket':'c2:'+'1'*32+':1'}
    ok('canonical_complete_pair',lambda:v.pair(pair))
    for bad in ({**pair,'commandId':'01'},{**pair,'commandTicket':'c2:'+'0'*32+':1'},{**pair,'commandTicket':'c2:'+'1'*32+':9223372036854775808'}):
        ok('reject_noncanonical_pair_'+str(len(checks)),lambda b=bad:reject(lambda:v.pair(b)))
    ok('cannot_report_unrun_as_business_pass',lambda:reject(lambda:v.validate({'schemaVersion':1,'status':'NOT_RUN'},{},{})))
    # Deliberately synthetic report only: walk every positive validator branch,
    # then mutate independent dangerous boundaries. It never stands for CDP/native execution.
    old={'schemaVersion':1,'apiVersion':1,'uiCompatibilityVersion':1,'assets':[{'path':'index.html','sizeBytes':1,'sha256':'a'*64},{'path':'assets/main.js','sizeBytes':1,'sha256':'a'*64}]}
    new=copy.deepcopy(old)
    for asset in new['assets']:asset['sha256']='b'*64
    def ref(n):return {'commandId':str(n),'commandTicket':'c2:'+'1'*32+':'+str(n)}
    ledger=[]
    for n,ordinal,family,path in [(1,7,'content',None),(2,10,'content','/fc/api/v1/content/reminders/1/2026-11-02'),(3,12,'materials','/fc/api/v1/materials/folders/2/1/append'),(4,14,'materials','/fc/api/v1/materials/folders/2/1/delete'),(5,16,'voice','/fc/api/v1/voice/broadcasts')]:
        ledger.append({'ordinal':ordinal,'path':'/fc/api/v1/commands','method':'POST','status':201,'family':family,**ref(n)})
        if path:ledger.append({'ordinal':ordinal+1,'path':path,'method':'POST','status':200,'input':ref(n),**ref(n)})
    ledger.append({'ordinal':9,'path':'/fc/api/v1/content/commands/'+ref(1)['commandTicket']+'/1','method':'GET','status':200,**ref(1)})
    report={'schemaVersion':1,'status':'PASS_TEACHER_DOM_FINITE_WIN11_ONLY','marker':'FutureClassroom-Test-TLS-'+'a'*32,'scope':'SYNTHETIC_ORDINARY_TEACHER_SAME_HTTPS_DOM','checks':dict.fromkeys(v.CHECKS,True),'ledger':ledger,'assets':[dict(bundle=b,**asset) for b,m in [('old',old),('new',new)] for asset in m['assets']],'nativeActions':v.ACTIONS,'midnightObservation':{'oldDate':'2026-11-02','newDate':'2026-11-03','newContentEmpty':True,'oldDraftRejected':True},'download':{'guid':'a'*8+'-'+'a'*4+'-'+'a'*4+'-'+'a'*4+'-'+'a'*12,'completed':True,'bytes':2},'pairingCycles':[{'cycle':i,'requestOrdinal':(i-1)*3+1,'requestId':str(i),'authorizedPollOrdinal':(i-1)*3+2,'sessionOrdinal':i*3,'principalId':'2','role':'TEACHER_ADMIN','assignments':[{'classId':'1','subjectId':'1'}]} for i in (1,2)],'droppedResponse':{**ref(1),'path':'/fc/api/v1/content/homework/1/1/2/2026-11-02','status':200,'contentSha256':'a'*64,'key':{'kind':'HOMEWORK','classId':'1','subjectId':'1','ownerTeacherId':'2','date':'2026-11-02'}},'observedRequestCount':20,'elapsedMillis':1,'maxMillis':120000,'contextDisposed':True,'cdpDisconnected':True,'physicalBrowserCleanup':'SEPARATE_ORIGINAL_EDGE_JOB_REQUIRED','voiceScope':'SYNTHETIC_ZERO_WAV_CAPTURE_ONLY_NO_REAL_MIC_OR_AUDIBILITY','windows7':'NOT_RUN','windows10':'NOT_RUN','phase':'complete'}
    assert v.validate(report,old,new)['status']=='PASS_G03_DOM_REPORT_ONLY'
    checks.append('synthetic_report_full_validator_path_only_not_business_execution')
    for mutate in (lambda r:r.update(cookie='secret'),lambda r:r['checks'].pop('isolatedMidnightRejectsOldDraft'),lambda r:r.update(contextDisposed=False),lambda r:r.update(elapsedMillis=120001),lambda r:r['pairingCycles'][1].update(role='GLOBAL_ADMIN'),lambda r:r['ledger'][0].update(family='voice'),lambda r:r['assets'][0].update(sha256='c'*64)):
        bad=copy.deepcopy(report);mutate(bad);reject(lambda:v.validate(bad,old,new))
    reject(lambda:v.validate(report,old,old));checks.append('synthetic_report_negative_scope_pair_family_cleanup_deadline_and_real_bundle')
    # Validator rejects superficial four-click PASS before reading any ledger.
    ok('cannot_report_four_ui_clicks_as_complete',lambda:reject(lambda:v.validate({'schemaVersion':1,'status':'PASS_TEACHER_DOM_FINITE_WIN11_ONLY','marker':'FutureClassroom-Test-TLS-'+'a'*32,'checks':dict.fromkeys(['homeworkPersistentCurrent','reminderPersistentBottomShortcut','materialsAppendDownloadDeleteReadback','voiceSyntheticCaptureSubmitCurrentAndPhysicalReturn'],True)},{},{})))
    for path in S.glob('*.py'): ast.parse(path.read_text())
    checks.append('python_all_import_syntax')
    node_script=r'''
import assert from 'node:assert/strict';
const {validateInit,pair}=await import(process.argv[1]);
const asset=(path,n)=>({path,sizeBytes:1,sha256:n.repeat(64)});
const manifest=n=>({schemaVersion:1,apiVersion:1,uiCompatibilityVersion:1,assets:[asset('index.html',n),asset('assets/main.js',n)]});
const init={type:'INIT',marker:'FutureClassroom-Test-TLS-'+'a'.repeat(32),cdpUrl:'ws://127.0.0.1:1234/devtools/browser/'+'a'.repeat(32),origin:'https://127.0.0.1:4321',maxMillis:120000,pairingCode:'12345678',loginName:'synthetic-ordinary-teacher',materialPath:'C:\\owned\\FutureClassroom-Test-TLS-'+ 'a'.repeat(32)+'\\g03-teacher-material.txt',materialName:'g03-teacher-material.txt',materialBytes:2,materialSha256:'a'.repeat(64),browserOwnerMarker:'FutureClassroom-Test-Browser-'+'b'.repeat(32),downloadPath:'C:\\owned\\FutureClassroom-Test-Browser-'+'b'.repeat(32)+'\\downloads',oldManifest:manifest('a'),newManifest:manifest('b'),expectedCsp:"script-src 'self'; connect-src 'self'"};
assert.equal(validateInit(init),init);
for(const change of [{cookie:'injected'},{pairingCode:'1234567'},{maxMillis:120001},{loginName:'global-admin'},{newManifest:manifest('a')},{cdpUrl:'ws://remote.example:1234/devtools/browser/'+'a'.repeat(32)},{materialPath:'C:\\owned\\other.txt'},{expectedCsp:"script-src 'self' 'unsafe-eval'; connect-src 'self'"}])assert.throws(()=>validateInit({...init,...change}));
assert.deepEqual(pair({commandId:'1',commandTicket:'c2:'+'a'.repeat(32)+':1'}),{commandId:'1',commandTicket:'c2:'+'a'.repeat(32)+':1'});
console.log(JSON.stringify({status:'PASS_LOCAL_NEGATIVE_INPUT_CONTRACTS',cases:10,windows:'NOT_RUN'}));
'''
    result=subprocess.run(['node','--input-type=module','-e',node_script,(S/'windows-teacher-browser-harness.mjs').as_uri()],capture_output=True,timeout=20)
    if result.returncode: raise AssertionError('NODE_PURE_FAILED '+result.stderr.decode())
    checks.append('node_actual_import_and_closed_input_negatives_10')
    # The adapter must construct without starting a worker, network, tunnel or Node.
    observer=load('windows-teacher-browser-observer')
    class Remote: host='future-classroom-win11'
    obj=observer.make_observer(a.support_root,Remote(),S,S/'NOT_CREATED.json','FutureClassroom-Test-TLS-'+'a'*32,{}, {},"script-src 'self'; connect-src 'self'",None,'future-classroom-win11')
    assert obj.node is None and obj.owner is None and obj.tunnel is None and not obj.started
    checks.append('observer_construction_is_inert_actual_base_interface')
    marker='FutureClassroom-Test-TLS-'+'a'*32
    code=observer.decode_pairing(('G03_PAIR_V1\n'+marker+'\n1\n12345678\nsynthetic-ordinary-teacher\n').encode(),marker,1)
    assert code=='12345678' and observer.action_request(marker,0,'CACHE_UPGRADE').endswith(b'CACHE_UPGRADE\n')
    reject(lambda:observer.decode_pairing(('G03_PAIR_V1\n'+marker+'\n2\n12345678\nsynthetic-ordinary-teacher\n').encode(),marker,1))
    reject(lambda:observer.action_request(marker,0,'REVOKE'))
    reject(lambda:observer.decode_ack(json.dumps({'type':'ACK','action':'CACHE_UPGRADE','marker':marker,'status':'PASS','oldOwnerDrained':True,'sameOrigin':'https://127.0.0.1:4321','pairingCode':'12345678','cookie':'injected'}).encode(),marker,0))
    checks.append('native_original_cycle_order_and_no_extra_ack_data')
    edge=load('windows-teacher-edge-owner')
    assert edge.CSHARP==load_original(a.support_root/'windows-test-edge-owner.py').CSHARP
    assert '180000' in edge.SUPERVISE and 'StopAndWait(20000)' in edge.SUPERVISE and 'time.monotonic() + 215' in (S/'windows-teacher-edge-owner.py').read_text()
    assert 'synthetic-zero.wav' in edge.RAW_NAMES and '--use-file-for-fake-audio-capture='+"'+$wav" in edge.SUPERVISE
    checks.append('exact_original_job_csharp_and_original_180_20_215_bounds')
    for name in ('windows-teacher-edge-owner.py','windows-teacher-browser-observer.py','verify-teacher-browser-report.py'):
        r=subprocess.run([sys.executable,'-B',str(S/name)],capture_output=True,timeout=5)
        assert r.returncode==0 and json.loads(r.stdout)['status']=='NOT_RUN'
    checks.append('all_python_default_entrypoints_inert')
    r=subprocess.run(['node',str(S/'windows-teacher-browser-harness.mjs')],capture_output=True,timeout=5);assert r.returncode==0 and json.loads(r.stdout)['status']=='NOT_RUN';checks.append('node_default_entrypoint_inert')
    print(json.dumps({'status':'PASS_LOCAL_CONTRACT_CHECKS_ONLY','checks':checks,'count':len(checks),'node':json.loads(result.stdout),'windows':'NOT_RUN','browserDom':'NOT_RUN','nativeRust':'NOT_COMPILED_BY_THIS_CHECK'},indent=2))

def load_original(path):
    spec=importlib.util.spec_from_file_location('g03_upstream_owner',path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
if __name__=='__main__':main()
