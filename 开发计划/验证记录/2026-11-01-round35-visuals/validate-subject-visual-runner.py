#!/usr/bin/env python3
"""Pure transcript/CLI/string-generation checks only: never Remote/SSH/Windows."""
from pathlib import Path
import ast,base64,contextlib,copy,hashlib,io,json,re,sys
root=Path('/tmp/fc-round35-visuals-worktree');ev=Path('/tmp/fc-round35-visuals-evidence');source=(root/'scripts/windows-probe.py').read_text();before=base64.b64decode(json.loads((ev/'subject-visual-runner-before.json').read_text())['base64']).decode()
ns={'__name__':'native_probe_validator_only'};exec(compile(source,'scripts/windows-probe.py','exec'),ns)
cases=[]
def check(name,fn):
    fn();cases.append({'name':name,'status':'PASS'})
def need(value):
    if not value:raise AssertionError('check failed')
steps=[f'SUBJECT_VISUAL_NATIVE_STEP index={i} name={name}' for i,name in enumerate(ns['SUBJECT_VISUAL_STEPS'])]
footer='SUBJECT_VISUAL_NATIVE_PASS steps=22 shared_workers=2 real_sqlite_reopen=PASS joint_save=PASS stale_rejected=PASS synthetic_d2d=PASS colour_dialog=NOT_OPENED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms=1234'
valid=[*steps,footer];validate=ns['validate_subject_visual_output']
check('valid-exact22-final-marker',lambda:need(validate(0,valid)['steps']==22))
check('valid-original120s-boundary',lambda:need(validate(0,[*steps,footer.replace('1234','120000')])['elapsedMs']==120000))
def reject(code,lines):
    try:validate(code,lines)
    except RuntimeError:return
    raise AssertionError('invalid transcript accepted')
for name,code,lines in [
('nonzero-exit',1,valid),('bool-exit-is-not-int',False,valid),('not-list',0,{}),('nonstring-output',0,[*steps,None,footer]),
('missing-step',0,[*steps[:-1],footer]),('repeated-step',0,[*steps,steps[-1],footer]),('wrong-index',0,[steps[0].replace('index=0','index=1'),*steps[1:],footer]),('wrong-name',0,[steps[0]+'-wrong',*steps[1:],footer]),('out-of-order',0,[steps[1],steps[0],*steps[2:],footer]),('malformed-reserved-step',0,[*steps,'SUBJECT_VISUAL_NATIVE_STEP malformed',footer]),('missing-footer',0,steps),('duplicate-footer',0,[*steps,footer,footer]),('footer-before-last-step',0,[*steps[:-1],footer,steps[-1]]),('cleanup-not-pass',0,[*steps,footer.replace('isolated_cleanup=PASS','isolated_cleanup=FAIL')]),('wrong-workers',0,[*steps,footer.replace('shared_workers=2','shared_workers=3')]),('claimed-visible-dialog',0,[*steps,footer.replace('colour_dialog=NOT_OPENED','colour_dialog=PASS')]),('noncanonical-footer-tail',0,[*steps,footer+' extra=true']),('over120s',0,[*steps,footer.replace('1234','120001')]),('zero-elapsed',0,[*steps,footer.replace('1234','0')]),('negative-elapsed',0,[*steps,footer.replace('1234','-1')])]:check(name,lambda c=code,l=lines:reject(c,l))
module=ast.parse(source);old_module=ast.parse(before)
def function(tree,name):return next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name==name)
for name in ('validate_voice_output_output','validate_voice_settings_output','validate_migration_output'):
    check('unchanged-original-'+name,lambda name=name:need(ast.dump(function(module,name),include_attributes=False)==ast.dump(function(old_module,name),include_attributes=False)))
# Use exactly the parser and initial argument-only validation statements. Stop
# before host/file checks and before any Remote or filesystem method is reached.
def parser_from(tree):
    f=copy.deepcopy(function(tree,'main'));f.name='parse_only';kept=[]
    for node in f.body:
        if isinstance(node,ast.If) and 'args.host.startswith' in ast.unparse(node.test):break
        kept.append(node)
    f.body=kept+[ast.Return(ast.Name(id='args',ctx=ast.Load()))];program=ast.fix_missing_locations(ast.Module(body=[f],type_ignores=[]));scope=dict(ns);exec(compile(program,'parser-prefix-only','exec'),scope);return scope['parse_only']
