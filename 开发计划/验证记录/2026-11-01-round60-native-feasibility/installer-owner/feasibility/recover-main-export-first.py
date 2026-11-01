from pathlib import Path
import os,json,hashlib,subprocess,errno,difflib,ast
R=Path('/workspace/共享/Eliora/未来教室计划');E=Path('/tmp/fc-round60-lifecycle-research-evidence/installer-owner/feasibility')
O=E/'recovered-main-export-first';O.mkdir()
def identity(p):
 b=p.read_bytes();st=p.lstat();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'device':st.st_dev,'inode':st.st_ino,'mode':st.st_mode,'mtimeNs':st.st_mtime_ns,'ctimeNs':st.st_ctime_ns}
def save(p,b):
 with p.open('xb') as f:f.write(b)
def command(args,n):
 c=subprocess.run(args,cwd=R,capture_output=True,timeout=30)
 save(O/(n+'.stdout.log'),c.stdout);save(O/(n+'.stderr.log'),c.stderr)
 assert c.returncode==0
 return {'argv':args,'cwd':str(R),'exitCode':c.returncode,'stdout':identity(O/(n+'.stdout.log')),'stderr':identity(O/(n+'.stderr.log'))},c.stdout
p=R/'Binary/Binary.Guardian';d=p.parent
assert not p.is_symlink() and not d.is_symlink()
before=identity(p);refs=[E/'build-fifth/guardian.exe',E/'build-fifth/independent-guardian-stream.stdout.log',E/'build-fifth/Binary/Binary.Guardian']
assert before['bytes']==253577 and before['sha256']=='01c9c1182b1adab40c94f1daa40e617f4d0ca00e513c30a991b8f849fc16415f'
assert all(p.read_bytes()==q.read_bytes() for q in refs)
ls,tracked=command(['git','ls-files','--','Binary'],'tracked-before');assert tracked==b''
statusBefore,_=command(['git','status','--short','--untracked-files=all'],'status-before')
childrenBefore=sorted(q.name for q in d.iterdir());assert childrenBefore==['Binary.Guardian']
oldStat=p.lstat();data=p.read_bytes();dest=O/'Binary.Guardian'
# Original bytes stay in owned evidence; preserve the source metadata in report.
try:
 os.rename(p,dest);method='os.rename to exclusively created owned evidence directory'
except OSError as ex:
 if ex.errno!=errno.EXDEV:raise
 save(dest,data)
 with dest.open('rb') as f:os.fsync(f.fileno())
 assert dest.read_bytes()==data and identity(p)==before
 p.unlink();method='exclusive copy+fsync+byte/SHA recheck then unlink only original exact file (cross-filesystem)'
assert dest.read_bytes()==data and not p.exists()
# Birthtime is unavailable. An empty directory is harmless and is not a Git item;
# do not assert its prior absence or remove a potentially pre-existing empty dir.
childrenAfter=sorted(q.name for q in d.iterdir());assert not childrenAfter
statusAfter,_=command(['git','status','--short','--untracked-files=all'],'status-after')
statusBinary,out=command(['git','status','--short','--untracked-files=all','--','Binary'],'status-binary-after');assert out==b''
original=E/'verify-static.py';fixed=E/'verify-static-cwd-fixed-first.py';source=original.read_text()
needle='cp=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,timeout=120,check=False)'
assert source.count(needle)==1
changed=source.replace(needle,'cp=subprocess.run(argv,cwd=build,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,timeout=120,check=False)')
# New helper, not a rewrite of any frozen source. It is prepared, never executed.
save(fixed,changed.encode());ast.parse(changed)
save(O/'verify-static-cwd-fixed.patch',''.join(difflib.unified_diff(source.splitlines(True),changed.splitlines(True),fromfile='frozen/verify-static.py',tofile='new/verify-static-cwd-fixed-first.py')).encode())
originalReview=json.loads((E/'static-independent-second.json').read_bytes())
row=next(c for c in originalReview['commands'] if c['name']=='table-Binary')
report={'status':'PASS_EXACT_OWN_BINARY_RECOVERED_MAIN_BINARY_STATUS_EMPTY','scope':'Recover only exact R60 exported guardian stream accidentally written in inherited main cwd; no main source or other file mutation.','before':before,'attributionReferences':[identity(q) for q in refs],'exportCommand':row,'rootCause':{'helper':identity(original),'subprocessCwd':'Not specified; inherited main cwd for the independent audit invocation.','sideEffect':'msiinfo export Binary writes the Binary-table embedded stream into relative Binary/Binary.Guardian. Builder already used cwd=out; independent verifier omitted it.','evidenceLimit':'Original command JSON did not record cwd; source omission, exact stream identity and matching final artifact timing support attribution, not a newly rerun tool experiment.'},'move':{'method':method,'destination':identity(dest),'sourceAbsent':not p.exists()},'mainDirectory':{'path':str(d),'childrenBefore':childrenBefore,'childrenAfter':childrenAfter,'removed':False,'reason':'Filesystem birthtime unavailable and no pre-tool directory absence snapshot. Empty directory preserved rather than assert physical NEW ownership.'},'git':{'trackedBefore':ls,'statusBefore':statusBefore,'statusAfter':statusAfter,'binaryStatusAfter':statusBinary,'binaryHasNoTrackedOrUntrackedFiles':True,'wholeStatusScope':'Other current root-owned changes only recorded verbatim, not modified or interpreted by this recovery.'},'futureToolFix':{'newHelper':identity(fixed),'PythonAST':'PASS_SYNTAX_ONLY_NOT_EXECUTED','change':'All independent msiinfo subprocesses now have explicit cwd=build; use only this new helper in a freshly owned output build context. Frozen old helper remains for history, not execution.','oldHelperUnchanged':True,'newPatch':identity(O/'verify-static-cwd-fixed.patch')},'notRun':['Windows','Cargo','MSI tools','Git mutation','main source edits','unknown-file removal']}
save(O/'recovery-first.json',(json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode())
print(json.dumps({'report':identity(O/'recovery-first.json'),'recovered':identity(dest),'directoryEmptyRetained':True,'binaryGitStatusEmpty':True,'newHelper':identity(fixed)},ensure_ascii=False))
