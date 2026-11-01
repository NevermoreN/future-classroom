/* Scratch-only falsification fixture. No real Run path or production adapter.
 * No C runtime: imported APIs are confined to Kernel32/Advapi32/KtmW32.
 * Registry/TxR operations run only in supervised, killable owned processes.
 */
#define _WIN32_WINNT 0x0601
#define WINVER 0x0601
#define UNICODE
#define _UNICODE
#include <windows.h>
#include <ktmw32.h>
#include <stdint.h>

#define CASE_MS 6000u
#define PREPARE_MS 3000u
#define CLEANUP_MS 3000u
#define WRITER_OBSERVE_MS 1000u
#define WRITER_RETURN_MS 1500u
#define VIEW KEY_WOW64_32KEY
#define PREFIX L"FutureClassroom-RegistryWriteIntent-Test-"
#define ORIGINAL L"synthetic-original"
#define REPLACEMENT L"synthetic-replacement"
#define SENTINEL L"synthetic-sentinel"
static WCHAR nonce[33], leaf[128], path[256], executable[1024];
static ULONGLONG operation_deadline;
static const BYTE BINARY_ORIGINAL[4]={0xde,0xad,0xbe,0xef};
static DWORD event_sequence;
static BYTE owner_sid[SECURITY_MAX_SID_SIZE];
static DWORD owner_sid_bytes;
static BOOL resource_ok=TRUE;