parser=parser_from(module)
base=['windows-probe.py','--host','test@invalid.local','--identity','/never-read/key','--exe','/never-read/probe.exe','--report','/never-write/report.json']
def parse(flags):
    previous=sys.argv;sys.argv=base+flags
    try:
        with contextlib.redirect_stderr(io.StringIO()):return parser()
    finally:sys.argv=previous
legacy=['--voice-output','--voice-settings','--calendar-download','--application','--calendar','--autostart','--recovery','--reinitialization','--migration','--materials','--audio-silent','--web-voice','--web-pairing']
for flag in legacy:
    def rejected(flag=flag):
        try:parse(['--subject-visuals',flag])
        except SystemExit as error:need(error.code==2);return
        raise AssertionError('mutually exclusive workflows accepted')
    check('cli-mutual-'+flag,rejected)
check('cli-subject-only',lambda:need(parse(['--subject-visuals']).subject_visuals))
check('cli-subject-with-readonly-host-and-metadata',lambda:need(parse(['--subject-visuals','--system-host','--test-metadata','/never-read/tests.jsonl']).system_host))
# Execute only the pure PowerShell string builder statements, stopping before
# encoding/upload. Compare legacy builder bytes for every original workflow.
def builder_from(tree):
    main=function(tree,'main');body=[];started=False
    for node in main.body:
        # Native command generation is inside main's original try body.
        if isinstance(node,ast.Try):
            for n in node.body:
                text=ast.unparse(n)
                if isinstance(n,ast.Assign) and text.startswith('body = child_temp'):started=True
                if started and isinstance(n,ast.Assign) and text.startswith("body = '[Console]"):break
                if started:body.append(copy.deepcopy(n))
            if started:break
    need(bool(body));f=ast.FunctionDef(name='build',args=ast.arguments(posonlyargs=[],args=[ast.arg(arg='args'),ast.arg(arg='directory'),ast.arg(arg='child_temp')],kwonlyargs=[],kw_defaults=[],defaults=[]),body=body+[ast.Return(ast.Name(id='body',ctx=ast.Load()))],decorator_list=[]);scope=dict(ns);exec(compile(ast.fix_missing_locations(ast.Module(body=[f],type_ignores=[])),'ps-string-only','exec'),scope);return scope['build']
newbuild=builder_from(module);oldbuild=builder_from(old_module)
for flags in [[],['--system-host'],*[[f] for f in legacy]]:
    args=parse(flags)
    check('unchanged-legacy-native-script-'+('/'.join(flags) or 'default'),lambda a=args:need(newbuild(a,'C:\\Synthetic\\probe','')==oldbuild(a,'C:\\Synthetic\\probe','')))
for flags in [['--subject-visuals'],['--subject-visuals','--system-host']]:
    body=newbuild(parse(flags),'C:\\Synthetic\\probe','')
    check('new-task-command-'+str(len(flags)),lambda b=body:need(b.count(' --subject-visuals-probe ')==1 and b.count('subjectVisualExitCode=$subjectVisualCode;')==1 and b.count('subjectVisualOutput=$subjectVisualOutput;')==1))
# Native ORDER is exact byte string names, not a guessed copied task count.
native=(root/'crates/fc-bootstrap/src/application/subject_visual_probe.rs').read_text();order=re.findall(r'"([a-z-]+)"',native[native.index('const ORDER:'):native.index('type Check')]);check('actual-rust-order-matches-python',lambda:need(order==list(ns['SUBJECT_VISUAL_STEPS'])))
report={'status':'PASS','scope':'Host pure transcript validators, initial CLI parser prefix and PowerShell string builder only; NO Remote/SSH/Windows/EXE execution','sourceSha256':hashlib.sha256(source.encode()).hexdigest(),'sourceBeforeSha256':hashlib.sha256(before.encode()).hexdigest(),'cases':cases,'caseCount':len(cases),'nativeFeatureBudgetSeconds':120,'newTaskObservationSeconds':180,'newRemoteObservationSeconds':190,'originalBudgets':'unchanged240/300 task and250/310 remote; source comparison and byte-identical legacy native scripts checked','evidenceLimit':'These synthetic validators are not actual native feature/probe execution.'}
out=ev/'subject-visual-runner-local-validation-first.json';assert not out.exists();out.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'status':report['status'],'cases':len(cases),'report':str(out),'sourceSha256':report['sourceSha256']},indent=2))
