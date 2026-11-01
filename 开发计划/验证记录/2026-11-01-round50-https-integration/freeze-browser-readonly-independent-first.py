from pathlib import Path
import json,hashlib
E=Path('/tmp/fc-round50-https-integration-evidence');sha=lambda b:hashlib.sha256(b).hexdigest()
review=json.loads((E/'browser-readonly-independent-second.json').read_bytes());assert review['status']=='PASS_TWO_FINITE_ACTUAL_BROWSERS_AND_FRESH_READONLY'
rows=[]
for name in ['browser-readonly-independent-first.json','verify-final-browser-readonly-first.py','browser-readonly-independent-second.json','verify-final-browser-readonly-second.py','browser-readonly-parser-model-first.py','browser-readonly-parser-model-first.json','browser-readonly-parser-model-first.command.json','browser-readonly-parser-model-first.stdout.log','browser-readonly-parser-model-first.stderr.log']:
 p=E/name;data=p.read_bytes();rows.append({'path':str(p),'bytes':len(data),'sha256':sha(data)})
models=json.loads((E/'browser-readonly-parser-model-first.json').read_bytes());assert models['status']=='PASS_PURE_MODELS_ONLY' and models['positiveCount']==4 and models['negativeCount']==24
assert models['helper']['sha256']==sha((E/'verify-final-browser-readonly-second.py').read_bytes())
extra=[]
for flow in review['flows']:
 r=json.loads(Path(flow['browser']).read_bytes());last=r['nativeProgress'][-1];a=last['acceptance']
 assert a['posts']==a['completions'] and a['idle_cancellations']==a['idle_returns']
 assert a['ready_slots']==a['bound_slots']==a['native_pending']==a['owned_sockets']==0
 debug=r['nativeDebugLines'][-1];assert f"closed: {last['retired']}, retired: {last['retired']}," in debug
 extra.append({'browser':flow['browser'],'posts':a['posts'],'completions':a['completions'],'idleCancellations':a['idle_cancellations'],'idleReturns':a['idle_returns'],'readySlots':a['ready_slots'],'boundSlots':a['bound_slots'],'nativePending':a['native_pending'],'ownedSockets':a['owned_sockets'],'closedEqualsRetired':last['retired'],'nativePhysicalDrainReceipt':r['native']['physicalNativeDrain'],'scope':'Application native-drain receipt and exact IOCP accounting, no independent kernel trace or SSP forced-cancel assertion.'})
for x in review['inputs']:assert sha(Path(x['path']).read_bytes())==x['sha256']
result={'schemaVersion':1,'status':'FROZEN_PASS_LOCAL_INDEPENDENT_RECOMPUTATION','scope':'Only local evidence reads. No SSH, Windows, browser, Cargo, product/source changes or cleanup. All new files under R50; R46 directory was never written by this task.','files':rows,'inputCount':len(review['inputs']),'checkCount':len(review['checks']),'flows':review['flows'],'finalDebtChecks':extra,'models':{'positive':4,'negative':24,'scope':'Pure malformed-metadata copies only, not product execution.'},'originalFailure':review['history'],'limitations':review['limitations'],'matrix':'R50 all-workspace Host/all-target matrix is a separate root-owned consumer and not graded by this audit.'}
p=E/'browser-readonly-independent-final-freeze.json'
with p.open('x') as f:json.dump(result,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'status':result['status'],'report':str(p),'bytes':p.stat().st_size,'sha256':sha(p.read_bytes()),'inputs':result['inputCount'],'checks':result['checkCount'],'models':'4positive24negative','finalDebtChecks':extra},ensure_ascii=False))
