#!/usr/bin/env python3
"""Finite G03 DOM report contract. Does not infer native/Edge physical release."""
import argparse, hashlib, json, re
from datetime import date, timedelta
from pathlib import Path
CHECKS=frozenset(('actualPairUiOrdinaryTeacher','materialsUnpublishedUploadCancelNoAppend','homeworkPersistentCurrent','reminderPersistentBottomShortcut','unknownOriginalPairReadbackNoReplay','offlineDraftRetainedNoReplay','materialsAppendDownloadDeleteReadback','voiceSyntheticCaptureSubmitCurrentAndPhysicalReturn','isolatedMidnightRejectsOldDraft','sameOriginDistinctBundleEntryAndLazyChunk','revokedTeacherClearsWorkspaceNoReplay','batchTwentyReviewRemoveAndConfirmedOrder','batchTwentySerialPreparedNoEarlyCommit','batchOneAtomicCommitAndOriginalHistory','batchCurrentDirectoryTwentyNewestLastConfirmed'))
ACTIONS=['CACHE_UPGRADE','VOICE_SETTLED','MIDNIGHT','REVOKE','COMPLETE']

def require(ok,code):
    if not ok: raise ValueError(code)
def pair(v):
    require(type(v) is dict and isinstance(v.get('commandId'),str) and re.fullmatch(r'[1-9][0-9]{0,19}',v['commandId']) and int(v['commandId'])<=2**64-1,'PAIR_RAW')
    m=re.fullmatch(r'c2:([0-9a-f]{32}):([1-9][0-9]{0,18})',v.get('commandTicket',''))
    require(m and m[1]!='0'*32 and int(m[2])<=2**63-1,'PAIR_TICKET')
    return v['commandId'],v['commandTicket']