/* Freestanding helpers prevent accidental msvcrt imports or external tools. */
void *memset(void *p,int v,size_t n){unsigned char *q=p;while(n--)*q++=(unsigned char)v;return p;}
void *memcpy(void *p,const void *s,size_t n){unsigned char *q=p;const unsigned char *r=s;while(n--)*q++=*r++;return p;}
static UINT wl(const WCHAR *s){UINT n=0;while(s[n])n++;return n;}
static BOOL weq(const WCHAR *a,const WCHAR *b){while(*a&&*a==*b){a++;b++;}return *a==*b;}
static void wc(WCHAR *d,const WCHAR *s){while((*d++=*s++)){} }
static void wa(WCHAR *d,const WCHAR *s){wc(d+wl(d),s);}
static char *ascii(char *d,const char *s){while(*s)*d++=*s++;return d;}
static char *number(char *d,ULONGLONG n){char b[24];UINT k=0;do{b[k++]=(char)('0'+n%10);n/=10;}while(n);while(k)*d++=b[--k];return d;}
static void event(const char *name,DWORD code,DWORD detail){
 char b[512],*p=b;DWORD written=0;
 p=ascii(p,"RWI_EVENT pid=");p=number(p,GetCurrentProcessId());p=ascii(p," seq=");p=number(p,++event_sequence);
 p=ascii(p," tick_ms=");p=number(p,GetTickCount64());p=ascii(p," name=");p=ascii(p,name);
 p=ascii(p," code=");p=number(p,code);p=ascii(p," detail=");p=number(p,detail);*p++='\n';
 if(!WriteFile(GetStdHandle(STD_OUTPUT_HANDLE),b,(DWORD)(p-b),&written,NULL)||written!=(DWORD)(p-b))resource_ok=FALSE;
}
static void status(const char *value){event(value,0,0);}
static void process_identity(HANDLE process,DWORD pid,const char *tag){
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
 for(DWORD i=0;i<length;i++){*p++=hex[bytes[i]>>4];*p++=hex[bytes[i]&15];}*p++='\n';
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
static DWORD remaining(DWORD maximum){
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
static BOOL close_handle(HANDLE *h,const char *tag){if(!*h||*h==INVALID_HANDLE_VALUE)return TRUE;BOOL ok=CloseHandle(*h);DWORD e=ok?0:GetLastError();event(tag,e,0);if(ok)*h=NULL;else resource_ok=FALSE;return ok;}
static BOOL close_key(HKEY *h,const char *tag){if(!*h)return TRUE;LONG e=RegCloseKey(*h);event(tag,e,0);if(e==ERROR_SUCCESS)*h=NULL;else resource_ok=FALSE;return e==ERROR_SUCCESS;}
static BOOL build_paths(const WCHAR *value){
 if(wl(value)!=32)return FALSE;
 for(UINT i=0;i<32;i++)if(!((value[i]>='0'&&value[i]<='9')||(value[i]>='a'&&value[i]<='f')))return FALSE;
 wc(nonce,value);wc(leaf,PREFIX);wa(leaf,nonce);wc(path,L"Software\\");wa(path,leaf);return TRUE;
}
static LONG open_root(HKEY *out,REGSAM access){LONG e=RegOpenKeyExW(HKEY_CURRENT_USER,path,0,access|VIEW,out);event("open_root",e,0);return e;}
static LONG set_string(HKEY key,const WCHAR *name,const WCHAR *value,const char *tag){LONG e=RegSetValueExW(key,name,0,REG_SZ,(const BYTE*)value,(wl(value)+1)*sizeof(WCHAR));event(tag,e,0);return e;}
/* Exact bounded REG_SZ; missing is distinct and never treated as replacement. */
static LONG query_string(HKEY key,const WCHAR *name,WCHAR *value,UINT capacity,const char *tag){
 DWORD type=0,bytes=capacity*sizeof(WCHAR);memset(value,0,bytes);LONG e=RegQueryValueExW(key,name,NULL,&type,(BYTE*)value,&bytes);
 event(tag,e,bytes);if(e!=ERROR_SUCCESS)return e;
 if(type!=REG_SZ||bytes<2||bytes>capacity*sizeof(WCHAR)||bytes%2||value[bytes/2-1]!=0||wl(value)!=bytes/2-1)return ERROR_INVALID_DATA;
 successful_value_bytes((const BYTE*)value,bytes,type);return ERROR_SUCCESS;
}
static BOOL marker(HKEY root){WCHAR value[64];LONG e=query_string(root,L"OwnerNonce",value,64,"query_marker");return e==0&&weq(value,nonce)&&same_owner_sid(root);}
static LONG case_open(HKEY root,WCHAR name,HKEY *key,REGSAM access){WCHAR sub[2]={name,0};LONG e=RegOpenKeyExW(root,sub,0,access|VIEW,key);event("open_case",e,name);return e;}

static DWORD prepare(void){
 HKEY software=NULL,root=NULL,key=NULL;DWORD disposition=0;LONG e;
 e=RegOpenKeyExW(HKEY_CURRENT_USER,L"Software",0,KEY_CREATE_SUB_KEY|VIEW,&software);event("prepare_open_existing_software",e,0);if(e)return 10;
 e=RegCreateKeyExW(software,leaf,0,NULL,REG_OPTION_NON_VOLATILE,KEY_ALL_ACCESS|VIEW,NULL,&root,&disposition);
 event("prepare_create_leaf",e,disposition);
 if(e||disposition!=REG_CREATED_NEW_KEY){close_key(&root,"prepare_close_unclaimed");close_key(&software,"prepare_close_software");return 11;}
 e=set_string(root,L"OwnerNonce",nonce,"prepare_set_marker");
 if(!e){e=RegSetValueExW(root,L"OwnerSid",0,REG_BINARY,owner_sid,owner_sid_bytes);event("prepare_set_owner_sid",e,owner_sid_bytes);}
 for(WCHAR c='A';!e&&c<='H';c++){
  WCHAR sub[2]={c,0};e=RegCreateKeyExW(root,sub,0,NULL,0,KEY_ALL_ACCESS|VIEW,NULL,&key,&disposition);event("prepare_create_case",e,disposition);
  if(!e&&disposition!=REG_CREATED_NEW_KEY)e=ERROR_ALREADY_EXISTS;
  if(!e&&c=='C'){e=RegSetValueExW(key,L"FutureClassroom",0,REG_BINARY,BINARY_ORIGINAL,sizeof(BINARY_ORIGINAL));event("prepare_set_binary_original",e,sizeof(BINARY_ORIGINAL));}
  else if(!e)e=set_string(key,L"FutureClassroom",c=='B'?REPLACEMENT:ORIGINAL,"prepare_set_original");
  if(!e)e=set_string(key,L"Sentinel",SENTINEL,"prepare_set_sentinel");
  if(!close_key(&key,"prepare_close_case"))e=ERROR_INVALID_HANDLE;
 }
 close_key(&root,"prepare_close_root");close_key(&software,"prepare_close_software");
 return e||!resource_ok?12:0;
}

static BOOL command(WCHAR *out,const WCHAR *mode,WCHAR which){
 wc(out,L"fc-rwi ");wa(out,mode);wa(out,L" ");UINT n=wl(out);out[n++]=which;out[n]=0;wa(out,L" ");wa(out,nonce);wa(out,L" ");
 WCHAR digits[24];UINT count=0;ULONGLONG value=operation_deadline;
 do{digits[count++]=(WCHAR)('0'+value%10);value/=10;}while(value);
 n=wl(out);while(count)out[n++]=digits[--count];out[n]=0;return TRUE;
}
static BOOL start_child(const WCHAR *mode,WCHAR which,BOOL suspended,PROCESS_INFORMATION *pi){
 STARTUPINFOW si;WCHAR cmd[256];memset(&si,0,sizeof(si));memset(pi,0,sizeof(*pi));si.cb=sizeof(si);
 si.dwFlags=STARTF_USESTDHANDLES;si.hStdInput=GetStdHandle(STD_INPUT_HANDLE);si.hStdOutput=GetStdHandle(STD_OUTPUT_HANDLE);si.hStdError=GetStdHandle(STD_ERROR_HANDLE);
 command(cmd,mode,which);
 BOOL ok=CreateProcessW(executable,cmd,NULL,NULL,TRUE,CREATE_NO_WINDOW|(suspended?CREATE_SUSPENDED:0),NULL,NULL,&si,pi);
 event("create_child",ok?0:GetLastError(),ok?pi->dwProcessId:0);if(ok)process_identity(pi->hProcess,pi->dwProcessId,"original_child_process_identity");return ok;
}
static DWORD writer(WCHAR which){
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
 DWORD tx_timeout=remaining(3000);if(!tx_timeout){known=FALSE;goto done;}
 tx=CreateTransaction(NULL,NULL,0,0,0,tx_timeout,L"FutureClassroom scratch target-value intent");
 event("create_transaction",tx==INVALID_HANDLE_VALUE?GetLastError():0,0);if(tx==INVALID_HANDLE_VALUE){tx=NULL;known=FALSE;goto done;}
 wc(keypath,path);wa(keypath,L"\\");UINT n=wl(keypath);keypath[n++]=which;keypath[n]=0;
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

/* Cleanup is a separate bounded process and runs only after the supervisor has
 * observed every case job ActiveProcesses==0. It accepts a CLOSED synthetic
 * namespace, never arbitrary content or a marker alone. */
static BOOL safe_values(HKEY key,BOOL is_root){
 DWORD index=0;WCHAR name[128],value[128];LONG e;
 for(;;){DWORD count=128;memset(name,0,sizeof(name));e=RegEnumValueW(key,index,name,&count,NULL,NULL,NULL,NULL);event("cleanup_enum_value",e,index);
  if(e==ERROR_NO_MORE_ITEMS)return TRUE;
  if(e||count==0||count>=128)return FALSE;
  if(is_root){if(weq(name,L"OwnerNonce")){if(query_string(key,name,value,128,"cleanup_read_marker")||!weq(value,nonce))return FALSE;}else if(weq(name,L"OwnerSid")){if(!same_owner_sid(key))return FALSE;}else return FALSE;}
  else if(weq(name,L"Sentinel")){if(query_string(key,name,value,128,"cleanup_read_sentinel")||!weq(value,SENTINEL))return FALSE;}
  else if(weq(name,L"FutureClassroom")){BYTE bytes[256];DWORD size=0,type=0;if(query_raw(key,name,bytes,&size,&type,"cleanup_read_target")||!raw_kind(bytes,size,type))return FALSE;}
  else return FALSE;
  index++;
 }
}
static BOOL safe_case(HKEY root,WCHAR which){
 HKEY key=NULL;LONG e=case_open(root,which,&key,KEY_READ);if(e)return FALSE;
 BOOL safe=safe_values(key,FALSE);DWORD count=0;
 e=RegQueryInfoKeyW(key,NULL,NULL,NULL,&count,NULL,NULL,NULL,NULL,NULL,NULL,NULL);event("cleanup_case_has_no_children",e,count);
 if(e||count)safe=FALSE;
 close_key(&key,"cleanup_close_case");return safe;
}

static DWORD cleanup(void){
 HKEY root=NULL,software=NULL;LONG e=open_root(&root,KEY_READ);if(e)return 40;
 BOOL safe=marker(root)&&safe_values(root,TRUE);DWORD i=0;WCHAR name[128];
 while(safe){DWORD count=128;e=RegEnumKeyExW(root,i,name,&count,NULL,NULL,NULL,NULL);event("cleanup_enum_case",e,i);
  if(e==ERROR_NO_MORE_ITEMS)break;
  if(e||count!=1||name[0]<'A'||name[0]>'H'){safe=FALSE;break;}
  if(!safe_case(root,name[0]))safe=FALSE;
  i++;
 }
 close_key(&root,"cleanup_close_verified_root");if(!safe||!resource_ok){status("CLEANUP_FAIL_RETAIN_NAMESPACE_MISMATCH_OR_CLOSE");return 41;}
 e=RegOpenKeyExW(HKEY_CURRENT_USER,L"Software",0,KEY_READ|KEY_WRITE|DELETE|VIEW,&software);event("cleanup_open_existing_software",e,0);if(e)return 42;
 e=RegDeleteTreeW(software,leaf);event("cleanup_delete_exact_leaf",e,0);close_key(&software,"cleanup_close_software");if(e||!resource_ok)return 43;
 e=RegOpenKeyExW(HKEY_CURRENT_USER,path,0,KEY_READ|VIEW,&root);event("cleanup_verify_absent",e,0);close_key(&root,"cleanup_close_unexpected_present");
 if(e!=ERROR_FILE_NOT_FOUND||!resource_ok)return 44;
 status("CLEANUP_COMPLETE_EXACT_OWNED_LEAF_ABSENT");return 0;
}

static DWORD supervised(const WCHAR *mode,WCHAR which,DWORD budget,BOOL *physically_empty){
 HANDLE job=NULL;PROCESS_INFORMATION pi;JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits;JOBOBJECT_BASIC_ACCOUNTING_INFORMATION counts;
 DWORD wait,exit=255;BOOL good=TRUE;*physically_empty=FALSE;memset(&pi,0,sizeof(pi));
 operation_deadline=GetTickCount64()+budget;
 event("operation_original_deadline_low",0,(DWORD)operation_deadline);event("operation_original_deadline_high",0,(DWORD)(operation_deadline>>32));
 job=CreateJobObjectW(NULL,NULL);event("supervisor_create_job",job?0:GetLastError(),0);if(!job)return 50;
 memset(&limits,0,sizeof(limits));limits.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
 if(!SetInformationJobObject(job,JobObjectExtendedLimitInformation,&limits,sizeof(limits))){event("supervisor_set_job",GetLastError(),0);close_handle(&job,"supervisor_close_unused_job");return 51;}
 if(!start_child(mode,which,TRUE,&pi)){close_handle(&job,"supervisor_close_unused_job");return 52;}
 if(!AssignProcessToJobObject(job,pi.hProcess)){
  event("supervisor_assign_job",GetLastError(),0);
  BOOL killed=TerminateProcess(pi.hProcess,53);event("supervisor_terminate_unassigned_suspended_child",killed?0:GetLastError(),0);
  wait=WaitForSingleObject(pi.hProcess,1000);event("supervisor_unassigned_child_actual_wait",wait,1000);good=FALSE;
 }else{
  event("supervisor_assign_job",0,pi.dwProcessId);
  if(ResumeThread(pi.hThread)==(DWORD)-1){event("supervisor_resume",GetLastError(),0);good=FALSE;}
  wait=WaitForSingleObject(pi.hProcess,remaining(budget));event("supervisor_original_budget_wait",wait,budget);
  if(wait!=WAIT_OBJECT_0){good=FALSE;status("INCONCLUSIVE_NATIVE_DEADLINE_EXHAUSTED");}
  else if(!GetExitCodeProcess(pi.hProcess,&exit)||exit!=0)good=FALSE;
 }
 if(!good){BOOL ok=TerminateJobObject(job,54);event("supervisor_terminate_only_owned_job",ok?0:GetLastError(),0);}
 ULONGLONG return_deadline=GetTickCount64()+1500;
 for(;;){memset(&counts,0,sizeof(counts));BOOL ok=QueryInformationJobObject(job,JobObjectBasicAccountingInformation,&counts,sizeof(counts),NULL);
  if(!ok){event("supervisor_job_count_error",GetLastError(),0);break;}
  if(counts.ActiveProcesses==0){*physically_empty=TRUE;event("supervisor_actual_job_active_zero",0,0);break;}
  if(GetTickCount64()>=return_deadline){event("supervisor_actual_job_not_returned",ERROR_TIMEOUT,counts.ActiveProcesses);break;}Sleep(5);
 }
 wait=WaitForSingleObject(pi.hProcess,0);event("supervisor_actual_process_wait",wait,0);
 if(wait!=WAIT_OBJECT_0||!GetExitCodeProcess(pi.hProcess,&exit)){good=FALSE;*physically_empty=FALSE;event("supervisor_actual_process_exit_error",GetLastError(),0);}else event("supervisor_actual_process_exit",0,exit);
 close_handle(&pi.hThread,"supervisor_close_thread");close_handle(&pi.hProcess,"supervisor_close_process");
 if(*physically_empty)close_handle(&job,"supervisor_close_empty_job");
 else { /* A live owner is retained until process termination; no new case or cleanup. */
  status("CLEANUP_FAIL_OWNED_JOB_NOT_PHYSICALLY_RETURNED");
 }
 return good&&*physically_empty&&resource_ok?exit:55;
}
static BOOL parse_mode(WCHAR *mode,WCHAR *which,WCHAR *value){
 const WCHAR *p=GetCommandLineW();if(*p=='"'){p++;while(*p&&*p!='"')p++;if(*p)p++;}else while(*p&&*p!=' ')p++;
 while(*p==' ')p++;
 if(!*p){mode[0]=0;return TRUE;}
 UINT n=0;while(*p&&*p!=' '&&n<31)mode[n++]=*p++;mode[n]=0;while(*p==' ')p++;
 if(!p[0]||p[1]!=' ')return FALSE;
 *which=*p;p+=2;
 n=0;while(*p&&*p!=' '&&n<33)value[n++]=*p++;value[n]=0;while(*p==' ')p++;
 if(!build_paths(value)||!*p)return FALSE;
 operation_deadline=0;UINT digits=0;
 while(*p>='0'&&*p<='9'){
  UINT digit=(UINT)(*p++-'0');if(++digits>20||operation_deadline>(UINT64_MAX-digit)/10)return FALSE;
  operation_deadline=operation_deadline*10+digit;
 }
 while(*p==' ')p++;
 return !*p&&operation_deadline!=0;
}
/* RtlGenRandom is the documented Advapi32 SystemFunction036 export. */
BOOLEAN WINAPI SystemFunction036(PVOID,ULONG);
void __cdecl entry(void){
 WCHAR mode[32],value[34],which=0;DWORD result=1;BOOL empty=FALSE,all_empty=TRUE,experiment=TRUE;
 if(!load_owner_sid())ExitProcess(65);
 DWORD length=GetModuleFileNameW(NULL,executable,1024);if(!length||length>=1024)ExitProcess(60);
 if(!parse_mode(mode,&which,value))ExitProcess(61);
 if(mode[0]){event("inherited_deadline_low",0,(DWORD)operation_deadline);event("inherited_deadline_high",0,(DWORD)(operation_deadline>>32));if(!forward_allowed())ExitProcess(66);}
 if(!mode[0]){
  BYTE random[16];const WCHAR hex[]=L"0123456789abcdef";if(!SystemFunction036(random,sizeof(random)))ExitProcess(62);
  for(UINT i=0;i<16;i++){value[i*2]=hex[random[i]>>4];value[i*2+1]=hex[random[i]&15];}value[32]=0;if(!build_paths(value))ExitProcess(63);
  /* Public synthetic ownership identifier, never a real credential. */
  char out[128],*q=ascii(out,"RWI_OWNED_NONCE ");for(UINT i=0;i<32;i++)*q++=(char)nonce[i];*q++='\n';DWORD wrote;WriteFile(GetStdHandle(STD_OUTPUT_HANDLE),out,(DWORD)(q-out),&wrote,NULL);
  result=supervised(L"--prepare",'P',PREPARE_MS,&empty);all_empty=empty;
  if(result){
   /* A timeout or nonzero prepare is NOT proof that this supervisor owns a
    * newly created key. Preserve any partial scope instead of adopting it
    * using only an observed marker. No case or cleanup starts here. */
   status("PREPARE_NOT_SUCCESSFULLY_COMPLETED_RETAIN_ROOT_NO_CLEANUP");ExitProcess(73);
  }
  if(!result)for(WCHAR c='A';c<='H';c++){
   event("supervisor_case_begin",0,c);result=supervised(L"--case",c,CASE_MS,&empty);all_empty=all_empty&&empty;if(result)experiment=FALSE;
   if(!all_empty)break;
  }
  if(all_empty){result=supervised(L"--cleanup",'D',CLEANUP_MS,&empty);if(result||!empty){status("FIXTURE_CLEANUP_FAIL");ExitProcess(70);}}
  else {status("FIXTURE_CLEANUP_FAIL_NATIVE_OWNER_RETAINED");ExitProcess(71);}
  if(experiment&&resource_ok){status("OBSERVATIONS_COMPLETE_FINITE_FALSIFICATION_ONLY_NOT_CAS_PROOF");ExitProcess(0);}
  status("EXPERIMENT_INCONCLUSIVE_CLEANUP_COMPLETED_NOT_PASS");ExitProcess(72);
 }
 else if(weq(mode,L"--prepare")&&which=='P')result=prepare();
 else if(weq(mode,L"--cleanup")&&which=='D')result=cleanup();
 else if(weq(mode,L"--case")&&which>='A'&&which<='H')result=run_case(which);
 else if(weq(mode,L"--reader")&&which>='A'&&which<='H')result=reader(which);
 else if(weq(mode,L"--writer")&&which>='A'&&which<='H')result=writer(which);
 else result=64;
 event("owned_child_exit",result,which);ExitProcess(result);
}
