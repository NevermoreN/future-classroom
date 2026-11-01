from pathlib import Path
P=Path(__file__).parent;old=(P.parent/'scratch-fixture/registry-write-intent.c').read_text();s=old
s=s.replace('static DWORD event_sequence;', 'static DWORD event_sequence;\nstatic BYTE owner_sid[SECURITY_MAX_SID_SIZE];\nstatic DWORD owner_sid_bytes;')
idx=s.index('static BOOL close_handle')
s=s[:idx]+'''static void process_identity(HANDLE process,DWORD pid,const char *tag){
 FILETIME created,exit,kernel,user;
 BOOL ok=GetProcessTimes(process,&created,&exit,&kernel,&user);
 event(tag,ok?0:GetLastError(),pid);
 if(ok){event("original_creation_low",0,created.dwLowDateTime);event("original_creation_high",0,created.dwHighDateTime);}
 else resource_ok=FALSE;
}
static void successful_value_bytes(const BYTE *bytes,DWORD length,DWORD type){
 char b[1024],*p=b;DWORD written=0;const char hex[]="0123456789abcdef";
 p=ascii(p,"RWI_VALUE pid=");p=number(p,GetCurrentProcessId());p=ascii(p," seq=");p=number(p,++event_sequence);
 p=ascii(p," tick_ms=");p=number(p,GetTickCount64());p=ascii(p," type=");p=number(p,type);p=ascii(p," bytes=");p=number(p,length);p=ascii(p," hex=");
 if(length>256){resource_ok=FALSE;return;}
 for(DWORD i=0;i<length;i++){*p++=hex[bytes[i]>>4];*p++=hex[bytes[i]&15];}*p++='\\n';
 if(!WriteFile(GetStdHandle(STD_OUTPUT_HANDLE),b,(DWORD)(p-b),&written,NULL)||written!=(DWORD)(p-b))resource_ok=FALSE;
}
static BOOL load_owner_sid(void){
 HANDLE token=NULL;union {TOKEN_USER user;BYTE buffer[256];} info;DWORD bytes=0;
 if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token)){event("owner_open_token",GetLastError(),0);return FALSE;}
 BOOL ok=GetTokenInformation(token,TokenUser,&info,sizeof(info),&bytes);
 if(!ok){event("owner_token_info",GetLastError(),bytes);CloseHandle(token);return FALSE;}
 if(!IsValidSid(info.user.User.Sid)){CloseHandle(token);return FALSE;}
 owner_sid_bytes=GetLengthSid(info.user.User.Sid);
 if(owner_sid_bytes>sizeof(owner_sid)||!CopySid(sizeof(owner_sid),owner_sid,info.user.User.Sid)){CloseHandle(token);return FALSE;}
 BOOL closed=CloseHandle(token);event("owner_close_token",closed?0:GetLastError(),owner_sid_bytes);return closed;
}
static BOOL same_owner_sid(HKEY key){
 BYTE bytes[SECURITY_MAX_SID_SIZE];DWORD size=sizeof(bytes),type=0;
 LONG e=RegQueryValueExW(key,L"OwnerSid",NULL,&type,bytes,&size);event("query_owner_sid",e,size);
 if(e||type!=REG_BINARY||size!=owner_sid_bytes)return FALSE;
 for(DWORD i=0;i<size;i++)if(bytes[i]!=owner_sid[i])return FALSE;
 return TRUE;
}
'''+s[idx:]
s=s.replace('return ERROR_SUCCESS;\n}\nstatic BOOL marker', 'successful_value_bytes((const BYTE*)value,bytes,type);return ERROR_SUCCESS;\n}\nstatic BOOL marker')
s=s.replace('return e==0&&weq(value,nonce);}', 'return e==0&&weq(value,nonce)&&same_owner_sid(root);}')
s=s.replace(' e=set_string(root,L"OwnerNonce",nonce,"prepare_set_marker");\n', ' e=set_string(root,L"OwnerNonce",nonce,"prepare_set_marker");\n if(!e){e=RegSetValueExW(root,L"OwnerSid",0,REG_BINARY,owner_sid,owner_sid_bytes);event("prepare_set_owner_sid",e,owner_sid_bytes);}\n')
s=s.replace("c<='C'", "c<='H'")
s=s.replace('event("create_child",ok?0:GetLastError(),ok?pi->dwProcessId:0);return ok;', 'event("create_child",ok?0:GetLastError(),ok?pi->dwProcessId:0);if(ok)process_identity(pi->hProcess,pi->dwProcessId,"original_child_process_identity");return ok;')
s=s.replace('e=set_string(key,L"FutureClassroom",REPLACEMENT,"ordinary_set_return");', 'e=set_string(key,L"FutureClassroom",(which==\'F\'||which==\'G\')?REPLACEMENT:ORIGINAL,"ordinary_set_return");')
a=s.index('static DWORD run_case(');b=s.index('\n/* Cleanup is a separate',a)
s=s[:a]+'''static LONG create_intent(HKEY parent,HANDLE tx,const WCHAR *intent,HKEY *child,BOOL before_read){
 DWORD disposition=0;
 status(before_read?"tx_create_intent_BEFORE_read_begin":"tx_create_intent_AFTER_read_begin");
 LONG e=RegCreateKeyTransactedW(parent,intent,0,NULL,0,KEY_ALL_ACCESS|VIEW,NULL,child,&disposition,tx,NULL);
 event("tx_create_intent_return",e,disposition);
 if(e)return e;
 if(disposition!=REG_CREATED_NEW_KEY)return ERROR_ALREADY_EXISTS;
 return close_key(child,"tx_close_original_new_child")?ERROR_SUCCESS:ERROR_INVALID_HANDLE;
}
static DWORD run_case(WCHAR which){
 HKEY root=NULL,parent=NULL,child=NULL;HANDLE tx=NULL;PROCESS_INFORMATION writer_pi;memset(&writer_pi,0,sizeof(writer_pi));
 WCHAR keypath[260],intent[64],preimage[128];LONG e=0;DWORD wait=0,writer_exit=255,kind=0;BOOL committed=FALSE,known=TRUE,tx_finished=FALSE,writer_actual=FALSE;
 const BOOL has_intent=which!='A'&&which!='G';
 const BOOL before_read=which=='D'||which=='E'||which=='F'||which=='H';
 const BOOL delete_early=which=='C'||which=='E';
 const BOOL has_writer=which!='H';
 if(open_root(&root,KEY_READ)||!marker(root))return 30;
 tx=CreateTransaction(NULL,NULL,0,0,0,3000,L"FutureClassroom scratch write intent only");
 event("create_transaction",tx==INVALID_HANDLE_VALUE?GetLastError():0,0);if(tx==INVALID_HANDLE_VALUE){tx=NULL;known=FALSE;goto done;}
 wc(keypath,path);wa(keypath,L"\\\\");UINT n=wl(keypath);keypath[n++]=which;keypath[n]=0;
 e=RegOpenKeyTransactedW(HKEY_CURRENT_USER,keypath,0,KEY_ALL_ACCESS|VIEW,&parent,tx,NULL);event("tx_open_parent",e,which);if(e){known=FALSE;goto done;}
 wc(intent,L"intent_");wa(intent,nonce);
 if(has_intent&&before_read){e=create_intent(parent,tx,intent,&child,TRUE);if(e){known=FALSE;goto done;}}
 e=query_string(parent,L"FutureClassroom",preimage,128,"tx_read_target");if(e||!weq(preimage,ORIGINAL)){known=FALSE;goto done;}
 if(has_intent&&!before_read){e=create_intent(parent,tx,intent,&child,FALSE);if(e){known=FALSE;goto done;}}
 if(delete_early){e=RegDeleteKeyTransactedW(parent,intent,VIEW,0,tx,NULL);event("tx_delete_intent_BEFORE_writer",e,0);if(e){known=FALSE;goto done;}}
 if(has_writer){
  if(!start_child(L"--writer",which,FALSE,&writer_pi)){known=FALSE;goto done;}
  close_handle(&writer_pi.hThread,"case_close_writer_thread");
  wait=WaitForSingleObject(writer_pi.hProcess,WRITER_OBSERVE_MS);event("ordinary_wait_initial",wait,WRITER_OBSERVE_MS);
  if(wait!=WAIT_OBJECT_0){
   status("INCONCLUSIVE_ORDINARY_NOT_RETURNED_WITHIN_ORIGINAL_OBSERVATION");known=FALSE;
   if(rollback(tx,"blocked_original_tx_rollback"))tx_finished=TRUE;
   close_key(&parent,"blocked_close_tx_parent");close_handle(&tx,"blocked_close_tx_handle");
   wait=WaitForSingleObject(writer_pi.hProcess,WRITER_RETURN_MS);event("ordinary_wait_after_rollback",wait,WRITER_RETURN_MS);
   if(wait==WAIT_OBJECT_0&&GetExitCodeProcess(writer_pi.hProcess,&writer_exit)){writer_actual=TRUE;event("ordinary_actual_exit_after_rollback",0,writer_exit);}
   goto done;
  }
  if(!GetExitCodeProcess(writer_pi.hProcess,&writer_exit)){event("ordinary_exit_query",GetLastError(),0);known=FALSE;goto done;}
  writer_actual=TRUE;event("ordinary_actual_exit_BEFORE_target_delete",0,writer_exit);
  if(writer_exit!=0){status("OBSERVED_ORDINARY_REJECTED_NOT_CAS_PROOF");known=FALSE;goto done;}
 }
 status("tx_delete_target_begin");e=RegDeleteValueW(parent,L"FutureClassroom");event("tx_delete_target_return",e,0);if(e){known=FALSE;goto done;}
 if(has_intent&&!delete_early){e=RegDeleteKeyTransactedW(parent,intent,VIEW,0,tx,NULL);event("tx_delete_intent_AFTER_target_stage",e,0);if(e){known=FALSE;goto done;}}
 /* End only after the actual original HKEYs have returned, preserving their
  * close LSTATUS; do not treat logical disposal as an implicit commit. */
 if(!close_key(&child,"precommit_close_child")||!close_key(&parent,"precommit_close_parent")){known=FALSE;goto done;}
 status("tx_commit_begin");committed=CommitTransaction(tx);event("tx_commit_return",committed?0:GetLastError(),0);
 if(committed)tx_finished=TRUE;else known=FALSE;
 done:
 if(tx&&!tx_finished){if(!rollback(tx,"final_tx_rollback"))known=FALSE;else tx_finished=TRUE;}
 close_key(&child,"final_close_tx_child");close_key(&parent,"final_close_tx_parent");close_handle(&tx,"final_close_tx_handle");
 if(writer_pi.hProcess){
  wait=WaitForSingleObject(writer_pi.hProcess,0);event("ordinary_final_exit_wait",wait,0);
  if(wait!=WAIT_OBJECT_0){known=FALSE;status("INCONCLUSIVE_ORDINARY_PROCESS_STILL_ACTIVE");}
  else if(!writer_actual&&GetExitCodeProcess(writer_pi.hProcess,&writer_exit)){writer_actual=TRUE;event("ordinary_actual_exit_final",0,writer_exit);}
  close_handle(&writer_pi.hThread,"final_close_writer_thread");close_handle(&writer_pi.hProcess,"final_close_writer_process");
 }
 if(!tx&&!parent&&!child&&tx_finished){if(final_value(root,which,&kind))known=FALSE;}else known=FALSE;
 /* Every successful readback also checks the unrelated sentinel and that the
  * temporary child has not silently persisted beyond transaction completion. */
 if(!tx&&!parent&&!child&&tx_finished){
  HKEY ordinary=NULL,unexpected=NULL;WCHAR sentinel[128];
  if(case_open(root,which,&ordinary,KEY_READ))known=FALSE;
  else{
   if(query_string(ordinary,L"Sentinel",sentinel,128,"final_sentinel")||!weq(sentinel,SENTINEL))known=FALSE;
   LONG ce=RegOpenKeyExW(ordinary,intent,0,KEY_READ|VIEW,&unexpected);event("final_intent_lookup",ce,0);
   if(ce!=ERROR_FILE_NOT_FOUND)known=FALSE;
   close_key(&unexpected,"final_close_unexpected_intent");close_key(&ordinary,"final_close_verified_case");
  }
 }
 close_key(&root,"final_close_root");
 if(known&&resource_ok&&committed&&kind==1){
  if(!has_writer){status("NO_COMPETITOR_PROGRESS_OBSERVED");return 0;}
  if(writer_actual&&writer_exit==0){
   if(which=='G'){status("BASELINE_DISTINCT_REPLACEMENT_LOST");return 0;}
   if(which=='F'){status("CANDIDATE_FALSIFIED_DISTINCT_REPLACEMENT_LOST");return 0;}
   status("SAME_VALUE_WRITE_NOT_CONFLICTED_DELETE_COMMITTED_NOT_BYTES_CAS_COUNTEREXAMPLE");return 0;
  }
 }
 status("INCONCLUSIVE_NOT_A_UNIVERSAL_CAS_PROOF");return 31;
}
'''+s[b:]
s=s.replace('if(!weq(name,L"OwnerNonce")||query_string(key,name,value,128,"cleanup_read_marker")||!weq(value,nonce))return FALSE;', 'if(weq(name,L"OwnerNonce")){if(query_string(key,name,value,128,"cleanup_read_marker")||!weq(value,nonce))return FALSE;}else if(weq(name,L"OwnerSid")){if(!same_owner_sid(key))return FALSE;}else return FALSE;')
s=s.replace("name[0]>'C'", "name[0]>'H'")
s=s.replace("which<='C'", "which<='H'")
s=s.replace('DWORD length=GetModuleFileNameW', 'if(!load_owner_sid())ExitProcess(65);\n DWORD length=GetModuleFileNameW')
s=s.replace('EXPERIMENT_COMPLETE_FINITE_FALSIFICATION_ONLY_NOT_CAS_PROOF','OBSERVATIONS_COMPLETE_FINITE_FALSIFICATION_ONLY_NOT_CAS_PROOF')
# Child admission is bounded to known cases; no free-form name/path arguments.
assert "which<='C'" not in s and "c<='C'" not in s
out=P/'registry-write-intent.c'
with out.open('x') as f:f.write(s)
print(len(s.encode()))
