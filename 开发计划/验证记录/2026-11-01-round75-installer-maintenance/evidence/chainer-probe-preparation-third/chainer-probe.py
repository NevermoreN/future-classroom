#!/usr/bin/env python3
"""Independent rollback-only chainer fixture preparation/checking. Never compiles or executes."""
import argparse
import hashlib
import json
from pathlib import Path, PureWindowsPath
import re
import stat
import struct
import subprocess
import uuid
import xml.etree.ElementTree as ET

NS='http://schemas.microsoft.com/wix/2006/wi'
NAMESPACE=uuid.UUID('31aa0673-32c2-4c95-b343-b4f1071b0950')
MARKER='future-classroom-owned-r75-chainer-probe/v1\n'
CONDITION='NOT (REMOVE = "ALL") AND NOT (ACTION = "ADVERTISE")'
NAME='FutureClassroom independent rollback chainer probe'
FORBIDDEN={'File','RemoveRegistry','RemoveFile','CreateFolder','ServiceInstall','ServiceControl','Shortcut','Environment','IniFile','RemoveIniFile','LockPermissions','MsiLockPermissionsEx','Upgrade','CustomAction'}
ALLOWED={'Component','Registry','FeatureComponents','Property','Directory','Feature','Media','LaunchCondition','Error','Binary','MsiEmbeddedChainer','InstallExecuteSequence','InstallUISequence','AdminExecuteSequence','AdminUISequence','AdvtExecuteSequence'}
SYSTEM={'_Tables','_Columns','_Validation','_SummaryInformation'}
EXECUTE={'LaunchConditions':100,'ValidateProductID':700,'CostInitialize':800,'FileCost':900,'CostFinalize':1000,'InstallValidate':1400,'InstallInitialize':1500,'AllocateRegistrySpace':1550,'ProcessComponents':1600,'UnpublishComponents':1700,'UnpublishFeatures':1800,'RemoveRegistryValues':2600,'RemoveFiles':3500,'RemoveFolders':3600,'CreateFolders':3700,'InstallFiles':4000,'WriteRegistryValues':5000,'RegisterUser':6000,'RegisterProduct':6100,'PublishComponents':6200,'PublishFeatures':6300,'PublishProduct':6400,'InstallExecute':6500,'InstallFinalize':6600}
AUX={
 'InstallUISequence':{'LaunchConditions':100,'ValidateProductID':700,'CostInitialize':800,'FileCost':900,'CostFinalize':1000,'ExecuteAction':1300},
 'AdminUISequence':{'CostInitialize':800,'FileCost':900,'CostFinalize':1000,'ExecuteAction':1300},
 'AdminExecuteSequence':{'CostInitialize':800,'FileCost':900,'CostFinalize':1000,'InstallValidate':1400,'InstallInitialize':1500,'InstallAdminPackage':3900,'InstallFiles':4000,'InstallFinalize':6600},
 'AdvtExecuteSequence':{'CostInitialize':800,'CostFinalize':1000,'InstallValidate':1400,'InstallInitialize':1500,'PublishFeatures':6300,'PublishProduct':6400,'InstallFinalize':6600}}

def need(v,m):
    if not v: raise ValueError(m)
def sha(b): return hashlib.sha256(b).hexdigest()
def read(p,limit=4*1024*1024):
    need(stat.S_ISREG(p.lstat().st_mode) and p.stat().st_size<=limit,'bounded regular input')
    return p.read_bytes()
def identity(p):
    b=read(p);return {'bytes':len(b),'sha256':sha(b)}
def json_bytes(v): return (json.dumps(v,indent=2,ensure_ascii=False)+'\n').encode()

