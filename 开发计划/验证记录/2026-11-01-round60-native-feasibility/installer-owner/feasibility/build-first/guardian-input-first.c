#define UNICODE
#define _UNICODE
#define _WIN32_WINNT 0x0601
#define _WIN32_MSI 450
#include <windows.h>
#include <msi.h>
#include <sddl.h>
#include <stdio.h>
#include <wchar.h>
#include <stdlib.h>
#include <errno.h>
#include "fixture.h"

/* Synthetic owned HKCU only. This is a transaction feasibility observer,
 * not a production gate, service, arbitrary command runner or MSI acceptance. */
static HKEY evidence;
static int dword(const wchar_t *name, DWORD value) {
    return RegSetValueExW(evidence,name,0,REG_DWORD,(const BYTE *)&value,sizeof(value))==ERROR_SUCCESS;
}
static int qword(const wchar_t *name, ULONGLONG value) {
    return RegSetValueExW(evidence,name,0,REG_QWORD,(const BYTE *)&value,sizeof(value))==ERROR_SUCCESS;
}
static int text(const wchar_t *name,const wchar_t *value) {
    size_t n=wcslen(value); if(n>256) return 0;
    return RegSetValueExW(evidence,name,0,REG_SZ,(const BYTE *)value,(DWORD)((n+1)*sizeof(wchar_t)))==ERROR_SUCCESS;
}
static int fixed_id(const wchar_t *s) {
    if(wcslen(s)!=32) return 0;
    for(size_t n=0;n<32;n++) if(!((s[n]>=L'0'&&s[n]<=L'9')||(s[n]>=L'a'&&s[n]<=L'f'))) return 0;
    return 1;
}
static int current_sid(wchar_t *out,size_t capacity) {
    HANDLE token=NULL; BYTE data[512]; DWORD length=0; LPWSTR sid=NULL; int ok=0;
    if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token)) return 0;
    if(GetTokenInformation(token,TokenUser,data,sizeof(data),&length)&&
       ConvertSidToStringSidW(((TOKEN_USER *)data)->User.Sid,&sid)&&wcslen(sid)<capacity) {
        wcscpy(out,sid);ok=1;
    }
    if(sid) LocalFree(sid);
    CloseHandle(token);return ok;
}
int wmain(int argc,wchar_t **argv) {
    HKEY control=NULL; DWORD kind=0,bytes=0,decision=0; wchar_t operation[33]={0};
    wchar_t keypath[256],ready_name[160],ack_name[160],sid[192];
    HANDLE ready=NULL,ack=NULL,changed=NULL; DWORD code=2,wait_result=WAIT_FAILED;
    UINT join_result=ERROR_INVALID_PARAMETER,end_result=ERROR_INVALID_PARAMETER;
    unsigned long raw=0; wchar_t *tail=NULL; FILETIME creation,exit_time,kernel,user;
    if(argc!=2) return 10;
    errno=0;raw=wcstoul(argv[1],&tail,0);
    if(errno||!tail||*tail||raw==0) return 11;
    /* Never create/control arbitrary paths supplied by arguments. */
    if(RegOpenKeyExW(HKEY_CURRENT_USER,FIXTURE_ROOT,0,KEY_QUERY_VALUE,&control)!=ERROR_SUCCESS) return 12;
    bytes=sizeof(operation);
    if(RegQueryValueExW(control,L"CurrentOperation",NULL,&kind,(BYTE *)operation,&bytes)!=ERROR_SUCCESS||kind!=REG_SZ||bytes!=sizeof(operation)||!fixed_id(operation)) {RegCloseKey(control);return 13;}
    bytes=sizeof(decision);
    if(RegQueryValueExW(control,L"Decision",NULL,&kind,(BYTE *)&decision,&bytes)!=ERROR_SUCCESS||kind!=REG_DWORD||bytes!=sizeof(decision)||decision>1) {RegCloseKey(control);return 14;}
    RegCloseKey(control);control=NULL;
    if(_snwprintf(keypath,256,L"%ls\\Evidence\\%ls\\Guardian-%lu",FIXTURE_ROOT,operation,(unsigned long)GetCurrentProcessId())<0) return 15;
    if(RegCreateKeyExW(HKEY_CURRENT_USER,keypath,0,NULL,0,KEY_SET_VALUE,NULL,&evidence,NULL)!=ERROR_SUCCESS) return 16;
    if(!current_sid(sid,192)||!GetProcessTimes(GetCurrentProcess(),&creation,&exit_time,&kernel,&user)) goto done;
    ULARGE_INTEGER created;created.LowPart=creation.dwLowDateTime;created.HighPart=creation.dwHighDateTime;
    if(!text(L"ActualSid",sid)||!dword(L"Pid",GetCurrentProcessId())||!qword(L"CreationFileTime",created.QuadPart)||
       !dword(L"ArgCount",(DWORD)argc)||!dword(L"TransactionId",(DWORD)raw)||!dword(L"RequestedDecision",decision)||!text(L"Stage",L"Entered")) goto done;
    if(_snwprintf(ready_name,160,L"Global\\FCMSI-%ls-%ls-ready",FIXTURE_NONCE,operation)<0||
       _snwprintf(ack_name,160,L"Global\\FCMSI-%ls-%ls-ack",FIXTURE_NONCE,operation)<0) goto done;
    ready=OpenEventW(EVENT_MODIFY_STATE,FALSE,ready_name);
    ack=OpenEventW(SYNCHRONIZE,FALSE,ack_name);
    if(!ready||!ack) { dword(L"EventOpenError",GetLastError()); goto done; }
    /* The external owned supervisor retains this exact original process handle
     * before ACK. No emitted record or requested stop is physical-exit proof. */
    if(!SetEvent(ready)) goto done;
    wait_result=WaitForSingleObject(ack,10000);
    if(!dword(L"ObserveAckWait",wait_result)||wait_result!=WAIT_OBJECT_0) goto done;
    join_result=MsiJoinTransaction((MSIHANDLE)raw,0,&changed);
    if(!dword(L"JoinReturn",join_result)) goto done;
    if(join_result!=ERROR_SUCCESS) {code=20;goto done;}
    if(!text(L"Stage",L"Joined")) goto done;
    DWORD owner_signal=changed?WaitForSingleObject(changed,0):WAIT_FAILED;
    if(!dword(L"OwnerEventBeforeEnd",owner_signal)||!dword(L"EndEntered",1)) goto done;
    end_result=MsiEndTransaction(decision?MSITRANSACTIONSTATE_COMMIT:MSITRANSACTIONSTATE_ROLLBACK);
    if(!dword(L"EndReturn",end_result)||!dword(L"EndReturned",1)||!text(L"Stage",L"EndReturned")) goto done;
    code=end_result==ERROR_SUCCESS?0:21;
 done:
    /* This probe never closes the engine-provided transaction ID: its owning
     * API lifecycle belongs to the engine/Join-End protocol. Only the returned
     * Win32 event and our own observer handles are closed here. */
    if(changed&&!CloseHandle(changed)) code=22;
    if(ready&&!CloseHandle(ready)) code=22;
    if(ack&&!CloseHandle(ack)) code=22;
    if(evidence) {
        if(!dword(L"AboutToReturnCode",code)) code=23;
        if(RegCloseKey(evidence)!=ERROR_SUCCESS) code=23;
    }
    return (int)code;
}
