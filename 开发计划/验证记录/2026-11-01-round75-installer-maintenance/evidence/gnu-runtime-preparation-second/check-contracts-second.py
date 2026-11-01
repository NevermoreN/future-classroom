"""Pure binding and historical parser vectors. No target EXE/Windows/runtime."""
from pathlib import Path
from types import SimpleNamespace
import ast,copy,json,runpy
E=Path('/tmp/fc-round75-installer-maintenance/evidence');D=E/'gnu-runtime-preparation-second';F=E/'gnu-runtime-preparation-first'
new=runpy.run_path(str(D/'run-gnu-win11-second.py'));old_parser=runpy.run_path(str(F/'parse-native-children-first.py'));new_parser=runpy.run_path(str(D/'parse-native-children-second.py'))
checks=[]
def check(n,c):assert c,n;checks.append(n)
def reject(n,fn):
 try:fn()
 except AssertionError:checks.append(n)
 else:raise AssertionError('accepted '+n)
for p in D.glob('*.py'):ast.parse(p.read_bytes())
check('Python_AST',True)
old=(F/'parse-native-children-first.py').read_text();current=(D/'parse-native-children-second.py').read_text()
check('native_parser_algorithm_unchanged_only_broker_SHA',old.replace('2812383941139a17169559a902df34724c6cf2deadcd2aaf9961493c31c12691',new['BROKER_SHA'])==current)
# Replay old actual raw vectors only, not new-source/PE runtime evidence.
report=json.loads((E/'root-windows-gnu-full-first.json').read_bytes());producer=json.loads((E/'n-release-second.json').read_bytes());native=next(r for r in producer['testExecutables'] if r['target']=='fc_windows' and r['kind']==['lib']);item=next(r for r in report['tests'] if r['name']==Path(native['path']).name)
guard=dict(path=new['BROKER'],sha256=new['BROKER_SHA']);old_guard=dict(guard,sha256='2812383941139a17169559a902df34724c6cf2deadcd2aaf9961493c31c12691')
a=old_parser['native_harness'](item,native,old_guard);b=new_parser['native_harness'](item,native,guard)
check('historical_original_raw_parser_results_identical',a==b)
check('child_counts_not_promoted_to_parent',len(b['childScopes'])==2 and b['passed']==a['passed'])
wrong=copy.deepcopy(item);wrong['output']=[s for s in wrong['output'] if not s.startswith('running 1 test')]
reject('reject_missing_child_boundaries',lambda:new_parser['native_harness'](wrong,native,guard))
reject('reject_previous_broker_SHA',lambda:new_parser['native_harness'](item,native,old_guard))
# Exercise the actual new admission block using immutable real JSONs, separately
# from overall producer success. Mutation vectors never become producer facts.
command=json.loads((D/'command-second.json').read_bytes())['command'];args={command[i][2:].replace('-','_'):command[i+1] for i in range(3,len(command),2)}
ctx=dict(new);ctx.update(a=SimpleNamespace(**args),producer=json.loads((E/'n-release-third.json').read_bytes()),terminal=json.loads((E/'n-producer-terminal-third.json').read_bytes()),freeze=json.loads((E/'root-n3-codegen-source-freeze-first.json').read_bytes()),joint=json.loads((E/'root-joint-source-freeze-fourth.json').read_bytes()))
ctx['source_rows']={r['path']:r for r in ctx['producer']['sourceBefore']}
src=(D/'run-gnu-win11-second.py').read_text();start=src.index("        need(joint['status']");end=src.index("        for row in joint['freezeReports']",start);block='\n'.join(line[8:] for line in src[start:end].splitlines())
def bind(c):exec(block,c)
bind(dict(ctx));check('actual_F_J_producer_terminal_eight_bindings',True)
for name,key,mutation in [('wrong_F_joint','freeze',lambda v:v.update(jointSha256='0'*64)),('wrong_producer_joint','producer',lambda v:v['jointFreeze'].update(sha256='0'*64)),('missing_implementation_ref','terminal',lambda v:v['implementationFreezes'].pop()),('duplicate_F_source','freeze',lambda v:v['source'].append(v['source'][0])),('unknown_J_source','joint',lambda v:v['sources'][0].update(path='unknown'))]:
 c=dict(ctx);c[key]=copy.deepcopy(ctx[key]);mutation(c[key]);reject('reject_'+name,lambda:bind(c))
check('no_circular_F_in_J_requirement',"JOINT_BINDS_EXACT_N_FREEZE" not in src and "freeze['jointSha256']" in src)
model=D/'nonterminal-model.json';model.write_text('{"status":"RUNNING"}\n')
try:new['read'](model)
except new['Waiting'] as error:check('RUNNING_model_returns_WAIT_not_completion',str(error).startswith('NONTERMINAL_'))
else:raise AssertionError('RUNNING accepted')
check('exact_22_900_native1_public20_contract_retained',"==22" in src and "'900','--native-seconds','1','--system-host','--public-desktop'" in src and "len(steps)==20" in src)
print(json.dumps(dict(status='PASS_PURE_BINDING_AND_HISTORICAL_PARSER_VECTORS_NOT_RUNTIME',count=len(checks),checks=checks,windowsExecuted=False,targetExeExecuted=False),indent=2))
