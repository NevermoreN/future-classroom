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
static DWORD event_sequence;
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
 return ERROR_SUCCESS;
}
static BOOL marker(HKEY root){WCHAR value[64];LONG e=query_string(root,L"OwnerNonce",value,64,"query_marker");return e==0&&weq(value,nonce);}
static LONG case_open(HKEY root,WCHAR name,HKEY *key,REGSAM access){WCHAR sub[2]={name,0};LONG e=RegOpenKeyExW(root,sub,0,access|VIEW,key);event("open_case",e,name);return e;}

static DWORD prepare(void){
 HKEY software=NULL,root=NULL,key=NULL;DWORD disposition=0;LONG e;
 e=RegOpenKeyExW(HKEY_CURRENT_USER,L"Software",0,KEY_CREATE_SUB_KEY|VIEW,&software);event("prepare_open_existing_software",e,0);if(e)return 10;
 e=RegCreateKeyExW(software,leaf,0,NULL,REG_OPTION_NON_VOLATILE,KEY_ALL_ACCESS|VIEW,NULL,&root,&disposition);
 event("prepare_create_leaf",e,disposition);
 if(e||disposition!=REG_CREATED_NEW_KEY){close_key(&root,"prepare_close_unclaimed");close_key(&software,"prepare_close_software");return 11;}
 e=set_string(root,L"OwnerNonce",nonce,"prepare_set_marker");
 for(WCHAR c='A';!e&&c<='C';c++){
  WCHAR sub[2]={c,0};e=RegCreateKeyExW(root,sub,0,NULL,0,KEY_ALL_ACCESS|VIEW,NULL,&key,&disposition);event("prepare_create_case",e,disposition);
  if(!e&&disposition!=REG_CREATED_NEW_KEY)e=ERROR_ALREADY_EXISTS;
  if(!e)e=set_string(key,L"FutureClassroom",ORIGINAL,"prepare_set_original");
  if(!e)e=set_string(key,L"Sentinel",SENTINEL,"prepare_set_sentinel");
  if(!close_key(&key,"prepare_close_case"))e=ERROR_INVALID_HANDLE;
 }
 close_key(&root,"prepare_close_root");close_key(&software,"prepare_close_software");
 return e||!resource_ok?12:0;
}

