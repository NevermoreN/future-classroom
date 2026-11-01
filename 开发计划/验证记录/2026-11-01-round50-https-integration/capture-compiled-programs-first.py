from pathlib import Path
import json,hashlib,shutil,subprocess
r=Path('/tmp/fc-round47-https-settings-worktree');e=Path('/tmp/fc-round50-https-integration-evidence');build=e/'full-matrix-second.json';d=json.loads(build.read_text());done={x['name']:x for x in d['commands']}
for name in ('windows-compile','windows-product','windows-strict'):assert done[name]['exitCode']==0 and done[name]['actualExit'] and not done[name]['timedOut']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for name,value in d['sourceBefore'].items():assert (r/name).stat().st_size==value['bytes'] and sha(r/name)==value['sha256']
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()==d['gitHeadBefore']
p=Path('/tmp/fc-round50-https-programs-preliminary');assert not p.exists();p.mkdir();(p/'.owner').write_text('future-classroom-owned-r50-https-programs-preliminary/v1\n');rows=[];metadata=[]
for stage in ('windows-compile','windows-product'):
 log=Path(done[stage]['stdout']['path']);assert sha(log)==done[stage]['stdout']['sha256']
 for line in log.read_text().splitlines():
  try:x=json.loads(line)
  except ValueError:continue
  if x.get('reason')!='compiler-artifact' or not x.get('executable'):continue
  test=x.get('profile',{}).get('test',False)
  if stage=='windows-compile' and not test:continue
  if stage=='windows-product' and (test or x['target']['name']!='fc-bootstrap'):continue
  original=Path(x['executable']);assert original.suffix=='.exe';dest=p/original.name;assert not dest.exists();shutil.copyfile(original,dest);assert sha(original)==sha(dest)
  rows.append({'path':str(dest),'originalPath':str(original),'bytes':dest.stat().st_size,'sha256':sha(dest),'test':test,'target':x['target']})
  if test:x['executable']=str(dest);metadata.append(x)
assert len(rows)==22 and len(metadata)==21
with (e/'windows-test-metadata-preliminary.jsonl').open('x') as out:
 for item in metadata:out.write(json.dumps(item)+'\n')
result={'status':'PASS_COMPLETED_GNU_COMPILE_PRODUCT_STABLE_STRICT_ONLY','scope':'Frozen source429;22exact copies; matrix nightly/Host still pending. For actual supplemental runtime while independent Host commands run; does not promote whole source, production or MSVC.','sourceBefore':d['sourceBefore'],'buildReportCapturedBytesSha256':{'bytes':build.stat().st_size,'sha256':sha(build)},'artifacts':rows,'metadata':{'path':str(e/'windows-test-metadata-preliminary.jsonl'),'sha256':sha(e/'windows-test-metadata-preliminary.jsonl')},'completedCommands':[done[n] for n in ('windows-compile','windows-product','windows-strict')]}
q=e/'compiled-programs-preliminary-first.json';assert not q.exists();q.write_text(json.dumps(result,indent=2)+'\n');print('CAPTURED_EXACT22_COMPLETED_COMPILE_NORMAL_AND_STABLE_STRICT')
