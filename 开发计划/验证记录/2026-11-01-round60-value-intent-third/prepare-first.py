from pathlib import Path
P=Path(__file__).parent;S=P.parent/'scratch-fixture-second/source-final/registry-write-intent.c';s=S.read_text()
s=s.replace('static DWORD event_sequence;', 'static ULONGLONG operation_deadline;\nstatic const BYTE BINARY_ORIGINAL[4]={0xde,0xad,0xbe,0xef};\nstatic DWORD event_sequence;')
i=s.index('static BOOL close_handle')
s=s[:i]+'''static DWORD remaining(DWORD maximum){
 ULONGLONG now=GetTickCount64();if(!operation_deadline||now>=operation_deadline)return 0;
 ULONGLONG n=operation_deadline-now;return n<maximum?(DWORD)n:maximum;
}
static BOOL forward_allowed(void){
 if(remaining(CASE_MS))return TRUE;
 status("FAIL_ORIGINAL_ABSOLUTE_DEADLINE_NO_FORWARD_OPERATION");return FALSE;
}
static BOOL equal_bytes(const BYTE *a,const BYTE *b,DWORD count){for(DWORD i=0;i<count;i++)if(a[i]!=b[i])return FALSE;return TRUE;}
static LONG query_raw(HKEY key,const WCHAR *name,BYTE *bytes,DWORD *size,DWORD *type,const char *tag){
 *size=256;*type=0;memset(bytes,0,256);
 LONG e=RegQueryValueExW(key,name,NULL,type,bytes,size);event(tag,e,*size);
 if(!e&&*size<=256)successful_value_bytes(bytes,*size,*type);
 if(!e&&*size>256)return ERROR_INVALID_DATA;
 return e;
}
static DWORD raw_kind(const BYTE *bytes,DWORD size,DWORD type){
 if(type==REG_SZ&&size==(wl(ORIGINAL)+1)*sizeof(WCHAR)&&equal_bytes(bytes,(const BYTE*)ORIGINAL,size))return 2;
 if(type==REG_SZ&&size==(wl(REPLACEMENT)+1)*sizeof(WCHAR)&&equal_bytes(bytes,(const BYTE*)REPLACEMENT,size))return 3;
 if(type==REG_BINARY&&size==sizeof(BINARY_ORIGINAL)&&equal_bytes(bytes,BINARY_ORIGINAL,size))return 4;
 return 0;
}
'''+s[i:]
a='''  if(!e)e=set_string(key,L"FutureClassroom",ORIGINAL,"prepare_set_original");''';assert a in s
s=s.replace(a,'''  if(!e&&c=='C'){e=RegSetValueExW(key,L"FutureClassroom",0,REG_BINARY,BINARY_ORIGINAL,sizeof(BINARY_ORIGINAL));event("prepare_set_binary_original",e,sizeof(BINARY_ORIGINAL));}
  else if(!e)e=set_string(key,L"FutureClassroom",c=='B'?REPLACEMENT:ORIGINAL,"prepare_set_original");''')
s=s.replace('command(cmd,mode,which);', 'command(cmd,mode,which);')
a=''' wc(out,L"fc-rwi ");wa(out,mode);wa(out,L" ");UINT n=wl(out);out[n++]=which;out[n]=0;wa(out,L" ");wa(out,nonce);return TRUE;''';assert a in s
s=s.replace(a,''' wc(out,L"fc-rwi ");wa(out,mode);wa(out,L" ");UINT n=wl(out);out[n++]=which;out[n]=0;wa(out,L" ");wa(out,nonce);wa(out,L" ");
 WCHAR digits[24];UINT count=0;ULONGLONG value=operation_deadline;
 do{digits[count++]=(WCHAR)('0'+value%10);value/=10;}while(value);
 n=wl(out);while(count)out[n++]=digits[--count];out[n]=0;return TRUE;''')