static BOOL command(WCHAR *out,const WCHAR *mode,WCHAR which){
 wc(out,L"fc-rwi ");wa(out,mode);wa(out,L" ");UINT n=wl(out);out[n++]=which;out[n]=0;wa(out,L" ");wa(out,nonce);return TRUE;
}
static BOOL start_child(const WCHAR *mode,WCHAR which,BOOL suspended,PROCESS_INFORMATION *pi){
 STARTUPINFOW si;WCHAR cmd[256];memset(&si,0,sizeof(si));memset(pi,0,sizeof(*pi));si.cb=sizeof(si);
 si.dwFlags=STARTF_USESTDHANDLES;si.hStdInput=GetStdHandle(STD_INPUT_HANDLE);si.hStdOutput=GetStdHandle(STD_OUTPUT_HANDLE);si.hStdError=GetStdHandle(STD_ERROR_HANDLE);
 command(cmd,mode,which);
 BOOL ok=CreateProcessW(executable,cmd,NULL,NULL,TRUE,CREATE_NO_WINDOW|(suspended?CREATE_SUSPENDED:0),NULL,NULL,&si,pi);
 event("create_child",ok?0:GetLastError(),ok?pi->dwProcessId:0);return ok;
}
static DWORD writer(WCHAR which){
 HKEY root=NULL,key=NULL;LONG e=open_root(&root,KEY_READ);if(e)return 20;
 if(!marker(root)){close_key(&root,"writer_close_bad_marker");return 21;}
 e=case_open(root,which,&key,KEY_SET_VALUE);close_key(&root,"writer_close_root");
 if(!e){status("ordinary_set_begin");e=set_string(key,L"FutureClassroom",REPLACEMENT,"ordinary_set_return");}
 close_key(&key,"writer_close_case");
 /* Native status is always in the original log; expected conflicts cannot
  * masquerade as successful writes via an exit-code truncation. */
 if(!resource_ok)return 22;
 return e==ERROR_SUCCESS?0:23;
}
static BOOL rollback(HANDLE tx,const char *tag){BOOL ok=RollbackTransaction(tx);event(tag,ok?0:GetLastError(),0);return ok;}
static LONG final_value(HKEY root,WCHAR which,DWORD *kind){
 HKEY key=NULL;WCHAR value[128];LONG e=case_open(root,which,&key,KEY_QUERY_VALUE);*kind=0;
 if(!e)e=query_string(key,L"FutureClassroom",value,128,"final_query_target");
 if(e==ERROR_FILE_NOT_FOUND){*kind=1;e=0;}
 else if(!e){if(weq(value,ORIGINAL))*kind=2;else if(weq(value,REPLACEMENT))*kind=3;else e=ERROR_INVALID_DATA;}
 close_key(&key,"final_close_case");event("final_target_kind",e,*kind);return e;
}
static DWORD run_case(WCHAR which){
 HKEY root=NULL,parent=NULL,child=NULL;HANDLE tx=NULL;PROCESS_INFORMATION writer_pi;memset(&writer_pi,0,sizeof(writer_pi));
 WCHAR keypath[260],intent[64],preimage[128];LONG e=0;DWORD disposition=0,wait=0,writer_exit=255,kind=0;BOOL committed=FALSE,known=TRUE;
 if(open_root(&root,KEY_READ)||!marker(root))return 30;
 tx=CreateTransaction(NULL,NULL,0,0,0,3000,L"FutureClassroom scratch write intent only");
 event("create_transaction",tx==INVALID_HANDLE_VALUE?GetLastError():0,0);if(tx==INVALID_HANDLE_VALUE){tx=NULL;known=FALSE;goto done;}
 wc(keypath,path);wa(keypath,L"\\");UINT n=wl(keypath);keypath[n++]=which;keypath[n]=0;
 e=RegOpenKeyTransactedW(HKEY_CURRENT_USER,keypath,0,KEY_ALL_ACCESS|VIEW,&parent,tx,NULL);event("tx_open_parent",e,which);if(e){known=FALSE;goto done;}
 wc(intent,L"intent_");wa(intent,nonce);
 if(which!='A'){
  status("tx_create_intent_before_target_read_begin");
  e=RegCreateKeyTransactedW(parent,intent,0,NULL,0,KEY_ALL_ACCESS|VIEW,NULL,&child,&disposition,tx,NULL);event("tx_create_intent_return",e,disposition);
  if(e||disposition!=REG_CREATED_NEW_KEY){known=FALSE;goto done;}
  if(!close_key(&child,"tx_close_new_child")){known=FALSE;goto done;}
 }
 e=query_string(parent,L"FutureClassroom",preimage,128,"tx_read_target");if(e||!weq(preimage,ORIGINAL)){known=FALSE;goto done;}
 if(which=='C'){e=RegDeleteKeyTransactedW(parent,intent,VIEW,0,tx,NULL);event("tx_delete_intent_before_writer",e,0);if(e){known=FALSE;goto done;}}
 if(!start_child(L"--writer",which,FALSE,&writer_pi)){known=FALSE;goto done;}
 close_handle(&writer_pi.hThread,"case_close_writer_thread");
 wait=WaitForSingleObject(writer_pi.hProcess,WRITER_OBSERVE_MS);event("ordinary_wait_initial",wait,WRITER_OBSERVE_MS);
 if(wait!=WAIT_OBJECT_0){
  status("INCONCLUSIVE_ORDINARY_NOT_RETURNED_WITHIN_ORIGINAL_OBSERVATION");known=FALSE;
  rollback(tx,"blocked_original_tx_rollback");
  close_key(&parent,"blocked_close_tx_parent");close_handle(&tx,"blocked_close_tx_handle");
  wait=WaitForSingleObject(writer_pi.hProcess,WRITER_RETURN_MS);event("ordinary_wait_after_rollback",wait,WRITER_RETURN_MS);
  goto done;
 }
 if(!GetExitCodeProcess(writer_pi.hProcess,&writer_exit)){event("ordinary_exit_query",GetLastError(),0);known=FALSE;goto done;}event("ordinary_actual_exit",0,writer_exit);
 if(writer_exit!=0){status("INCONCLUSIVE_ORDINARY_REJECTED_OR_RESOURCE_FAILURE");known=FALSE;goto done;}
 status("tx_delete_target_begin");e=RegDeleteValueW(parent,L"FutureClassroom");event("tx_delete_target_return",e,0);if(e){known=FALSE;goto done;}
 if(which=='B'){e=RegDeleteKeyTransactedW(parent,intent,VIEW,0,tx,NULL);event("tx_delete_intent_after_target_stage",e,0);if(e){known=FALSE;goto done;}}
 status("tx_commit_begin");committed=CommitTransaction(tx);event("tx_commit_return",committed?0:GetLastError(),0);
 if(!committed)known=FALSE;
 done:
 if(tx&&!committed&&!rollback(tx,"final_tx_rollback"))known=FALSE;
 close_key(&child,"final_close_tx_child");close_key(&parent,"final_close_tx_parent");close_handle(&tx,"final_close_tx_handle");
 if(writer_pi.hProcess){
  wait=WaitForSingleObject(writer_pi.hProcess,0);event("ordinary_final_exit_wait",wait,0);
  if(wait!=WAIT_OBJECT_0){known=FALSE;status("INCONCLUSIVE_ORDINARY_PROCESS_STILL_ACTIVE");}
  close_handle(&writer_pi.hThread,"final_close_writer_thread");close_handle(&writer_pi.hProcess,"final_close_writer_process");
 }
 /* Ordinary query is only after transaction handle/key closure. */
 if(!tx&&!parent&&!child){if(final_value(root,which,&kind))known=FALSE;}else known=FALSE;
 close_key(&root,"final_close_root");
 if(known&&resource_ok&&committed&&writer_exit==0&&kind==1){status(which=='A'?"BASELINE_LOST_REPLACEMENT_REPRODUCED":"CANDIDATE_FALSIFIED_LOST_REPLACEMENT");return 0;}
 status("INCONCLUSIVE_NOT_A_UNIVERSAL_CAS_PROOF");return 31;
}

