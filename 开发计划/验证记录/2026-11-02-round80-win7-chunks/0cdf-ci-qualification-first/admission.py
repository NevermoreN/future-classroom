"""Fixed official 71ced30 ZIPs; Win7 Clippy failure remains a failure. No native execution or network access."""
from pathlib import Path, PurePosixPath, PureWindowsPath
import subprocess,json,hashlib,zipfile,re,struct
R=Path('/tmp/fc-round80-controlled-repair')
W=Path('/workspace/共享/Eliora/未来教室计划')
SOURCE='0cdfd5a70fd90df3e04a570c8dcb22e48162b9e1'
CONFIG={'modern': {'run': 41387127138, 'artifact': 13116582610, 'size': 46264824, 'zipsha': '7a422191697419963917f211f334bf1f49929ec105c426515a40511d065bff70', 'target': 'i686-pc-windows-msvc', 'toolchain': '1.97.1-x86_64-pc-windows-msvc', 'compiler': '8bab26f4f68e0e26f0bb7960be334d5b520ea452', 'exe': 'fc_windows-ef7db952e7d3daf0.exe', 'exe_sha': 'b0560e41ea751a161814e44c60ebd91c6b3b5b6eb13c66877a9638e78a3fec32', 'exe_size': 3825664}, 'win7': {'run': 41387130749, 'artifact': 13116253681, 'size': 47095217, 'zipsha': '46823786559e0107d5bc468deac5d560f77f30834b8ef710d7ad98cf31884bf1', 'target': 'i686-win7-windows-msvc', 'toolchain': 'nightly-2026-10-29-x86_64-pc-windows-msvc', 'compiler': '92ffebeceaa0a071066f233873c2438e73d70fe7', 'exe': 'fc_windows-31bbbe137f78164a.exe', 'exe_sha': '76e601e1c2db888e21bd1f4cd82b267ef87618ec574ec2a2d83d8d718bca17fc', 'exe_size': 3945472}}
SCOPE_NAMES=('owned_removal_scope_never_accepts_unbound_or_runtime_authority_text','owned_removal_pending_requires_exact_original_schema_sid_and_tag5','owned_removal_scope_closes_full_manifest_receipts_and_object_identity')
def digest(b):return hashlib.sha256(b).hexdigest()
def fact(p):
 p=Path(p);assert p.is_file() and not p.is_symlink()
 b=p.read_bytes();return dict(path=str(p),bytes=len(b),sha256=digest(b))
def commit_blobs(rows, tree):
 paths=[r['path'] for r in rows];expected={p for p in subprocess.check_output(['git','ls-tree','-r','-z','--name-only',SOURCE],cwd=W).decode().split('\0') if p and not p.startswith('开发计划/验证记录/')};assert len(paths)==len(set(paths))==655 and set(paths)==expected
 for p in paths:
  q=PurePosixPath(p);assert not q.is_absolute() and all(t not in ('','.','..') for t in q.parts) and '\\' not in p and '\n' not in p and ':' not in p
 commands=''.join(SOURCE+':'+p+'\n' for p in paths).encode()
 result=subprocess.run(['git','cat-file','--batch'],cwd=W,input=commands,capture_output=True,check=True)
 data=result.stdout;cursor=0;blobs={};proof=[]
 tm={r['path']:r for r in tree['files']};assert len(tm)==len(tree['files'])

 # Replay all actual Git metadata, including historical evidence, without
 # pretending that historical file contents are current compiler inputs.
 rawtree=subprocess.check_output(['git','ls-tree','-r','-l','-z',SOURCE],cwd=W)
 committed={}
 for record in rawtree.split(b'\0'):
  if not record:continue
  prefix,name=record.split(b'\t',1);mode,kind,oid,size=prefix.split()
  committed[name.decode()] = (mode.decode(),kind.decode(),oid.decode(),int(size))
 assert len(committed)==len(tm)==19926
 for name,row in tm.items():
  assert (row['mode'],row['type'],row['git_blob_oid_sha1'],row['git_blob_bytes'])==committed[name]
 assert tree['schema']=='future-classroom.ci-git-tree-inputs/v1' and tree['source_sha']==SOURCE
 assert tree['root_tree_oid_sha1']==subprocess.check_output(['git','rev-parse',SOURCE+'^{tree}'],cwd=W).decode().strip()
 for row in rows:
  end=data.index(b'\n',cursor);header=data[cursor:end].split();assert len(header)==3 and header[1]==b'blob'
  n=int(header[2]);start=end+1;b=data[start:start+n];assert len(b)==n and data[start+n:start+n+1]==b'\n'
  t=tm[row['path']];assert t['type']=='blob' and t['git_blob_oid_sha1']==header[0].decode() and t['git_blob_bytes']==n
  mode='EXACT_GIT_BLOB';materialized=b
  if row['bytes']!=n or row['sha256']!=digest(b):
   b.decode('utf-8');assert b'\r' not in b and b'\0' not in b and b'\n' in b
   materialized=b.replace(b'\n',b'\r\n');mode='EXACT_UTF8_LF_TO_CRLF_CHECKOUT_BYTES'
  assert row['bytes']==len(materialized) and row['sha256']==digest(materialized),('COMMIT_INPUT_MISMATCH',row['path'])
  proof.append(dict(path=row['path'],gitBlobOid=header[0].decode(),gitBlobBytes=n,gitBlobSha256=digest(b),ciCheckoutBytes=row['bytes'],ciCheckoutSha256=row['sha256'],materialization=mode))
  blobs[row['path']]=b;cursor=start+n+1
 assert cursor==len(data)
 return blobs,proof