a=s.index('static DWORD writer(');b=s.index('\n/* Cleanup is a separate',a)
s=s[:a]+'''static DWORD writer(WCHAR which){
 HKEY root=NULL,key=NULL;LONG e;
 if(!forward_allowed())return 25;
 e=open_root(&root,KEY_READ);if(e)return 20;
 if(!marker(root)){close_key(&root,"writer_close_bad_marker");return 21;}
 e=case_open(root,which,&key,KEY_SET_VALUE);close_key(&root,"writer_close_root");
 if(!e&&forward_allowed()){status("ordinary_set_begin");e=set_string(key,L"FutureClassroom",REPLACEMENT,"ordinary_set_return");}else if(!e)e=ERROR_TIMEOUT;
 close_key(&key,"writer_close_case");
 if(!resource_ok)return 22;
 if(!forward_allowed())return 25;
 return e==ERROR_SUCCESS?0:23;
}
/* This is the comparison reader: a separate native process, no transaction
 * handle and only ordinary RegOpenKeyEx/RegQueryValueEx. It never reads the
 * staged expected value through the transaction that wrote it. */
static DWORD reader(WCHAR which){
 HKEY root=NULL,key=NULL;LONG e;BYTE bytes[256];DWORD size=0,type=0,kind=0;
 if(!forward_allowed())return 25;
 e=open_root(&root,KEY_READ);if(e)return 20;
 if(!marker(root)){close_key(&root,"reader_close_bad_marker");return 21;}
 e=case_open(root,which,&key,KEY_QUERY_VALUE);close_key(&root,"reader_close_root");
 if(!e&&forward_allowed()){status("ordinary_committed_read_begin");e=query_raw(key,L"FutureClassroom",bytes,&size,&type,"ordinary_committed_read_return");if(!e)kind=raw_kind(bytes,size,type);}else if(!e)e=ERROR_TIMEOUT;
 close_key(&key,"reader_close_case");event("ordinary_committed_compare_kind",e,kind);
 if(!resource_ok)return 22;
 if(!forward_allowed())return 25;
 if(e)return 23;
 return kind==2?0:24;
}
static BOOL rollback(HANDLE tx,const char *tag){BOOL ok=RollbackTransaction(tx);event(tag,ok?0:GetLastError(),0);return ok;}
static DWORD tx_outcome(HANDLE tx){
 DWORD outcome=0,level=0,flags=0,timeout=0;WCHAR description[256];
 BOOL ok=GetTransactionInformation(tx,&outcome,&level,&flags,&timeout,256,description);
 event("native_transaction_outcome",ok?0:GetLastError(),ok?outcome:0);return ok?outcome:0;
}
static BOOL abort_original(HANDLE tx,const char *tag){
 BOOL ok=rollback(tx,tag);DWORD outcome=tx_outcome(tx);
 return ok||outcome==TransactionOutcomeAborted;
}
static LONG final_value(HKEY root,WCHAR which,DWORD *kind){
 HKEY key=NULL;BYTE bytes[256];DWORD size=0,type=0;LONG e=case_open(root,which,&key,KEY_QUERY_VALUE);*kind=0;
 if(!e)e=query_raw(key,L"FutureClassroom",bytes,&size,&type,"final_query_target");
 if(e==ERROR_FILE_NOT_FOUND){*kind=1;e=0;}else if(!e){*kind=raw_kind(bytes,size,type);if(!*kind)e=ERROR_INVALID_DATA;}
 close_key(&key,"final_close_case");event("final_target_kind",e,*kind);return e;
}
static DWORD run_case(WCHAR which){
 HKEY root=NULL,parent=NULL;HANDLE tx=NULL;PROCESS_INFORMATION reader_pi,writer_pi;
 memset(&reader_pi,0,sizeof(reader_pi));memset(&writer_pi,0,sizeof(writer_pi));
 WCHAR keypath[260];LONG e=0;DWORD wait=0,read_exit=255,write_exit=255,kind=0;
 BOOL committed=FALSE,known=TRUE,finished=FALSE,reader_actual=FALSE,writer_actual=FALSE,writer_blocked=FALSE,mismatch=FALSE;
 const BOOL intent=which!='E',competition=which=='D'||which=='E';
 const BOOL cancelled=which=='F'||which=='H',timed_out=which=='G';
 if(!forward_allowed())return 30;
 if(open_root(&root,KEY_READ)||!marker(root))return 30;
 ULONGLONG transaction_deadline=GetTickCount64()+3000;
 if(transaction_deadline<operation_deadline)operation_deadline=transaction_deadline;
 event("transaction_original_deadline_low",0,(DWORD)operation_deadline);event("transaction_original_deadline_high",0,(DWORD)(operation_deadline>>32));
 if(!forward_allowed()){known=FALSE;goto done;}
 tx=CreateTransaction(NULL,NULL,0,0,0,remaining(3000),L"FutureClassroom scratch target-value intent");
 event("create_transaction",tx==INVALID_HANDLE_VALUE?GetLastError():0,0);if(tx==INVALID_HANDLE_VALUE){tx=NULL;known=FALSE;goto done;}
 wc(keypath,path);wa(keypath,L"\\\\");UINT n=wl(keypath);keypath[n++]=which;keypath[n]=0;
 if(!forward_allowed()){known=FALSE;goto done;}
 e=RegOpenKeyTransactedW(HKEY_CURRENT_USER,keypath,0,KEY_ALL_ACCESS|VIEW,&parent,tx,NULL);event("tx_open_parent",e,which);if(e){known=FALSE;goto done;}
 /* No target read, including a transacted target read, precedes this write.
  * The expected bytes are the independently supplied fixed synthetic value. */
 if(intent){
  if(!forward_allowed()){known=FALSE;goto done;}
  status("tx_stage_expected_WITHOUT_prior_target_read_begin");
  e=set_string(parent,L"FutureClassroom",ORIGINAL,"tx_stage_expected_return");if(e){known=FALSE;goto done;}
 }
 if(timed_out){
  while(remaining(3000))Sleep(5);
  status("ORIGINAL_TRANSACTION_DEADLINE_EXPIRED_FORWARD_REFUSED");goto done;
 }
 if(!forward_allowed()||!start_child(L"--reader",which,FALSE,&reader_pi)){known=FALSE;goto done;}
 close_handle(&reader_pi.hThread,"case_close_reader_thread");
 wait=WaitForSingleObject(reader_pi.hProcess,remaining(WRITER_OBSERVE_MS));event("reader_original_observation_wait",wait,WRITER_OBSERVE_MS);
 if(wait!=WAIT_OBJECT_0){
  known=FALSE;status("FAIL_FRESH_COMMITTED_READ_BLOCKED_UNDER_WRITE_INTENT_NO_FALLBACK");
  if(abort_original(tx,"blocked_reader_original_tx_rollback"))finished=TRUE;
  close_key(&parent,"blocked_reader_close_parent");close_handle(&tx,"blocked_reader_close_tx_handle");
  wait=WaitForSingleObject(reader_pi.hProcess,WRITER_RETURN_MS);event("reader_wait_AFTER_rollback_NOT_A_COMPARISON",wait,WRITER_RETURN_MS);
  goto done;
 }
 if(!GetExitCodeProcess(reader_pi.hProcess,&read_exit)){event("reader_exit_query",GetLastError(),0);known=FALSE;goto done;}
 reader_actual=TRUE;event("reader_actual_exit_BEFORE_decision",0,read_exit);
 if(!forward_allowed()){known=FALSE;goto done;}
 if(read_exit==24){mismatch=TRUE;status("COMMITTED_PREIMAGE_MISMATCH_ROLLBACK_REQUIRED");goto done;}
 if(read_exit!=0){known=FALSE;status("FAIL_FRESH_COMMITTED_READER_NATIVE_ERROR_NO_FALLBACK");goto done;}
 if(cancelled&&which=='F'){status("EXPLICIT_CANCEL_AFTER_INTENT_AND_EXTERNAL_COMPARE");goto done;}
 if(competition){
  if(!forward_allowed()||!start_child(L"--writer",which,FALSE,&writer_pi)){known=FALSE;goto done;}
  close_handle(&writer_pi.hThread,"case_close_writer_thread");
  wait=WaitForSingleObject(writer_pi.hProcess,remaining(WRITER_OBSERVE_MS));event("ordinary_writer_observation_wait",wait,WRITER_OBSERVE_MS);
  if(wait==WAIT_OBJECT_0){
   if(!GetExitCodeProcess(writer_pi.hProcess,&write_exit)){event("ordinary_exit_query",GetLastError(),0);known=FALSE;goto done;}
   writer_actual=TRUE;event("ordinary_actual_exit_BEFORE_target_delete",0,write_exit);
   if(write_exit!=0){status("ORDINARY_WRITER_REJECTED_KEEP_RAW_NATIVE_STATUS");}
  }else if(wait==WAIT_TIMEOUT){writer_blocked=TRUE;status("ORDINARY_WRITER_BLOCKED_OBSERVE_SERIALIZATION_WITH_ORIGINAL_TX");}
  else{known=FALSE;goto done;}
 }
 if(!forward_allowed()){known=FALSE;goto done;}
 status("tx_delete_target_begin");e=RegDeleteValueW(parent,L"FutureClassroom");event("tx_delete_target_return",e,0);if(e){known=FALSE;goto done;}
 if(cancelled){status("EXPLICIT_CANCEL_AFTER_STAGED_DELETE");goto done;}
 if(!close_key(&parent,"precommit_close_parent")||!forward_allowed()){known=FALSE;goto done;}
 status("tx_commit_begin");committed=CommitTransaction(tx);event("tx_commit_return",committed?0:GetLastError(),0);
 if(committed)finished=TRUE;else known=FALSE;
 tx_outcome(tx);
 done:
 if(tx&&!finished){if(abort_original(tx,"final_original_tx_rollback"))finished=TRUE;else known=FALSE;}
 close_key(&parent,"final_close_tx_parent");close_handle(&tx,"final_close_tx_handle");
 PROCESS_INFORMATION *all[2]={&reader_pi,&writer_pi};
 for(UINT i=0;i<2;i++)if(all[i]->hProcess){
  wait=WaitForSingleObject(all[i]->hProcess,WRITER_RETURN_MS);event(i?"writer_final_actual_wait":"reader_final_actual_wait",wait,WRITER_RETURN_MS);
  DWORD code=255;
  if(wait!=WAIT_OBJECT_0||!GetExitCodeProcess(all[i]->hProcess,&code)){known=FALSE;status("FAIL_ORIGINAL_READER_OR_WRITER_STILL_ACTIVE");}
  else{event(i?"writer_final_actual_exit":"reader_final_actual_exit",0,code);if(i){write_exit=code;writer_actual=TRUE;}else if(!reader_actual){read_exit=code;reader_actual=TRUE;}}
  close_handle(&all[i]->hThread,"final_close_child_thread");close_handle(&all[i]->hProcess,"final_close_child_process");
 }
 if(!tx&&!parent){if(final_value(root,which,&kind))known=FALSE;}else known=FALSE;
 HKEY check=NULL;WCHAR sentinel[128];
 if(case_open(root,which,&check,KEY_READ))known=FALSE;
 else{if(query_string(check,L"Sentinel",sentinel,128,"final_sentinel")||!weq(sentinel,SENTINEL))known=FALSE;close_key(&check,"final_close_verified_case");}
 close_key(&root,"final_close_root");
 if(!known||!resource_ok||!finished){status("FAIL_OR_INCONCLUSIVE_NATIVE_RESULT_NO_CAS_QUALIFICATION");return 31;}
 if((which=='B'||which=='C')&&mismatch&&!committed&&kind==(which=='B'?3u:4u)){status("INITIAL_NONMATCH_PRESERVED_BY_ROLLBACK");return 0;}
 if((which=='B'||which=='C')&&committed&&kind==1){status("CANDIDATE_FALSIFIED_EXTERNAL_READER_SAW_STAGED_EXPECTED_OVERWROTE_NONMATCH");return 0;}
 if((cancelled||timed_out)&&!committed&&kind==2){status("CANCEL_OR_ORIGINAL_TIMEOUT_PRESERVED_ORIGINAL");return 0;}
 if(which=='A'&&committed&&kind==1){status("NO_COMPETITOR_EXTERNAL_COMMITTED_READ_PROGRESS_OBSERVED");return 0;}
 if(competition&&writer_actual&&write_exit==0){
  if(committed&&kind==1){status(which=='E'?"BASELINE_NO_VALUE_INTENT_LOST_DISTINCT_REPLACEMENT":"CANDIDATE_FALSIFIED_VALUE_INTENT_LOST_DISTINCT_REPLACEMENT");return 0;}
  if(kind==3){status(writer_blocked?"WRITER_COMPLETED_AFTER_OWN_TRANSACTION_REPLACEMENT_RETAINED":"DISTINCT_REPLACEMENT_RETAINED_NATIVE_TX_OUTCOME_RECORDED");return 0;}
 }
 status("FAIL_OR_INCONCLUSIVE_UNEXPECTED_FINAL_STATE");return 31;
}
'''+s[b:]
# Permit only the one known binary synthetic initial value in cleanup.
s=s.replace('if(query_string(key,name,value,128,"cleanup_read_target")||(!weq(value,ORIGINAL)&&!weq(value,REPLACEMENT)))return FALSE;', 'BYTE bytes[256];DWORD size=0,type=0;if(query_raw(key,name,bytes,&size,&type,"cleanup_read_target")||!raw_kind(bytes,size,type))return FALSE;')
a=s.index('static BOOL safe_case(');b=s.index('\nstatic DWORD cleanup(',a)
s=s[:a]+'''static BOOL safe_case(HKEY root,WCHAR which){
 HKEY key=NULL;LONG e=case_open(root,which,&key,KEY_READ);if(e)return FALSE;
 BOOL safe=safe_values(key,FALSE);DWORD count=0;
 e=RegQueryInfoKeyW(key,NULL,NULL,NULL,&count,NULL,NULL,NULL,NULL,NULL,NULL,NULL);event("cleanup_case_has_no_children",e,count);
 if(e||count)safe=FALSE;
 close_key(&key,"cleanup_close_case");return safe;
}
'''+s[b:]
# Native child deadlines are inherited absolute ticks, not fresh per child timers.
s=s.replace(' job=CreateJobObjectW(NULL,NULL);', ' operation_deadline=GetTickCount64()+budget;\n event("operation_original_deadline_low",0,(DWORD)operation_deadline);event("operation_original_deadline_high",0,(DWORD)(operation_deadline>>32));\n job=CreateJobObjectW(NULL,NULL);')
s=s.replace('wait=WaitForSingleObject(pi.hProcess,budget);', 'wait=WaitForSingleObject(pi.hProcess,remaining(budget));')
a='''n=0;while(*p&&*p!=' '&&n<33)value[n++]=*p++;value[n]=0;while(*p==' ')p++;
 return !*p&&build_paths(value);''';assert a in s
s=s.replace(a,'''n=0;while(*p&&*p!=' '&&n<33)value[n++]=*p++;value[n]=0;while(*p==' ')p++;
 if(!build_paths(value)||!*p)return FALSE;
 operation_deadline=0;UINT digits=0;
 while(*p>='0'&&*p<='9'){
  UINT digit=(UINT)(*p++-'0');if(++digits>20||operation_deadline>(UINT64_MAX-digit)/10)return FALSE;
  operation_deadline=operation_deadline*10+digit;
 }
 while(*p==' ')p++;
 return !*p&&operation_deadline!=0;''')
s=s.replace(' else if(weq(mode,L"--writer")', ' else if(weq(mode,L"--reader")&&which>=\'A\'&&which<=\'H\')result=reader(which);\n else if(weq(mode,L"--writer")')
s=s.replace(' if(!mode[0]){', ' if(mode[0]){event("inherited_deadline_low",0,(DWORD)operation_deadline);event("inherited_deadline_high",0,(DWORD)(operation_deadline>>32));if(!forward_allowed())ExitProcess(66);}\n if(!mode[0]){')
with (P/'registry-value-intent.c').open('x') as f:f.write(s)
print(len(s.encode()))
