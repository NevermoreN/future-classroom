from pathlib import Path
import hashlib,json,shutil,subprocess,time,sys
root=Path.cwd()/'web';scratch=Path('/tmp/fc-round30-web-upgrade');ev=Path('/tmp/fc-round30-evidence');prefix=sys.argv[1]
def snapshot():return {str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(root.rglob('*')) if p.is_file() and not any(x in {'node_modules','dist'} for x in p.relative_to(root).parts) and p.name!='build-budget.json'}
before=snapshot();shutil.copytree(root,scratch,dirs_exist_ok=True,ignore=shutil.ignore_patterns('node_modules','dist','build-budget.json'))
for rel,sha in before.items():assert hashlib.sha256((scratch/rel).read_bytes()).hexdigest()==sha
commands=[['npm','run','typecheck'],['npm','test','--','--maxWorkers=2','--reporter=json','--outputFile='+str(ev/(prefix+'-vitest.json'))],['npm','run','build']];results=[]
for i,cmd in enumerate(commands):
 start=time.monotonic();log=ev/(prefix+f'-{i}.log')
 with log.open('xb') as out:r=subprocess.run(cmd,cwd=scratch,stdout=out,stderr=subprocess.STDOUT)
 results.append({'command':cmd,'exitCode':r.returncode,'elapsedSeconds':time.monotonic()-start,'log':str(log)})
 print(json.dumps(results[-1]),flush=True)
 if r.returncode:break
after=snapshot();report={'scope':'Frozen repository web sources copied to owned npm scratch; no repo node_modules/dist','commands':results,'sourceBefore':before,'sourceAfter':after,'changedInputs':[p for p in set(before)|set(after) if before.get(p)!=after.get(p)],'scratch':str(scratch),'generatedNoticesMatch':(scratch/'THIRD-PARTY-NOTICES.txt').read_bytes()==(root/'THIRD-PARTY-NOTICES.txt').read_bytes()}
if (scratch/'dist/fc-assets.json').exists():
 report['manifest']=json.loads((scratch/'dist/fc-assets.json').read_text());report['buildBudget']=json.loads((scratch/'build-budget.json').read_text())
(ev/(prefix+'.json')).write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');raise SystemExit(0 if len(results)==3 and all(x['exitCode']==0 for x in results) and not report['changedInputs'] and report['generatedNoticesMatch'] else 1)