/* Cleanup is a separate bounded process and runs only after the supervisor has
 * observed every case job ActiveProcesses==0. It accepts a CLOSED synthetic
 * namespace, never arbitrary content or a marker alone. */
static BOOL safe_values(HKEY key,BOOL is_root){
 DWORD index=0;WCHAR name[128],value[128];LONG e;
 for(;;){DWORD count=128;memset(name,0,sizeof(name));e=RegEnumValueW(key,index,name,&count,NULL,NULL,NULL,NULL);event("cleanup_enum_value",e,index);
  if(e==ERROR_NO_MORE_ITEMS)return TRUE;if(e||count==0||count>=128)return FALSE;
  if(is_root){if(!weq(name,L"OwnerNonce")||query_string(key,name,value,128,"cleanup_read_marker")||!weq(value,nonce))return FALSE;}
  else if(weq(name,L"Sentinel")){if(query_string(key,name,value,128,"cleanup_read_sentinel")||!weq(value,SENTINEL))return FALSE;}
  else if(weq(name,L"FutureClassroom")){if(query_string(key,name,value,128,"cleanup_read_target")||(!weq(value,ORIGINAL)&&!weq(value,REPLACEMENT)))return FALSE;}
  else return FALSE;
  index++;
 }
}
static BOOL safe_case(HKEY root,WCHAR which){
 HKEY key=NULL,child=NULL;WCHAR name[128],intent[64];LONG e=case_open(root,which,&key,KEY_READ);if(e)return FALSE;
 BOOL safe=safe_values(key,FALSE);DWORD index=0;wc(intent,L"intent_");wa(intent,nonce);
 while(safe){DWORD count=128;e=RegEnumKeyExW(key,index,name,&count,NULL,NULL,NULL,NULL);event("cleanup_enum_intent",e,index);
  if(e==ERROR_NO_MORE_ITEMS)break;if(e||count>=128||!weq(name,intent)){safe=FALSE;break;}
  e=RegOpenKeyExW(key,name,0,KEY_READ|VIEW,&child);event("cleanup_open_intent",e,0);
  DWORD subkeys=0,values=0;if(!e)e=RegQueryInfoKeyW(child,NULL,NULL,NULL,&subkeys,NULL,NULL,&values,NULL,NULL,NULL,NULL);event("cleanup_intent_empty",e,subkeys+values);
  if(e||subkeys||values)safe=FALSE;close_key(&child,"cleanup_close_intent");index++;
 }
 close_key(&key,"cleanup_close_case");return safe;
}
static DWORD cleanup(void){
 HKEY root=NULL,software=NULL;LONG e=open_root(&root,KEY_READ);if(e)return 40;
 BOOL safe=marker(root)&&safe_values(root,TRUE);DWORD i=0;WCHAR name[128];
 while(safe){DWORD count=128;e=RegEnumKeyExW(root,i,name,&count,NULL,NULL,NULL,NULL);event("cleanup_enum_case",e,i);
  if(e==ERROR_NO_MORE_ITEMS)break;if(e||count!=1||name[0]<'A'||name[0]>'C'){safe=FALSE;break;}
  if(!safe_case(root,name[0]))safe=FALSE;i++;
 }
 close_key(&root,"cleanup_close_verified_root");if(!safe||!resource_ok){status("CLEANUP_FAIL_RETAIN_NAMESPACE_MISMATCH_OR_CLOSE");return 41;}
 e=RegOpenKeyExW(HKEY_CURRENT_USER,L"Software",0,KEY_READ|KEY_WRITE|DELETE|VIEW,&software);event("cleanup_open_existing_software",e,0);if(e)return 42;
 e=RegDeleteTreeW(software,leaf);event("cleanup_delete_exact_leaf",e,0);close_key(&software,"cleanup_close_software");if(e||!resource_ok)return 43;
 e=RegOpenKeyExW(HKEY_CURRENT_USER,path,0,KEY_READ|VIEW,&root);event("cleanup_verify_absent",e,0);close_key(&root,"cleanup_close_unexpected_present");
 if(e!=ERROR_FILE_NOT_FOUND||!resource_ok)return 44;status("CLEANUP_COMPLETE_EXACT_OWNED_LEAF_ABSENT");return 0;
}