def validate(report,old,new):
    require(type(report) is dict and set(report)=={'schemaVersion','status','marker','scope','checks','ledger','assets','nativeActions','pairingCycles','download','batchObservation','midnightObservation','droppedResponse','observedRequestCount','elapsedMillis','maxMillis','contextDisposed','cdpDisconnected','physicalBrowserCleanup','voiceScope','windows7','windows10','phase'},'REPORT_EXACT_KEYS_NO_PRIVATE_APPEND')
    require(type(report) is dict and report.get('schemaVersion')==1 and report.get('status')=='PASS_TEACHER_DOM_FINITE_WIN11_ONLY','NOT_FINITE_DOM_PASS')
    require(re.fullmatch(r'FutureClassroom-Test-TLS-[0-9a-f]{32}',report.get('marker','')),'MARKER')
    require(type(report.get('checks')) is dict and set(report['checks'])==CHECKS and all(v is True for v in report['checks'].values()),'INCOMPLETE_ACTUAL_CHECKS')
    require(report.get('scope')=='SYNTHETIC_ORDINARY_TEACHER_SAME_HTTPS_DOM' and report.get('nativeActions')==ACTIONS and report.get('contextDisposed') is True and report.get('cdpDisconnected') is True,'ACTUAL_DISPOSAL_OR_PHASES')
    require(report.get('physicalBrowserCleanup')=='SEPARATE_ORIGINAL_EDGE_JOB_REQUIRED' and report.get('voiceScope')=='SYNTHETIC_ZERO_WAV_CAPTURE_ONLY_NO_REAL_MIC_OR_AUDIBILITY' and report.get('windows7')=='NOT_RUN' and report.get('windows10')=='NOT_RUN','SCOPE')
    require(type(report.get('maxMillis')) is int and 0<report['maxMillis']<=120000 and type(report.get('elapsedMillis')) in (int,float) and 0<=report['elapsedMillis']<=report['maxMillis'],'ORIGINAL_DEADLINE')
    require(type(report.get('observedRequestCount')) is int and 0<report['observedRequestCount']<=128,'REQUEST_BOUND')
    midnight=report.get('midnightObservation');require(type(midnight) is dict and set(midnight)=={'oldDate','newDate','newContentEmpty','oldDraftRejected'} and midnight['newContentEmpty'] is True and midnight['oldDraftRejected'] is True and date.fromisoformat(midnight['oldDate'])+timedelta(days=1)==date.fromisoformat(midnight['newDate']),'ACTUAL_NEW_DAY_CURRENT_READ')
    dload=report.get('download');require(type(dload) is dict and set(dload)=={'guid','completed','bytes'} and re.fullmatch(r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}',dload['guid']) and dload['completed'] is True and type(dload['bytes']) is int and 0<dload['bytes']<=4096,'ACTUAL_OWNED_BROWSER_DOWNLOAD')
    cycles=report.get('pairingCycles');require(type(cycles) is list and len(cycles)==2,'TWO_ACTUAL_PAIR_UI_CYCLES')
    for index,c in enumerate(cycles,1):
        require(type(c) is dict and set(c)=={'cycle','requestOrdinal','requestId','authorizedPollOrdinal','sessionOrdinal','principalId','role','assignments'} and c['cycle']==index and c['role']=='TEACHER_ADMIN' and c['assignments']==[{'classId':'1','subjectId':'1'}],'ORDINARY_PAIR_ROLE_SCOPE')
        require(all(type(c[k]) is int and 0<c[k]<=report['observedRequestCount'] for k in ('requestOrdinal','authorizedPollOrdinal','sessionOrdinal')) and c['requestOrdinal']<c['authorizedPollOrdinal']<c['sessionOrdinal'],'ACTUAL_PAIR_ORDER')
        require(all(isinstance(c[k],str) and re.fullmatch(r'[1-9][0-9]{0,19}',c[k]) for k in ('requestId','principalId')),'PAIR_ID')
    require(cycles[0]['principalId']==cycles[1]['principalId'] and cycles[0]['sessionOrdinal']<cycles[1]['requestOrdinal'],'SAME_REAL_TEACHER_REPAIRING')
    ledger=report.get('ledger');require(type(ledger) is list and len(ledger)<=128,'LEDGER_BOUND')
    ordinals=set();issued={};writes=[]
    for row in ledger:
        require(type(row) is dict and set(row)<= {'ordinal','path','method','status','commandId','commandTicket','input','family'},'LEDGER_SHAPE')
        require(type(row.get('ordinal')) is int and 0<row['ordinal']<=report['observedRequestCount'] and row['ordinal'] not in ordinals,'LEDGER_ORDINAL');ordinals.add(row['ordinal'])
        require(row.get('method') in ('GET','POST') and isinstance(row.get('path'),str) and row['path'].startswith('/fc/api/v1/') and '?' not in row['path'] and '\n' not in row['path'],'SAFE_PATH')
        require(type(row.get('status')) is int and row['status'] in (200,201),'RESPONSE_STATUS')
        if 'commandTicket' in row:
            p=pair(row)
            if row['path']=='/fc/api/v1/commands':
                require(row['method']=='POST' and row['status']==201 and row.get('family') in ('content','materials','voice') and p not in issued,'ISSUANCE');issued[p]=(row['ordinal'],row['family'])
        if 'input' in row:
            p=pair(row['input']);require(p in issued and issued[p][0]<row['ordinal'],'SUBMIT_AFTER_REAL_ISSUE');require(issued[p][1]==row['path'].split('/')[4],'SAME_FAMILY_ISSUANCE');writes.append((p,row['path']))
    require({r['family'] for r in ledger if 'family' in r}=={'content','materials','voice'},'FOUR_BUSINESS_COMMAND_FAMILIES')
    require(len(writes)>=4 and len(writes)==len(set(p for p,_ in writes)),'NO_SUCCESSFUL_AUTO_REPLAY')
    d=report.get('droppedResponse');require(type(d) is dict and set(d)=={'commandId','commandTicket','path','status','contentSha256','key'} and d['status']==200 and re.fullmatch('[0-9a-f]{64}',d['contentSha256']),'UNKNOWN_COMMITTED_RESPONSE')
    p=pair(d);require(p in issued and issued[p][1]=='content' and len([r for r in ledger if r['path']=='/fc/api/v1/content/commands/'+p[1]+'/'+p[0]])==1,'MANUAL_ORIGINAL_HISTORY')
    require(old!=new and any(a['path'].endswith('.js') and not any(b['path']==a['path'] and b['sha256']==a['sha256'] for b in new['assets']) for a in old['assets']),'REAL_DIFFERENT_BUNDLE')
    assets=report.get('assets');require(type(assets) is list and len(assets)<=128,'ASSETS_BOUND')
    for a in assets:
        require(type(a) is dict and set(a)=={'bundle','path','sha256','sizeBytes'} and a['bundle'] in ('old','new'),'ASSET_SHAPE')
        source=old if a['bundle']=='old' else new
        require(any(x['path']==a['path'] and x['sizeBytes']==a['sizeBytes'] and x['sha256']==a['sha256'] for x in source['assets']),'REAL_ASSET_BYTES')
    for generation in ('old','new'):
        require(any(a['bundle']==generation and a['path']=='index.html' for a in assets) and any(a['bundle']==generation and a['path'].endswith('.js') for a in assets),'ACTUAL_BOTH_BUNDLES')
    batch=report.get('batchObservation')
    require(type(batch) is dict and set(batch)=={'batchId','commandId','commandTicket','confirmedNames','uploadIds','materialIds','sha256s','memberCount','removedBeforeReselect','uploadOrder','firstAddedSequence','lastAddedSequence','folderRevisionBefore','folderRevisionAfter','historyOrdinal','commitOrdinal','currentOrdinal'},'BATCH_OBSERVATION_CLOSED')
    expected=[f'g03-batch-{n:02d}.txt' for n in (2,3,1,*range(4,21))]
    require(batch['confirmedNames']==expected and batch['memberCount']==20 and batch['removedBeforeReselect']==1,'BATCH_ACTUAL_REVIEW_ORDER')
    require(batch['sha256s']==[hashlib.sha256(f'Synthetic future classroom batch member {int(n[10:12]):02d}.\n'.encode('ascii')).hexdigest() for n in expected],'BATCH_SYNTHETIC_HASHES')
    for key in ('uploadIds','materialIds'):
        require(type(batch[key]) is list and len(batch[key])==20 and len(set(batch[key]))==20 and all(type(v) is str and re.fullmatch(r'[1-9][0-9]{0,19}',v) and int(v)<=2**64-1 for v in batch[key]),'BATCH_UNIQUE_CANONICAL_IDS')
    require(type(batch['batchId']) is str and re.fullmatch(r'[1-9][0-9]{0,19}',batch['batchId']) and int(batch['batchId'])<=2**64-1,'BATCH_CANONICAL_GROUP')
    require(all(type(batch[k]) is str and re.fullmatch(r'[1-9][0-9]{0,19}',batch[k]) and int(batch[k])<=2**64-1 for k in ('folderRevisionBefore','folderRevisionAfter')) and int(batch['folderRevisionAfter'])==int(batch['folderRevisionBefore'])+1,'BATCH_ONE_FOLDER_OPERATION')
    require(all(type(batch[k]) is str and re.fullmatch(r'[1-9][0-9]{0,19}',batch[k]) and int(batch[k])<=2**64-1 for k in ('firstAddedSequence','lastAddedSequence')) and int(batch['lastAddedSequence'])==int(batch['firstAddedSequence'])+19,'BATCH_CONTINUOUS_ACTUAL_SEQUENCE_NOT_RESET')
    bp=pair(batch);require(bp in issued and issued[bp][1]=='materials','BATCH_ACTUAL_MATERIALS_ISSUANCE')
    require(all(type(batch[k]) is int and 0<batch[k]<=report['observedRequestCount'] for k in ('commitOrdinal','historyOrdinal','currentOrdinal')) and batch['commitOrdinal']<batch['historyOrdinal']<batch['currentOrdinal'],'BATCH_COMMIT_HISTORY_CURRENT_ORDER')
    batch_path='/fc/api/v1/materials/folders/'+cycles[1]['principalId']+'/1/append-batch'
    batch_writes=[r for r in ledger if r['path']==batch_path]
    history_path='/fc/api/v1/materials/batch-commands/'+cycles[1]['principalId']+'/1/'+bp[1]+'/'+bp[0]
    require(len(batch_writes)==1 and batch_writes[0]['ordinal']==batch['commitOrdinal'] and pair(batch_writes[0]['input'])==bp and len([r for r in ledger if r['path']==history_path and r['ordinal']==batch['historyOrdinal']])==1,'BATCH_ONE_WRITE_ORIGINAL_QUERY')
    upload_rows=[r for r in ledger if r['path'].startswith('/fc/api/v1/materials/batches/'+batch['batchId']+'/files/') and r['method']=='POST' and r['status']==200]
    require(len(upload_rows)==20,'BATCH_TWENTY_UPLOAD_RECEIPTS')
    upload_rows.sort(key=lambda r:r['ordinal'])
    order=batch['uploadOrder'];require(type(order) is list and len(order)==20,'BATCH_TWENTY_ORIGINAL_REPLY_OBSERVATIONS')
    for index,(row,observation) in enumerate(zip(upload_rows,order)):
        require(type(observation) is dict and set(observation)=={'requestOrdinal','startedAtEvent','originalReplyObservedAtEvent','finishedAtEvent'},'BATCH_UPLOAD_ORDER_CLOSED')
        require(type(observation['requestOrdinal']) is int and observation['requestOrdinal']==row['ordinal'] and row['path']=='/fc/api/v1/materials/batches/'+batch['batchId']+'/files/'+batch['uploadIds'][index]+'/bytes','BATCH_UPLOAD_ORDER_ORIGINAL_TWENTY_ORDINALS')
        require(all(type(observation[k]) is int and 0<observation[k]<=65536 for k in ('startedAtEvent','originalReplyObservedAtEvent','finishedAtEvent')) and observation['startedAtEvent']<observation['originalReplyObservedAtEvent']<observation['finishedAtEvent'],'BATCH_ORIGINAL_REPLY_THEN_FINISHED')
        # The original reply is observed while paused before unmodified release.
        # A later loadingFinished delivery may follow the next renderer request.
        if index:
            require(order[index-1]['requestOrdinal']<observation['requestOrdinal'] and order[index-1]['originalReplyObservedAtEvent']<observation['startedAtEvent'],'BATCH_SERIAL_ORIGINAL_REPLY_BEFORE_NEXT_START')
    return {'status':'PASS_G03_DOM_REPORT_ONLY','native':'SEPARATE_ORIGINAL_REPORT_REQUIRED','edge':'SEPARATE_ORIGINAL_JOB_REQUIRED'}

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--report',type=Path);p.add_argument('--old-manifest',type=Path);p.add_argument('--new-manifest',type=Path);p.add_argument('--execute',action='store_true');a=p.parse_args()
    if not a.execute: print(json.dumps({'status':'NOT_RUN','mode':'DEFAULT_INERT'}))
    else:
        require(all((a.report,a.old_manifest,a.new_manifest)),'EXPLICIT_INPUTS_REQUIRED')
        print(json.dumps(validate(*(json.loads(x.read_bytes()) for x in (a.report,a.old_manifest,a.new_manifest)))))
