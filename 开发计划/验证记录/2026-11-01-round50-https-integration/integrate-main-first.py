from pathlib import Path
import json,subprocess,hashlib
main=Path('/workspace/共享/Eliora/未来教室计划');work=Path('/tmp/fc-round47-https-settings-worktree');ev=Path('/tmp/fc-round50-https-integration-evidence');diag=Path('/tmp/fc-round51-browser-diagnostics-evidence');base='e23bd538d828aeed4378a88f69cf81cb85a4389f'
def sha(b):return hashlib.sha256(b).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=main,text=True).strip()==base
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=work,text=True).strip()==base
tracked=subprocess.check_output(['git','diff','--name-only','-z','--','crates'],cwd=work).decode().split('\0');new=subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z','--','crates'],cwd=work).decode().split('\0');names=sorted(n for n in set(tracked+new) if n);assert len(names)==33 and all(n.endswith('.rs') for n in names)
source=json.loads((ev/'full-matrix-second.json').read_text())['sourceBefore'];report=ev/'main-source-integration-first.json';assert not report.exists();rows=[]
for n in names:
 p=work/n;b=p.read_bytes();assert {'bytes':len(b),'sha256':sha(b)}==source[n];q=main/n
 if n in tracked:assert q.read_bytes()==subprocess.check_output(['git','show',base+':'+n],cwd=main)
 else:assert not q.exists()
 rows.append({'path':n,'bytes':len(b),'sha256':sha(b),'role':'product-rust'})
f=diag/'implementation-freeze-second.json';assert sha(f.read_bytes())=='38555fe4cdcab3bdb34dcafe41f8249aab94d36d7dbc199ae3a81bff3ae74530';d=json.loads(f.read_text())
first=json.loads((diag/'implementation-freeze-first.json').read_text());before={r['path']:r['before'] for r in first['ownedFiles']}
for row in d['ownedFiles']:
 n=row['path'];b=(Path(d['snapshot'])/n).read_bytes();assert len(b)==row['bytes'] and sha(b)==row['sha256'];q=main/n
 if before[n] is None:assert not q.exists()
 else:assert sha(q.read_bytes())==before[n]['sha256']
 rows.append({**{k:row[k] for k in ('path','bytes','sha256')},'role':'test-diagnostics'})
assert len(rows)==36
for row in rows:
 q=main/row['path'];q.parent.mkdir(parents=True,exist_ok=True);b=(work/row['path']).read_bytes() if row['role']=='product-rust' else (Path(d['snapshot'])/row['path']).read_bytes();q.write_bytes(b);assert sha(q.read_bytes())==row['sha256']
for n,fact in source.items():assert {'bytes':(main/n).stat().st_size,'sha256':sha((main/n).read_bytes())}==fact
subprocess.run(['python3','scripts/check-architecture.py'],cwd=main,check=True,capture_output=True)
subprocess.run(['git','diff','--check','--',*names,*[x['path'] for x in d['ownedFiles']]],cwd=main,check=True,capture_output=True)
r={'status':'PASS_EXACT_SOURCE_INTEGRATION_HOST_FINAL_PENDING','mainBase':base,'candidateRoot':str(work),'rustBuildInputs':len(source),'allRustInputsMatchCandidate':True,'diagnosticFreezeSha256':sha(f.read_bytes()),'files':rows,'scope':'Only exact final33 Rust source and frozen R51second3 tests copied. Existing four plan edits and R46evidence retained. Architecture12/33 and source whitespace PASS; no Cargo/Windows rerun, no commit/push. Final Host test+strict still running and must complete before code promotion.'}
report.write_text(json.dumps(r,indent=2)+'\n');print('INTEGRATED_EXACT33_RUST_PLUS3_TESTFILES_SOURCE429_MATCH')