def parameters(nonce,root,owner):
    need(re.fullmatch('[0-9a-f]{32}',nonce),'nonce32 lowercase hex')
    need(re.fullmatch(r'S-1-5-21-(?:[0-9]{1,10}-){2}[0-9]{1,10}-[0-9]{1,10}',owner),'explicit local/domain user owner SID')
    need(all(int(n)<=0xffffffff for n in owner.split('-')[4:]),'SID subauthority range')
    p=PureWindowsPath(root)
    need(root.isascii() and p.is_absolute() and re.fullmatch('[A-Za-z]:',p.drive) and root==str(p),'canonical absolute drive path')
    need(p.name=='FutureClassroom-r75-chainer-probe-'+nonce and len(root)<=200 and len(p.parts)<=17,'fixed owned root and bound')
    aliases={'CON','PRN','AUX','NUL'}|{'COM'+str(i) for i in range(1,10)}|{'LPT'+str(i) for i in range(1,10)}
    need(':' not in root[2:] and not any(c in root for c in '\x00\r\n\t"[]*?<>|/%') and all(x not in ('.','..') and not x.endswith((' ','.')) and x.split('.')[0].upper() not in aliases for x in p.parts[1:]),'path aliases/MSI format forbidden')
    command=subprocess.list2cmdline(['probe',nonce,root]);need(len(command)<=255,'chainer CommandLine field bound')
    return {'schemaVersion':1,'scope':'INDEPENDENT_ROLLBACK_ONLY_API_FIXTURE_NOT_PRODUCT','nonce':nonce,'windowsReportRoot':root,'expectedOwnerSid':owner,'requiredMarker':MARKER,'productCode':'{'+str(uuid.uuid5(NAMESPACE,nonce+':product')).upper()+'}','upgradeCode':'{'+str(uuid.uuid5(NAMESPACE,nonce+':family')).upper()+'}','packageCode':'{'+str(uuid.uuid5(NAMESPACE,nonce+':package-registry-second')).upper()+'}','componentCode':'{'+str(uuid.uuid5(NAMESPACE,nonce+':component-registry-second')).upper()+'}','registryKey':'Software\\FutureClassroom-Test-ChainerProbe-'+nonce,'registryValue':'OwnedMarker','registryData':1,'registryView':'Registry32','commandLine':command,'runtimeBudgetSeconds':120}

def source(spec,template):
    need(set(re.findall(r'@@[A-Z_]+@@',template))=={'@@NONCE@@','@@ROOT@@','@@OWNER_SID@@'},'exact template slots')
    return template.replace('@@NONCE@@',spec['nonce']).replace('@@ROOT@@',spec['windowsReportRoot']).replace('@@OWNER_SID@@',spec['expectedOwnerSid']).encode('ascii')

def authoring(v):
    ET.register_namespace('',NS)
    def add(p,t,**a):return ET.SubElement(p,'{'+NS+'}'+t,a)
    wix=ET.Element('{'+NS+'}Wix')
    p=add(wix,'Product',Id=v['productCode'],UpgradeCode=v['upgradeCode'],Name=NAME,Language='1033',Version='0.0.1',Manufacturer='FutureClassroom test-only')
    add(p,'Package',Id=v['packageCode'],InstallerVersion='500',InstallScope='perUser',Compressed='yes')
    add(p,'Property',Id='MSIRESTARTMANAGERCONTROL',Value='DisableShutdown')
    add(p,'Condition',Message='Independent chainer fixture is per-user only.').text='NOT ALLUSERS'
    add(p,'Condition',Message='Independent chainer fixture cannot close applications.').text='MSIRESTARTMANAGERCONTROL = "DisableShutdown"'
    add(p,'Binary',Id='FcIndependentChainer',SourceFile='chainer-probe.exe')
    target=add(p,'Directory',Id='TARGETDIR',Name='SourceDir')
    local=add(target,'Directory',Id='LocalAppDataFolder')
    component=add(local,'Component',Id='ChainerRegistryProbe',Guid=v['componentCode'],Win64='no')
    add(component,'RegistryValue',Id='ChainerRegistryMarker',Root='HKCU',Key=v['registryKey'],Name=v['registryValue'],Type='integer',Value='1',KeyPath='yes')
    feature=add(p,'Feature',Id='ProbeFeature',Title='Rollback-only chainer; owned HKCU marker',Level='1')
    add(feature,'ComponentRef',Id='ChainerRegistryProbe')
    ET.indent(wix)
    return ET.tostring(wix,encoding='utf-8',xml_declaration=True)+b'\n'

def chainer_idt(v):
    return ('MsiEmbeddedChainer\tCondition\tCommandLine\tSource\tType\r\ns72\tS255\tS255\ts72\ti2\r\nMsiEmbeddedChainer\tMsiEmbeddedChainer\r\nFcIndependentChainer\t'+CONDITION+'\t'+v['commandLine']+'\tFcIndependentChainer\t2\r\n').encode('ascii')