static DWORD supervised(const WCHAR *mode,WCHAR which,DWORD budget,BOOL *physically_empty){
 HANDLE job=NULL;PROCESS_INFORMATION pi;JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits;JOBOBJECT_BASIC_ACCOUNTING_INFORMATION counts;
 DWORD wait,exit=255;BOOL good=TRUE;*physically_empty=FALSE;memset(&pi,0,sizeof(pi));
 job=CreateJobObjectW(NULL,NULL);event("supervisor_create_job",job?0:GetLastError(),0);if(!job)return 50;
 memset(&limits,0,sizeof(limits));limits.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
 if(!SetInformationJobObject(job,JobObjectExtendedLimitInformation,&limits,sizeof(limits))){event("supervisor_set_job",GetLastError(),0);close_handle(&job,"supervisor_close_unused_job");return 51;}
 if(!start_child(mode,which,TRUE,&pi)){close_handle(&job,"supervisor_close_unused_job");return 52;}
 if(!AssignProcessToJobObject(job,pi.hProcess)){
  event("supervisor_assign_job",GetLastError(),0);TerminateProcess(pi.hProcess,53);WaitForSingleObject(pi.hProcess,1000);good=FALSE;
 }else{
  event("supervisor_assign_job",0,pi.dwProcessId);
  if(ResumeThread(pi.hThread)==(DWORD)-1){event("supervisor_resume",GetLastError(),0);good=FALSE;}
  wait=WaitForSingleObject(pi.hProcess,budget);event("supervisor_original_budget_wait",wait,budget);
  if(wait!=WAIT_OBJECT_0){good=FALSE;status("INCONCLUSIVE_NATIVE_DEADLINE_EXHAUSTED");}
 }
 if(!good){BOOL ok=TerminateJobObject(job,54);event("supervisor_terminate_only_owned_job",ok?0:GetLastError(),0);}
 ULONGLONG return_deadline=GetTickCount64()+1500;
 for(;;){memset(&counts,0,sizeof(counts));BOOL ok=QueryInformationJobObject(job,JobObjectBasicAccountingInformation,&counts,sizeof(counts),NULL);
  if(!ok){event("supervisor_job_count_error",GetLastError(),0);break;}
  if(counts.ActiveProcesses==0){*physically_empty=TRUE;event("supervisor_actual_job_active_zero",0,0);break;}
  if(GetTickCount64()>=return_deadline){event("supervisor_actual_job_not_returned",ERROR_TIMEOUT,counts.ActiveProcesses);break;}Sleep(5);
 }
 wait=WaitForSingleObject(pi.hProcess,0);event("supervisor_actual_process_wait",wait,0);
 if(wait!=WAIT_OBJECT_0||!GetExitCodeProcess(pi.hProcess,&exit)){good=FALSE;event("supervisor_actual_process_exit_error",GetLastError(),0);}else event("supervisor_actual_process_exit",0,exit);
 close_handle(&pi.hThread,"supervisor_close_thread");close_handle(&pi.hProcess,"supervisor_close_process");
 if(*physically_empty)close_handle(&job,"supervisor_close_empty_job");
 else { /* A live owner is retained until process termination; no new case or cleanup. */
  status("CLEANUP_FAIL_OWNED_JOB_NOT_PHYSICALLY_RETURNED");
 }
 return good&&*physically_empty&&resource_ok?exit:55;
}
static BOOL parse_mode(WCHAR *mode,WCHAR *which,WCHAR *value){
 const WCHAR *p=GetCommandLineW();if(*p=='"'){p++;while(*p&&*p!='"')p++;if(*p)p++;}else while(*p&&*p!=' ')p++;
 while(*p==' ')p++;if(!*p){mode[0]=0;return TRUE;}
 UINT n=0;while(*p&&*p!=' '&&n<31)mode[n++]=*p++;mode[n]=0;while(*p==' ')p++;
 *which=*p++;if(*p++!=' ')return FALSE;
 n=0;while(*p&&*p!=' '&&n<33)value[n++]=*p++;value[n]=0;while(*p==' ')p++;
 return !*p&&build_paths(value);
}
/* RtlGenRandom is the documented Advapi32 SystemFunction036 export. */
BOOLEAN WINAPI SystemFunction036(PVOID,ULONG);
void __cdecl entry(void){
 WCHAR mode[32],value[34],which=0;DWORD result=1;BOOL empty=FALSE,all_empty=TRUE,experiment=TRUE;
 DWORD length=GetModuleFileNameW(NULL,executable,1024);if(!length||length>=1024)ExitProcess(60);
 if(!parse_mode(mode,&which,value))ExitProcess(61);
 if(!mode[0]){
  BYTE random[16];const WCHAR hex[]=L"0123456789abcdef";if(!SystemFunction036(random,sizeof(random)))ExitProcess(62);
  for(UINT i=0;i<16;i++){value[i*2]=hex[random[i]>>4];value[i*2+1]=hex[random[i]&15];}value[32]=0;if(!build_paths(value))ExitProcess(63);
  /* Public synthetic ownership identifier, never a real credential. */
  char out[128],*q=ascii(out,"RWI_OWNED_NONCE ");for(UINT i=0;i<32;i++)*q++=(char)nonce[i];*q++='\n';DWORD wrote;WriteFile(GetStdHandle(STD_OUTPUT_HANDLE),out,(DWORD)(q-out),&wrote,NULL);
  result=supervised(L"--prepare",'P',PREPARE_MS,&empty);all_empty=empty;
  if(result)experiment=FALSE;
  if(!result)for(WCHAR c='A';c<='C';c++){
   event("supervisor_case_begin",0,c);result=supervised(L"--case",c,CASE_MS,&empty);all_empty=all_empty&&empty;if(result)experiment=FALSE;
   if(!all_empty)break;
  }
  if(all_empty){result=supervised(L"--cleanup",'D',CLEANUP_MS,&empty);if(result||!empty){status("FIXTURE_CLEANUP_FAIL");ExitProcess(70);}}
  else {status("FIXTURE_CLEANUP_FAIL_NATIVE_OWNER_RETAINED");ExitProcess(71);}
  if(experiment&&resource_ok){status("EXPERIMENT_COMPLETE_FINITE_FALSIFICATION_ONLY_NOT_CAS_PROOF");ExitProcess(0);}
  status("EXPERIMENT_INCONCLUSIVE_CLEANUP_COMPLETED_NOT_PASS");ExitProcess(72);
 }
 else if(weq(mode,L"--prepare")&&which=='P')result=prepare();
 else if(weq(mode,L"--cleanup")&&which=='D')result=cleanup();
 else if(weq(mode,L"--case")&&which>='A'&&which<='C')result=run_case(which);
 else if(weq(mode,L"--writer")&&which>='A'&&which<='C')result=writer(which);
 else result=64;
 event("owned_child_exit",result,which);ExitProcess(result);
}
