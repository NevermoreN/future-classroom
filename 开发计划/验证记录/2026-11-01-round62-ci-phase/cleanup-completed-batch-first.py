from pathlib import Path
import hashlib, json, os, shutil, stat, subprocess, time

EV = Path(__file__).resolve().parent
ROOT = Path.cwd()
proof_path = EV/'committed-bytes-first.json'
assert hashlib.sha256(proof_path.read_bytes()).hexdigest() == 'e981b8deb9eba6c8af58da0757ef6cebed9a7820afbf19ae8fc32e52c8d1909b'
proof = json.loads(proof_path.read_text())
assert proof['status']=='PASS_ALL_COMMITTED_ORIGINAL_BYTES'
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==proof['sourceCommit']
assert not subprocess.check_output(['git','status','--porcelain'])
roots = [
    (Path('/tmp/fc-round59-ci-evidence'),'future-classroom-owned-r59-ci-evidence/v1\n'),
    (Path('/tmp/fc-round61-gui'),'future-classroom-owned-r61-gui/v1\n')]
for path,marker in roots:
    assert path.is_dir() and not path.is_symlink() and (path/'.owner').read_text()==marker
tree = roots[1][0]/'worktree'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=tree,text=True).strip()=='6082dd0173a0b8884c27ac78fda5054275f99f14'
assert subprocess.check_output(['git','rev-parse','refs/heads/codex/gui-startup'],text=True).strip()=='6082dd0173a0b8884c27ac78fda5054275f99f14'
source = json.loads((roots[1][0]/'evidence/main-integration-first.json').read_text())['files']
for row in source:
    for base in [tree,ROOT]:
        data=(base/row['path']).read_bytes()
        assert len(data)==row['bytes'] and hashlib.sha256(data).hexdigest()==row['sha256']
# All root-owned exec sessions and agents have reported actual completion. This
# extra check refuses only relevant live compiler/probe consumers; it never
# terminates unrelated processes or requires inspecting system processes' cwd.
processes = subprocess.check_output(['ps','-eo','comm=,args='],text=True).splitlines()
for line in processes:
    fields=line.split(None,1)
    if fields and fields[0] in {'cargo','rustc','rustfmt'}:
        assert not any(str(path) in line for path,_ in roots), line
counts=[]
for path,_ in roots:
    total=0;files=0
    for folder,dirs,names in os.walk(path,followlinks=False):
        for name in names:
            p=Path(folder)/name
            if stat.S_ISREG(p.lstat().st_mode): total+=p.stat().st_size;files+=1
    counts.append(dict(path=str(path),logicalBytes=total,regularFiles=files))
started=time.monotonic()
r=subprocess.run(['git','worktree','remove','--force',str(tree)],capture_output=True)
assert r.returncode==0, r.stderr.decode(errors='replace')
r=subprocess.run(['git','branch','-D','codex/gui-startup'],capture_output=True)
assert r.returncode==0, r.stderr.decode(errors='replace')
for path,_ in roots:
    shutil.rmtree(path)
    assert not path.exists()
assert not subprocess.check_output(['git','status','--porcelain'])
result=dict(status='PASS_COMMITTED_R59_R61_OWNED_ROOTS_REMOVED',sourceCommit=proof['sourceCommit'],
            proofSha256=hashlib.sha256(proof_path.read_bytes()).hexdigest(),removedRoots=counts,
            logicalBytes=sum(r['logicalBytes'] for r in counts),elapsedSeconds=time.monotonic()-started,
            removedBranch='codex/gui-startup',
            preserved=['main target/debug and global caches','Windows SSH service/public authorization and external0600privatekey','three intentional /mnt/data development ZIPs','R60 owned uncompleted MSI/registry fixtures for upcoming isolated native feasibility tests','active R62 evidence and next CI preparation'],
            scope='Only exact marker-owned completed roots after committed full byte proof and all known consumers released. No Windows mutation or private-key read.')
out=EV/'local-cleanup-first.json';assert not out.exists();out.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:result[k] for k in ['status','logicalBytes','elapsedSeconds']}),flush=True)