def qualify(label):
 c=CONFIG[label];root=R/('0cdf-'+label+'-download-first');zp=root/'artifact.zip';zfact=fact(zp)
 assert zfact['bytes']==c['size'] and zfact['sha256']==c['zipsha']
 dl=json.loads((root/'download-report.json').read_text());assert dl['status']=='PASS_GITHUB_ARTIFACT_ZIP_IDENTITY_ONLY' and dl['bytes']==c['size'] and dl['sha256']==c['zipsha'] and dl['artifactId']==c['artifact'] and dl['runId']==c['run'] and dl['head']==SOURCE
 inputs=[]
 with zipfile.ZipFile(zp) as z:
  names=z.namelist();assert len(names)==len(set(names))
  for name in names:
   q=PurePosixPath(name);assert not q.is_absolute() and all(t not in ('','.','..') for t in q.parts) and '\\' not in name and ':' not in name
   info=z.getinfo(name);assert not info.is_dir() and ((info.external_attr>>16)&0o170000) in (0,0o100000)
  index=json.loads((root/'zip-inventory.json').read_text());assert {r['path']:r['bytes'] for r in index}=={n:z.getinfo(n).file_size for n in names}
  def raw(name):
   b=z.read(name);inputs.append(dict(member=name,bytes=len(b),sha256=digest(b)));return b
  def js(name):return json.loads(raw(name))
  ledger=js('artifact-sha256.json');assert len(ledger)==len({r['path'] for r in ledger})
  lm={r['path']:r for r in ledger}
  for row in ledger:
   b=z.read(row['path']);assert len(b)==row['bytes'] and digest(b)==row['sha256'],('ARTIFACT_MEMBER',row['path'])
  for name in ['result.json','input-sha256.json','input-sha256-after.json','executables.json','bootstrap.json','run-start.json','logs/workspace-test-compile.result.json','logs/workspace-test-compile.stdout.log','logs/cargo-metadata.result.json','logs/cargo-metadata.stdout.log','logs/rust-version.result.json','logs/rust-version.stdout.log']:
   if name!='result.json':assert name in lm
  result=js('result.json');boot=js('bootstrap.json');start=js('run-start.json')
  allcommands=[]
  for name in sorted(n for n in names if n.startswith('logs/') and n.endswith('.result.json')):
   cmd=js(name)
   assert cmd['parent_exited'] and not cmd['timed_out_or_pipe_stalled'] and not cmd['log_truncated']
   for stream in ('stdout','stderr'):
    body=raw(cmd[stream]);assert digest(body)==cmd[stream+'_sha256']
   if not cmd['passed']:
    assert label=='win7' and cmd['name']=='clippy' and cmd['exit_code']==101
    body=z.read(cmd['stderr']).decode();assert 'error: using `chunks_exact` with a constant chunk size' in body and 'error: using `chunks_exact_mut` with a constant chunk size' in body and 'predecessor.rs:48:23' in body and 'controlled_owner.rs:219:33' in body
   else:assert cmd['exit_code']==0
   allcommands.append(cmd)
  assert len(allcommands)==(51 if label=='modern' else 53) and sum(not v['passed'] for v in allcommands)==(0 if label=='modern' else 1)
  assert len({v['name']for v in allcommands})==len(allcommands)
  assert {v['name']:v for v in allcommands}=={v['name']:v for v in js('result.json')['commands']}
  assert result['status']==('COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN' if label=='modern' else 'FAILED_OR_INCOMPLETE') and result['source_sha']==result['dispatch_source_sha']==SOURCE
  assert result['target']==c['target'] and result['toolchain']==c['toolchain']
  expected_flags='-C target-feature=+crt-static'+(' -C link-arg=/SUBSYSTEM:CONSOLE,6.01' if label=='win7' else '')
  assert result['rustflags']==start['rustflags']==expected_flags
  assert boot['expected_source_sha']==boot['dispatch_source_sha']==boot['workflow_sha']==SOURCE and int(boot['run_id'])==c['run'] and boot['run_attempt']=='1'
  assert start['expected_source_sha']==start['dispatch_source_sha']==start['workflow_sha']==SOURCE and start['target']==c['target'] and start['toolchain']==c['toolchain'] and start['tests']=='COMPILE_ONLY_NO_RUN'
  before=raw('input-sha256.json');after=raw('input-sha256-after.json');assert before==after and digest(before)==result['input_manifest_sha256']
  treebefore=raw('input-git-tree.json');treeafter=raw('input-git-tree-after.json');assert treebefore==treeafter and digest(treebefore)==result['git_tree_manifest_sha256']
  sources=json.loads(before);blobs,sourceproof=commit_blobs(sources,json.loads(treebefore))
  def command(name):
   value=js('logs/'+name+'.result.json');assert value['passed'] and value['exit_code']==0 and value['parent_exited'] and not value['timed_out_or_pipe_stalled'] and not value['log_truncated']
   for stream in ['stdout','stderr']:
    b=raw(value[stream]);assert digest(b)==value[stream+'_sha256']
   return value
  compile=command('workspace-test-compile');mdcommand=command('cargo-metadata');rust=command('rust-version')
  args=['test','-vv','--workspace','--all-targets','--release','--locked','--target',c['target'],'--no-run','--message-format=json']
  if label=='win7':args=['-Z','build-std']+args
  assert compile['arguments']==args and mdcommand['arguments']==['metadata','--locked','--format-version','1','--no-deps'] and rust['arguments']==['run',c['toolchain'],'rustc','-vV']
  assert ('commit-hash: '+c['compiler']+'\n') in z.read(rust['stdout']).decode()
  metadata=js(mdcommand['stdout']);assert len(metadata['workspace_members'])==12
  packages=[p for p in metadata['packages'] if p['name']=='fc-windows'];assert len(packages)==1;package=packages[0]
  target=[t for t in package['targets'] if t['name']=='fc_windows'];assert len(target)==1;target=target[0]
  assert target['kind']==target['crate_types']==['rlib','cdylib'] and PureWindowsPath(target['src_path'])==PureWindowsPath(metadata['workspace_root'])/'crates/fc-windows/src/lib.rs'
  messages=[json.loads(line) for line in z.read(compile['stdout']).decode().splitlines() if line.startswith('{')]
  artifacts=[a for a in messages if a.get('reason')=='compiler-artifact' and a.get('package_id')==package['id'] and a.get('target')==target and a.get('profile',{}).get('test') and a.get('executable')]
  assert len(artifacts)==1;artifact=artifacts[0];assert artifact['features']==['default'] and artifact['profile']['opt_level']=='3' and not artifact['profile']['debug_assertions']
  nativepath=PureWindowsPath(artifact['executable']);assert nativepath.name==c['exe'] and nativepath.is_relative_to(PureWindowsPath(metadata['target_directory'])/c['target']/'release')
  exes=js('executables.json');assert digest(z.read('executables.json'))==result['executables_manifest_sha256']
  selected=[a for a in exes if a['role']=='tests' and a['package']=='fc-windows' and a['target']=='fc_windows' and a['cargo_profile_test'] is True];assert len(selected)==1;exe=selected[0]
  member='tests/fc-windows/'+c['exe'];assert exe['relative_path']==member and exe['bytes']==c['exe_size'] and exe['sha256']==c['exe_sha'] and exe['kinds']==['rlib','cdylib'] and exe['executed'] is False
  assert lm[member]['bytes']==c['exe_size'] and lm[member]['sha256']==c['exe_sha']
  pe=raw(member);assert len(pe)==c['exe_size'] and digest(pe)==c['exe_sha']
  assert pe[:2]==b'MZ';off=struct.unpack_from('<I',pe,60)[0];assert pe[off:off+4]==b'PE\0\0' and struct.unpack_from('<H',pe,off+4)[0]==0x14c and struct.unpack_from('<H',pe,off+24)[0]==0x10b and struct.unpack_from('<H',pe,off+92)[0]==3
  if label=='win7':assert struct.unpack_from('<HH',pe,off+72)==(6,1)
  return dict(label=label,status=('PASS_CURRENT_MODERN_COMPILE_CHECKS_ONLY' if label=='modern' else 'PASS_0CDF_WIN7_PROVENANCE_TWO_CHUNKS_CLIPPY_ERRORS_RETAINED'),sourceSha=SOURCE,zip=zfact,runId=c['run'],artifactId=c['artifact'],target=c['target'],toolchain=c['toolchain'],compilerCommit=c['compiler'],ciMemberInputs=inputs,committedInputRowsVerified=len(sources),committedInputMaterializationProof=sourceproof,compilerArtifact=artifact,executable=exe,allNativeRuntime='NOT_RUN',allCommands=allcommands,originalOverallResult=result['status'],developmentG02Source='NOT_COVERED_BY_0CDF_CI')
