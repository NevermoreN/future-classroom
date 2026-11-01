"""R75 local-only second observer MSI author/build/IDT/export closure.
No intermediate/final MSI, Windows EXE, observer or production installer is run.
Only new owned local artifact files are changed. Every tool child is pinned.
"""
from pathlib import Path
import ast, datetime, difflib, hashlib, json, os, re, shutil, subprocess, sys, time, xml.etree.ElementTree as ET
sys.dont_write_bytecode=True
R=Path('/tmp/fc-round75-installer-maintenance');E=R/'evidence';W=R/'worktree';D=E/'context-probe-build-second-preparation';T=R/'tools'
P=E/'parent-context-probe-fourth/context-probe.py';CHECKER_SHA='93c87fef4ca04524e47c77c96d200274d2ebb6a3e918977244fea4ecf6bde979'
CLI=E/'gui-cli-list-first.json';CLI_SHA='f823b6e55eee0733b5d32ebeea2bdc2d9b3bfeda66186cdbd749c838a15a0875'
LEDGER=R/'gui-test-fixture-f8bd373d36224240a4aeb4ae49b76f96/fixture-ledger.json';LEDGER_SHA='fa372949bb74b30b540d122af552ebf4c2006e88a6e771ddd7bef76c57ab8799'
PINS={'tools/root/usr/bin/msibuild':'87ee3aa0af25bb997646071a45e35ebae3e3ba5f26fe7b479df97aa31bd8a1eb','tools/root/usr/bin/msiinfo':'50b25845c863756975a8523fe1b370c4bf5dd1812a2a0229cf22139aa4b5ea81','tools/root/usr/bin/wixl':'37fdfb01fb2e3fc752890f1e3eca4a161b5f41e251faa279ded0a9f907d73097','tools/wrappers/wixl':'2ac0b4eebef87eb93298a0de3667ec7ad0e829e94c6e1e52b93518d50a400c60'}
PLACEHOLDER='--fc-r75-unexecuted-type19-placeholder'
ABORT='Owned readonly context probe intentionally stops before InstallInitialize.'
SYSTEM={'_SummaryInformation','_Tables','_Columns','_Validation'}
NS='http://schemas.microsoft.com/wix/2006/wi'
def need(v,c):
    if not v:raise AssertionError(c)
def sha(b):return hashlib.sha256(b).hexdigest()
def fact(path):
    p=Path(path);need(p.resolve().is_relative_to(R) and not p.is_symlink() and p.is_file() and p.suffix.lower() not in {'.key','.pfx','.p12'},'OWNED_REGULAR_NONPRIVATE_INPUT');b=p.read_bytes();return dict(path=str(p),bytes=len(b),sha256=sha(b))
def write(path,data):
    with path.open('xb') as f:f.write(data)