def idt(raw,name):
    need(len(raw)<=1024*1024 and raw.isascii() and b'\0' not in raw,'ordinary IDT ASCII/non-NUL bound')
    lines=raw.decode('ascii').splitlines();need(len(lines)>=3,'IDT header')
    columns=lines[0].split('\t');keys=lines[2].split('\t')
    need(keys[0]==name and len(keys)>=2 and set(keys[1:])<=set(columns) and len(set(keys[1:]))==len(keys[1:]) and len(set(columns))==len(columns) and len(lines[1].split('\t'))==len(columns),'IDT table/columns/keys')
    need(all(len(x.split('\t'))==len(columns) for x in lines[3:]),'IDT width')
    rows=[dict(zip(columns,line.split('\t'))) for line in lines[3:]]
    need(len({tuple(row[k] for k in keys[1:]) for row in rows})==len(rows),'duplicate IDT key')
    return rows

def pe(data):
    need(len(data)>=64 and data[:2]==b'MZ','PE DOS header');p=struct.unpack_from('<I',data,60)[0]
    need(p+248<=len(data) and data[p:p+4]==b'PE\0\0' and struct.unpack_from('<H',data,p+4)[0]==0x14c,'x86 PE')
    n=struct.unpack_from('<H',data,p+6)[0];opt=struct.unpack_from('<H',data,p+20)[0];o=p+24
    need(1<=n<=16 and opt>=224 and o+opt+n*40<=len(data),'PE layout bound')
    need(struct.unpack_from('<H',data,o)[0]==0x10b and struct.unpack_from('<H',data,o+68)[0]==2,'PE32 GUI2')
    rva,size=struct.unpack_from('<II',data,o+96+14*8);need(rva>0 and size>=72,'CLR header required')
    positions=[]
    for i in range(n):
        s=o+opt+i*40;virtual,addr,rawsize,raw=struct.unpack_from('<IIII',data,s+8)
        if addr<=rva and rva+72<=addr+rawsize:
            offset=raw+rva-addr;need(offset+72<=len(data),'CLR file bound');positions.append(offset)
    need(len(positions)==1,'unique CLR section');flags=struct.unpack_from('<I',data,positions[0]+16)[0]
    need(flags&3==3 and not flags&0x20000,'ILONLY + 32BITREQUIRED, no prefer32 fallback')
    return {'machine':'I386','optionalHeader':'PE32','subsystem':2,'clrFlags':flags}

def compile_receipt(raw,v,exe,source_bytes):
    r=json.loads(raw);need(r['status']=='PASS_CSC_ACTUAL_EXIT0' and r['actualExit'] is True and r['exitCode']==0,'actual compiler exit0 receipt')
    need(r['sourceBefore']==r['sourceAfter']=={'bytes':len(source_bytes),'sha256':sha(source_bytes)} and r['sourceUnchanged'] is True,'exact generated C# source before/after')
    need(r['exe']=={'bytes':len(exe),'sha256':sha(exe)} and r['nonce']==v['nonce'],'compiler EXE/nonce binding')
    args=r['commandArgs'];need(len(args)==8 and args[1:6]==['/nologo','/target:winexe','/platform:x86','/optimize+','/warnaserror+'] and args[6].startswith('/out:') and args[7].endswith('ChainerProbe.cs'),'closed actual csc command')
    need(args[0].lower().endswith('\\microsoft.net\\framework\\v4.0.30319\\csc.exe') and re.fullmatch('[0-9a-f]{64}',r['compilerSha256']),'existing x86 Framework csc exact identity')
    return pe(exe)

