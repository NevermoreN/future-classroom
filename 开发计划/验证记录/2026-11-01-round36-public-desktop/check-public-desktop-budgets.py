from pathlib import Path
import ast,base64,copy,hashlib,json
r=Path('/tmp/fc-round36-public-desktop-worktree');ev=Path('/tmp/fc-round36-public-desktop-evidence');source=(r/'scripts/windows-probe.py').read_text();before=(ev/'editor-runner-before.py').read_text();ns={'__name__':'budget_pure'};exec(compile(source,'runner-source','exec'),ns)
# Build only the task command/deadline strings and return the remote timeout
# expression, without executing the surrounding Remote.powershell call.
def budget_from(text):
    tree=ast.parse(text);main=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='main');selected=[];timeout=None
    for n in main.body:
        if isinstance(n,ast.Try):
            begun=False
            for statement in n.body:
                code=ast.unparse(statement)
                if isinstance(statement,ast.Assign) and code.startswith('launch = '):begun=True
                if begun and isinstance(statement,ast.Assign) and code.startswith("report['native'] = "):
                    timeout=next(k.value for call in ast.walk(statement) if isinstance(call,ast.Call) and isinstance(call.func,ast.Attribute) and call.func.attr=='powershell' for k in call.keywords if k.arg=='timeout');break
                if begun:selected.append(copy.deepcopy(statement))
            if begun:break
    assert selected and timeout is not None
    f=ast.FunctionDef(name='budget',args=ast.arguments(posonlyargs=[],args=[ast.arg(arg='args')],kwonlyargs=[],kw_defaults=[],defaults=[]),body=selected+[ast.Return(ast.Tuple(elts=[ast.Name(id='launch',ctx=ast.Load()),timeout],ctx=ast.Load()))],decorator_list=[]);scope=dict(ns,arguments='owned-args',task_name="'owned-task'",root="'owned-root'");exec(compile(ast.fix_missing_locations(ast.Module(body=[f],type_ignores=[])),'budget-only','exec'),scope);return scope['budget']
new=budget_from(source);old=budget_from(before)
from types import SimpleNamespace
names=['public_desktop','subject_visuals','voice_output','voice_settings','application','materials','web_voice','calendar','autostart','recovery','reinitialization','migration']
def args(flag=None):return SimpleNamespace(**{n:n==flag for n in names})
cases=[]
for flag in [None,*names[1:]]:
    assert new(args(flag))==old(args(flag));cases.append({'flag':flag or 'default','status':'PASS','remoteTimeout':new(args(flag))[1]})
launch,timeout=new(args('public_desktop'));assert 'AddSeconds(180)' in launch and 'AddSeconds(40)' not in launch and timeout==190;cases.append({'flag':'public_desktop','status':'PASS','taskSeconds':180,'remoteSeconds':190})
report={'status':'PASS','scope':'pure AST-extracted task string and timeout evaluation only; no Remote/SSH/Windows','sourceSha256':hashlib.sha256(source.encode()).hexdigest(),'caseCount':len(cases),'cases':cases};q=ev/'public-desktop-runner-budget-validation-first.json';assert not q.exists();q.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'status':'PASS','cases':len(cases)},indent=2))