def main():
    need((R/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n' and (D/'.owner').read_bytes()==b'future-classroom-owned-r75-context-probe-build-second-preparation/v1\n','EXACT_OWNERS')
    need(sha(P.read_bytes())==CHECKER_SHA and sha(CLI.read_bytes())==CLI_SHA and sha(LEDGER.read_bytes())==LEDGER_SHA,'EXACT_ORIGINAL_CHECKER_CLI_LEDGER')
    cli=json.loads(CLI.read_bytes());ledger=json.loads(LEDGER.read_bytes());need(cli['status']=='ACTUAL_HARNESS_CLI_PARSE_PASS_NOT_OBSERVER_RUNTIME' and cli['cleanup']=='PASS' and cli['actualExit'] is True and cli['exitCode']==0,'ACTUAL_CLI_SUCCESS_AND_CLEANUP')
    for name,digest in PINS.items():need(fact(R/name)['sha256']==digest,'EXACT_OWNED_TOOL_PIN')
    need(fact(Path(ledger['guiFixture']['path']))==ledger['guiFixture'] and ledger['guiFixture']['sha256']==cli['observerSha256'],'SAME_ACTUAL_GUI_CLONE')
    first=E/'root-build-context-probe-first.json';first_report=json.loads(first.read_bytes());firstdir=Path(first_report['directory'])
    originals=[first,E/'root-build-context-probe-first.py',*[p for p in E.glob('context-probe-*-first.*.log')],*[p for p in firstdir.rglob('*') if p.is_file()]]
    originals += [p for p in D.rglob('*') if p.is_file() and p.name not in {'build-export-second.py'}]
    original_before={str(p):fact(p) for p in originals}
    joint_path=E/'root-joint-source-freeze-second.json';joint=json.loads(joint_path.read_bytes())
    def guards():
        rows=[]
        for row in joint['sources']:
            current=fact(W/row['path']);value=dict(path=row['path'],bytes=current['bytes'],sha256=current['sha256']);need(value==row,'FROZEN_SOURCE_GUARD');rows.append(value)
        return rows
    before=guards();report=D/'build-export-second.json';need(not report.exists(),'EXCLUSIVE_NEW_BUILD_RECEIPT')
    value=dict(status='RUNNING',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),originalPerCommandBudgetSeconds=20,originalOverallBudgetSeconds=120,sourceBefore=before,originalInputs=original_before,tools=[fact(R/n) for n in PINS],fourthChecker=fact(P),actualCli=fact(CLI),fixtureLedger=fact(LEDGER),commands=[],scope='Local test-only observer author/build/static exports only. Neither placeholder nor final MSI or any EXE/observer is executed. No Windows/production gate/Pending/main changes.')
    child_env=os.environ.copy()
    for key in ('LD_PRELOAD','LD_AUDIT','GIO_EXTRA_MODULES','GIO_MODULE_DIR'):child_env.pop(key,None)
    child_env.update(LD_LIBRARY_PATH=str(T/'root/usr/lib/x86_64-linux-gnu'),LC_ALL='C',GIO_USE_VFS='local',GSETTINGS_BACKEND='memory')
    began=time.monotonic()
    def save():report.write_text(json.dumps(value,indent=2)+'\n')
    def run(name,cmd,cwd=D,binary=None):
        need(time.monotonic()-began<120,'ORIGINAL_TOTAL120S_DEADLINE');started=time.monotonic();code=None;out=b'';err=b'';timeout=False
        try:actual=subprocess.run(cmd,cwd=cwd,env=child_env,capture_output=True,timeout=min(20,max(0.01,120-(time.monotonic()-began))));code=actual.returncode;out=actual.stdout;err=actual.stderr
        except subprocess.TimeoutExpired as error:out=error.stdout or b'';err=error.stderr or b'';timeout=True
        row=dict(name=name,command=cmd,cwd=str(cwd),actualExit=code is not None,exitCode=code,timedOut=timeout,elapsedSeconds=time.monotonic()-started)
        for key,data in (('stdout',out),('stderr',err)):
            path=binary if key=='stdout' and binary is not None else D/(name+'-second.'+key+'.log');write(path,data);row[key]=fact(path)
        value['commands'].append(row);save();need(code==0 and not timeout,'ACTUAL_EXIT0_'+name);return out
    directory=D/'authored-third';base=['python3','-B',str(P),'--observer-test-name',cli['selectedTests'][0],'--windows-report-root',cli['remoteRoot'],'--nonce',cli['nonce']]
    bindings=['--observer-sha256',ledger['guiFixture']['sha256'],'--fixture-ledger',str(LEDGER),'--fixture-ledger-sha256',LEDGER_SHA]
    save()
    try:
        run('01-author',base+['author','--observer-exe',ledger['guiFixture']['path'],*bindings,'--actual-cli-proof',str(CLI),'--cli-proof-sha256',CLI_SHA,'--output-dir',str(directory)])
        original_wxs=(directory/'context-probe.wxs').read_bytes();tree=ET.fromstring(original_wxs);abort=tree.findall('.//{'+NS+'}CustomAction');abort=[n for n in abort if n.attrib.get('Id')=='FcAbortBeforeInitialize'];need(len(abort)==1 and abort[0].attrib==dict(Id='FcAbortBeforeInitialize',Error=ABORT),'EXACT_ORIGINAL_TYPE19_AUTHOR_NODE')
        abort[0].attrib.clear();abort[0].attrib.update(Id='FcAbortBeforeInitialize',BinaryKey='FcReadonlyObserver',ExeCommand=PLACEHOLDER,Return='check',Impersonate='yes')
        ET.register_namespace('',NS);placeholder=ET.tostring(tree,encoding='utf-8',xml_declaration=True)+b'\n';write(directory/'context-probe-placeholder.wxs',placeholder)
        expected=ET.fromstring(placeholder);node=[n for n in expected.findall('.//{'+NS+'}CustomAction') if n.attrib['Id']=='FcAbortBeforeInitialize'][0];node.attrib.clear();node.attrib.update(Id='FcAbortBeforeInitialize',Error=ABORT)
        need(ET.tostring(expected)==ET.tostring(ET.fromstring(original_wxs)),'ONLY_ABORT_AUTHOR_NODE_DIFFERENCE')
        write(D/'original-to-placeholder-second.diff',''.join(difflib.unified_diff(original_wxs.decode().splitlines(True),placeholder.decode().splitlines(True),fromfile='original-author.wxs',tofile='placeholder-build.wxs')).encode())
        value['authoringChange']=dict(original=fact(directory/'context-probe.wxs'),placeholder=fact(directory/'context-probe-placeholder.wxs'),onlyAbortAttributesChanged=True,intermediateExecution='NEVER_RUN')
        run('02-build-placeholder',[str(T/'wrappers/wixl'),'-a','x86','-o','context-probe-placeholder.msi','context-probe-placeholder.wxs'],directory)
        intermediate=directory/'context-probe-placeholder.msi';final=directory/'context-probe-final.msi';write(final,intermediate.read_bytes());value['intermediateMsi']=fact(intermediate)
        info=str(T/'root/usr/bin/msiinfo');build=str(T/'root/usr/bin/msibuild')
        def export_set(stem,msi):
            raw=run(stem+'-tables',[info,'tables',str(msi)]);names=raw.decode('ascii').splitlines();need(1<=len(names)<=64 and len(names)==len(set(names)) and all(re.fullmatch('[A-Za-z_][A-Za-z_0-9]*',n) for n in names),'ACTUAL_COMPLETE_CLOSED_TABLE_NAMES')
            folder=D/(stem+'-second-exports');folder.mkdir();exports={}
            for name in names:
                if name in SYSTEM:continue
                data=run(stem+'-export-'+name,[info,'export',str(msi),name]);write(folder/(name+'.idt'),data);exports[name]=data
            summary=run(stem+'-summary',[info,'suminfo',str(msi)])
            return dict(names=names,rawList=D/(stem+'-tables-second.stdout.log'),folder=folder,exports=exports,summarySha256=sha(summary))
        initial=export_set('03-placeholder',intermediate);ca=initial['exports']['CustomAction'];lines=ca.decode('ascii').splitlines();need(lines[:3]==['Action\tType\tSource\tTarget\tExtendedType','s72\ti2\tS72\tS255\tI4','CustomAction\tAction'],'ACTUAL_WIXL_EMPTY_FIVE_COLUMN_CUSTOMACTION_SCHEMA')
        rows=[dict(zip(lines[0].split('\t'),line.split('\t'))) for line in lines[3:]];command=cli['actualListCommandLine'];need(cli['commandArgs'][-1]=='--list','ORIGINAL_LIST_CONTROL');wanted=subprocess.list2cmdline(cli['commandArgs'][:-1]);need(rows==[],'ACTUAL_WIXL_UNSUPPORTED_BINARYKEY_EMITS_NO_CA_ROWS');value['intermediateCustomActionQualification']=dict(columns=lines[:3],rows=rows,wixlExit0NotSemanticCAApproval=True,warning='Pinned wixl emits SourceNULL warnings for both authored BinaryKey/ExeCommand nodes; complete final IDT creates the intended two real table rows.')
        final_rows=[['FcObserveParent','2','FcReadonlyObserver',wanted],['FcAbortBeforeInitialize','19','',ABORT]]
        headers=['Action\tType\tSource\tTarget','s72\ti2\tS72\tS255','CustomAction\tAction'];idt=('\r\n'.join(headers+['\t'.join(row) for row in final_rows])+'\r\n').encode('ascii');patch=D/'CustomAction-final-second.idt';write(patch,idt)
        run('04-replace-customaction',[build,str(final),'-q','DROP TABLE `CustomAction`','-i',str(patch)],directory)
        finished=export_set('05-final',final);need(set(initial['names'])==set(finished['names']),'UNCHANGED_COMPLETE_TABLE_SET')
        changed=[name for name in initial['exports'] if initial['exports'][name]!=finished['exports'][name]];need(changed==['CustomAction'],'ONLY_CUSTOMACTION_EXPORT_DIFFERENCE')
        extracted=directory/'extracted-observer.exe';run('06-extract-binary',[info,'extract',str(final),'Binary.FcReadonlyObserver'],directory,binary=extracted);need(fact(extracted)['sha256']==ledger['guiFixture']['sha256'] and extracted.read_bytes()==Path(ledger['guiFixture']['path']).read_bytes(),'FINAL_BINARY_EXACT_GUI_TEST_CLONE')
        checked=run('07-fourth-checker',base+['check-exports','--exports',str(finished['folder']),'--tables-list',str(finished['rawList']),'--extracted-observer',str(extracted),*bindings],directory)
        result=json.loads(checked);need(result['status']=='PASS_EXPORTED_PROBE_TOPOLOGY_ONLY','UNCHANGED_FOURTH_CHECKER_ACCEPTS_ACTUAL_FINAL')
        value.update(status='PASS_LOCAL_CONTEXT_TYPE19_IDT_BUILD_EXPORT_ONLY_NOT_WINDOWS',msi=fact(final),tableCheck=result,completeTableNames=finished['names'],allOrdinaryAndVirtualExports=[fact(p) for p in sorted(finished['folder'].iterdir())],exportDifference=dict(changedTables=changed,allOtherTableExportsByteIdentical=True,originalFiveColumnEmptyCustomActionReplacedByExactFourColumnTwoRows=True,intermediateSummarySha256=initial['summarySha256'],finalSummarySha256=finished['summarySha256']),finalCustomAction=final_rows,importedIdt=fact(patch),extractedBinary=fact(extracted),firstFailurePreserved=dict(report=fact(first),reason='Original actual wixl0.103 rejected CustomAction Error property and SourceNULL. The separate first local second-attempt failure additionally preserved actual wixl SourceNULL/empty five-column CustomAction. This new attempt stages unexecuted authored Type2 placeholders, then drops the tool empty CustomAction schema and imports the complete intended four-column Type2/Type19 table. No source or original checker/author changes.'),sourceAfter=guards())
        need(value['sourceAfter']==before and original_before=={str(p):fact(p) for p in originals},'FROZEN_SOURCE_AND_ALL_FIRST_ORIGINALS_UNCHANGED')
        need(all(fact(R/n)['sha256']==digest for n,digest in PINS.items()) and sha(P.read_bytes())==CHECKER_SHA and sha(CLI.read_bytes())==CLI_SHA and sha(LEDGER.read_bytes())==LEDGER_SHA,'TOOLS_AND_AUTHOR_CLI_LEDGER_UNCHANGED')
        value['originalsUnchanged']=True
    except Exception as error:value.update(status='FAIL_LOCAL_SECOND_BUILD_OR_EXPORTS',error=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'LOCAL_TOOL_OR_INPUT_FAILURE'))
    finally:value['elapsedSeconds']=time.monotonic()-began;value['completedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat();save()
    print(json.dumps(dict(status=value['status'],msi=value.get('msi'),elapsedSeconds=value['elapsedSeconds'],commandCount=len(value['commands']),report=fact(report),error=value.get('error'))));return 0 if value['status']=='PASS_LOCAL_CONTEXT_TYPE19_IDT_BUILD_EXPORT_ONLY_NOT_WINDOWS' else 1
if __name__=='__main__':raise SystemExit(main())