def verify(tables,v):
    need({'Component','Registry','FeatureComponents','Property','Directory','Feature','LaunchCondition','Binary','MsiEmbeddedChainer','InstallExecuteSequence'}<=tables.keys(),'required actual tables')
    need(all(not tables.get(n) for n in FORBIDDEN),'no File/Upgrade/CA or other forbidden effects')
    need(all(not rows for n,rows in tables.items() if n not in ALLOWED|SYSTEM),'unknown nonempty table')
    props={'UpgradeCode':v['upgradeCode'],'MSIRESTARTMANAGERCONTROL':'DisableShutdown','Manufacturer':'FutureClassroom test-only','ProductLanguage':'1033','ProductCode':v['productCode'],'ProductName':NAME,'ProductVersion':'0.0.1'}
    need(sorted(tables['Property'],key=lambda r:r['Property'])==sorted([{'Property':k,'Value':val} for k,val in props.items()],key=lambda r:r['Property']),'exact fixture product/RM properties')
    need(sorted(tables['Directory'],key=lambda r:r['Directory'])==sorted([{'Directory':'TARGETDIR','Directory_Parent':'','DefaultDir':'SourceDir'},{'Directory':'LocalAppDataFolder','Directory_Parent':'TARGETDIR','DefaultDir':'.'}],key=lambda r:r['Directory']),'exact registry component directory only')
    need(tables['Component']==[{'Component':'ChainerRegistryProbe','ComponentId':v['componentCode'],'Directory_':'LocalAppDataFolder','Attributes':'4','Condition':'','KeyPath':'ChainerRegistryMarker'}],'exact HKCU32 RegistryKeyPath4 component')
    need(tables['Registry']==[{'Registry':'ChainerRegistryMarker','Root':'1','Key':v['registryKey'],'Name':v['registryValue'],'Value':'#1','Component_':'ChainerRegistryProbe'}],'only exact owned nonce registry marker')
    need(tables['Feature']==[{'Feature':'ProbeFeature','Feature_Parent':'','Title':'Rollback-only chainer; owned HKCU marker','Description':'','Display':'2','Level':'1','Directory_':'','Attributes':'0'}],'exact attached marker Feature')
    need(tables['FeatureComponents']==[{'Feature_':'ProbeFeature','Component_':'ChainerRegistryProbe'}],'marker attached to actual Feature')
    need(tables['Binary']==[{'Name':'FcIndependentChainer','Data':'Binary.FcIndependentChainer'}],'one exact Binary')
    need(not tables.get('Media') and not tables.get('Error'),'no external cabinet/media or custom error rows')
    need(tables['MsiEmbeddedChainer']==[{'MsiEmbeddedChainer':'FcIndependentChainer','Condition':CONDITION,'CommandLine':v['commandLine'],'Source':'FcIndependentChainer','Type':'2'}],'exact sole rollback chainer/command, x cleanup must not rerun it')
    conditions=[{'Condition':'NOT ALLUSERS','Description':'Independent chainer fixture is per-user only.'},{'Condition':'MSIRESTARTMANAGERCONTROL = "DisableShutdown"','Description':'Independent chainer fixture cannot close applications.'}]
    need(sorted(tables['LaunchCondition'],key=lambda r:r['Condition'])==sorted(conditions,key=lambda r:r['Condition']),'per-user/no shutdown conditions')
    for table,allowed in [('InstallExecuteSequence',EXECUTE)]+list(AUX.items()):
        rows=tables.get(table,[]);need(all(set(r)=={'Action','Condition','Sequence'} for r in rows) and len({r['Action'] for r in rows})==len(rows),'unique standard sequence rows')
        for r in rows:
            condition='NOT Installed' if table=='InstallExecuteSequence' and r['Action']=='AllocateRegistrySpace' else ''
            need(r['Action'] in allowed and r['Sequence']==str(allowed[r['Action']]) and r['Condition']==condition,'no unknown/reordered/conditional actions')
    actions={r['Action'] for r in tables['InstallExecuteSequence']}
    need({'LaunchConditions','CostFinalize','InstallValidate','InstallInitialize','ProcessComponents','RemoveRegistryValues','WriteRegistryValues','RegisterProduct','PublishProduct','InstallFinalize'}<=actions,'normal registry install/removal and registration/rollback sequence must exist')
    return {'status':'PASS_EXACT_STATIC_CHAINER_FIXTURE_ONLY','runtime':'NOT_RUN','productGuardian':'NOT_RUN','registryFixture':{k:v[k] for k in ['componentCode','registryKey','registryValue','registryData','registryView']}}

def main():
    p=argparse.ArgumentParser(description=__doc__);sub=p.add_subparsers(dest='mode',required=True)
    a=sub.add_parser('prepare');a.add_argument('--nonce',required=True);a.add_argument('--windows-report-root',required=True);a.add_argument('--owner-sid',required=True);a.add_argument('--output-dir',type=Path,required=True)
    c=sub.add_parser('check-exports')
    for flag in ['spec','compile-receipt','tables-list','exports','extracted-probe','msi','summary']:c.add_argument('--'+flag,type=Path,required=True)
    for flag in ['spec-sha256','compile-receipt-sha256','msi-sha256']:c.add_argument('--'+flag,required=True)
    args=p.parse_args()
    if args.mode=='prepare':
        v=parameters(args.nonce,args.windows_report_root,args.owner_sid);template=read(Path(__file__).with_name('ChainerProbe.cs.in')).decode('ascii');cs=source(v,template);wxs=authoring(v);chainer=chainer_idt(v)
        v.update(source={'bytes':len(cs),'sha256':sha(cs)},sourceTemplateSha256=sha(template.encode('ascii')),wxsSha256=sha(wxs),chainerIdtSha256=sha(chainer))
        need(not args.output_dir.exists() and not args.output_dir.is_symlink(),'fresh prepare output');args.output_dir.mkdir()
        for name,data in {'.owner':b'future-classroom-owned-r75-chainer-probe-generated/v1\n','ChainerProbe.cs':cs,'probe.wxs':wxs,'MsiEmbeddedChainer.idt':chainer,'spec.json':json_bytes(v)}.items():
            with (args.output_dir/name).open('xb') as f:f.write(data)
        print(json.dumps({'status':'PREPARED_NOT_COMPILED_NOT_BUILT_NOT_RUN','spec':identity(args.output_dir/'spec.json'),'productCode':v['productCode']}));return
    raw=read(args.spec);need(sha(raw)==args.spec_sha256,'explicit specSHA');v=json.loads(raw)
    expected=parameters(v['nonce'],v['windowsReportRoot'],v['expectedOwnerSid']);need(all(v[k]==val for k,val in expected.items()),'spec closed parameters')
    cs=read(args.spec.parent/'ChainerProbe.cs');need({'bytes':len(cs),'sha256':sha(cs)}==v['source'],'source identity')
    template=read(Path(__file__).with_name('ChainerProbe.cs.in'));need(sha(template)==v['sourceTemplateSha256'] and source(v,template.decode('ascii'))==cs,'template/exact fixed constants')
    receipt=read(args.compile_receipt);need(sha(receipt)==args.compile_receipt_sha256,'explicit actual compiler receipt SHA')
    exe=read(args.extracted_probe);pefact=compile_receipt(receipt,v,exe,cs)
    msidata=read(args.msi);need(sha(msidata)==args.msi_sha256,'actual final MSI SHA')
    listed=read(args.tables_list).decode('ascii').splitlines();need(len(listed)==len(set(listed)) and all(re.fullmatch('[A-Za-z_][A-Za-z_0-9]*',n) for n in listed),'complete unique original table listing')
    originals={x.stem:read(x,1024*1024) for x in args.exports.glob('*.idt')}
    need(originals.get('_ForceCodepage')==b'\r\n\r\n0\t_ForceCodepage\r\n\0','exact virtual neutral codepage metadata')
    need(set(originals)==set(listed)-SYSTEM,'all non-system exports incl virtual metadata')
    tables={n:idt(b,n) for n,b in originals.items() if n!='_ForceCodepage'}
    result=verify(tables,v)
    summary=read(args.summary).decode('ascii');need(len(re.findall(r'^Template: Intel;1033\r?$',summary,re.M))==1 and len(re.findall(r'^Version: 500 \(1f4\)\r?$',summary,re.M))==1,'actual x86 MSI500 summary')
    result.update(finalMsi={'bytes':len(msidata),'sha256':sha(msidata)},probe={'bytes':len(exe),'sha256':sha(exe),'pe':pefact},specSha256=sha(raw),compileReceiptSha256=sha(receipt),tableListingSha256=sha(read(args.tables_list)),exportIdentities={n:{'bytes':len(b),'sha256':sha(b)} for n,b in originals.items()},summarySha256=sha(read(args.summary)),scope='Static only; root separately binds exact msibuild/export/extraction command receipts to this same MSI SHA. No execution performed.')
    print(json.dumps(result,indent=2))
if __name__=='__main__':main()
