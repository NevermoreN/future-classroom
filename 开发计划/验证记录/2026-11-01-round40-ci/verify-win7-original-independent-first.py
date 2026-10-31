from pathlib import Path,PurePosixPath
import datetime,hashlib,json,re,runpy,struct,subprocess,tempfile,zipfile,zlib
ROOT=Path('/workspace/共享/Eliora/未来教室计划')
EV=Path('/tmp/fc-round40-ci-evidence');BASE=EV/'win7-artifact-raw'
ZIP=Path('/tmp/fc-round40-win7-original.zip');OUT=EV/'win7-original-independent-first.json'
SOURCE='3b3d3abbfb80240989e32bc0858234d25bc277fc'
EXPECTED_ZIP_SHA='43587d8992441a5602c47d49b63dcc45f59ff34b7395a2882d9ffb8b2f8f201f'
sha=lambda b:hashlib.sha256(b).hexdigest()
load=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
assert not OUT.exists(), 'refuse report overwrite'
audit_script=ROOT/'scripts/audit-pe.py';audit=runpy.run_path(str(audit_script))['audit_pe']
report={'schema':'fc.win7-original-independent.v1','status':'RUNNING','recordedAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceSha':SOURCE,'scope':'Independent original artifact ZIP/CRC/SHA/producer ledger/full input checkout/22 PE32 CUI subsystem6.1 and imported dynamic CRT rejection. No executable run, Cargo, Windows, remote, source mutation, lint suppression, or Git mutation. CI failure remains FAILED_OR_INCOMPLETE.','zip':{'path':str(ZIP)},'members':[],'executables':[],'inputs':[]}
try:
 rawzip=ZIP.read_bytes();assert len(rawzip)==41254371 and sha(rawzip)==EXPECTED_ZIP_SHA
 report['zip'].update(bytes=len(rawzip),sha256=sha(rawzip));del rawzip
 producer=load(BASE/'artifact-sha256.json');assert len(producer)==200
 ledger={x['path']:x for x in producer};assert len(ledger)==len(producer)
 exe_rows=load(BASE/'executables.json');assert len(exe_rows)==22
 exes={x['relative_path']:x for x in exe_rows};assert len(exes)==22
 assert sum(x['role']=='tests' for x in exe_rows)==21
 rawfiles={str(p.relative_to(BASE)).replace('\\','/'):p for p in BASE.rglob('*') if p.is_file()}
 with zipfile.ZipFile(ZIP) as z:
  infos=z.infolist();names=[x.filename for x in infos]
  assert len(names)==len(set(names))==201 and not any(x.is_dir() for x in infos)
  assert set(names)==set(ledger)|{'artifact-sha256.json'}
  assert set(rawfiles)==set(names)-set(exes)
  with tempfile.TemporaryDirectory(prefix='fc-round40-win7-pe-') as temporary:
   temp=Path(temporary);report['temporaryPeDirectory']=str(temp)
   for info in infos:
    name=info.filename;pp=PurePosixPath(name)
    assert not pp.is_absolute() and '..' not in pp.parts and '\\' not in name
    data=z.read(info);digest=sha(data);assert len(data)==info.file_size
    assert zlib.crc32(data)&0xffffffff==info.CRC
    row={'path':name,'bytes':len(data),'sha256':digest,'zipCrc32':f'{info.CRC:08x}','crcChecked':True,'producerLedgerMatched':name in ledger,'rawOriginalMatched':name in rawfiles}
    if name in ledger:assert ledger[name]['bytes']==len(data) and ledger[name]['sha256']==digest
    if name in rawfiles:assert rawfiles[name].read_bytes()==data
    if name in exes:
     expected=exes[name];assert expected['bytes']==len(data) and expected['sha256']==digest
     assert data[:2]==b'MZ';peoff=struct.unpack_from('<I',data,0x3c)[0];assert data[peoff:peoff+4]==b'PE\0\0'
     machine=struct.unpack_from('<H',data,peoff+4)[0];optional=peoff+24
     magic=struct.unpack_from('<H',data,optional)[0]
     subsystem_version=list(struct.unpack_from('<HH',data,optional+48));subsystem=struct.unpack_from('<H',data,optional+68)[0]
     assert machine==0x14c and magic==0x10b and subsystem==3 and subsystem_version==[6,1]
     p=temp/PurePosixPath(name).name;assert not p.exists();p.write_bytes(data)
     try: pe=audit(p)
     finally:p.unlink()
     assert not pe['structural_errors']
     dlls=list(pe['imports']);assert dlls
     forbidden=[d for d in dlls if d.startswith(('vcruntime','msvcp','api-ms-win-crt','libgcc_s_','libstdc++','libwinpthread')) or d=='ucrtbase.dll' or re.fullmatch(r'msvcr[0-9]+\.dll',d)]
     assert not forbidden,(name,forbidden)
     report['executables'].append({'producer':expected,'bytesAndShaMatched':True,'peHeader':{'machine':hex(machine),'optionalMagic':hex(magic),'subsystem':subsystem,'subsystemVersion':subsystem_version},'forbiddenDynamicRuntimeImports':forbidden,'independentPeAudit':pe,'temporaryExeActuallyDeleted':not p.exists()})
    report['members'].append(row)
  report['temporaryPeDirectoryActuallyRemoved']=not temp.exists();assert not temp.exists()
 inputs=load(BASE/'input-sha256.json');assert len(inputs)==5829
 paths={p.decode() for p in subprocess.check_output(['git','ls-tree','-r','--name-only','-z',SOURCE],cwd=ROOT).split(b'\0') if p}
 assert len({x['path'] for x in inputs})==len(inputs) and {x['path'] for x in inputs}==paths
 proc=subprocess.Popen(['git','cat-file','--batch'],cwd=ROOT,stdin=subprocess.PIPE,stdout=subprocess.PIPE)
 direct=0;crlf=0
 try:
  for x in inputs:
   assert '\n' not in x['path']
   proc.stdin.write((SOURCE+':'+x['path']+'\n').encode());proc.stdin.flush()
   hdr=proc.stdout.readline().decode().strip().split();assert len(hdr)==3 and hdr[1]=='blob'
   body=proc.stdout.read(int(hdr[2]));assert proc.stdout.read(1)==b'\n'
   gitbytes=len(body);gitsha=sha(body)
   if gitbytes==x['bytes'] and gitsha==x['sha256']:
    direct+=1;mode='EXACT_GIT_BLOB'
   else:
    assert b'\r' not in body and b'\0' not in body
    converted=body.replace(b'\n',b'\r\n');assert len(converted)==x['bytes'] and sha(converted)==x['sha256'],x['path']
    crlf+=1;mode='EXACT_LF_TO_CRLF_CHECKOUT'
   report['inputs'].append({**x,'gitBlobBytes':gitbytes,'gitBlobSha256':gitsha,'checkoutMatch':mode})
 finally:
  proc.stdin.close();assert proc.wait()==0
 result=load(BASE/'result.json');cleanup=load(BASE/'cleanup.json');start=load(BASE/'run-start.json')
 assert result['source_sha']==result['dispatch_source_sha']==SOURCE
 assert result['status']=='FAILED_OR_INCOMPLETE' and result['baseline']=='Win7' and result['target']=='i686-win7-windows-msvc'
 assert result['input_manifest_sha256']==sha((BASE/'input-sha256.json').read_bytes())
 commands=result['commands'];failed=[x for x in commands if not x['passed']]
 assert len(failed)==1 and failed[0]['name']=='clippy' and failed[0]['exit_code']==101
 assert all(x['exit_code']==0 for x in commands if x['name']!='clippy')
 assert all(not x['timed_out_or_pipe_stalled'] and not x['log_truncated'] and x['parent_exited'] for x in commands)
 assert cleanup['status']=='OWNED_BUILD_ROOTS_REMOVED' and all(x['removed'] for x in cleanup['roots'])
 # Original logs remain bytes in raw tree and are independently bound to ZIP above.
 logs=[]
 for command in commands:
  logs.append({'command':command,'originalLogs':[{'path':command[k],'bytes':(BASE/command[k]).stat().st_size,'sha256':sha((BASE/command[k]).read_bytes())} for k in ['stdout','stderr']]})
 lint=(BASE/'logs/clippy.stderr.log').read_text(encoding='utf-8-sig')
 diagnostics=[]
 for match in re.finditer(r'(?m)^error: initializer for `thread_local` value can be made `const`\n',lint):
  block=lint[match.start():].split('\n\n',1)[0];diagnostics.append(block)
 assert len(diagnostics)==5
 report.update(status='PASS_INDEPENDENT_ARTIFACT_CHECKS_CI_REMAINS_FAILED',counts={'zipMembers':len(report['members']),'producerLedgerRows':len(producer),'nonbinaryRawFiles':len(rawfiles),'testExecutables':21,'productExecutables':1,'inputs':len(inputs),'directGitBlobMatches':direct,'exactLfToCrlfMatches':crlf,'commands':len(commands),'clippyMissingConstDiagnostics':len(diagnostics)},ciTerminal={'originalStatus':result['status'],'sourceSha':result['source_sha'],'failedCommands':failed,'originalResultOtherFields':{k:v for k,v in result.items() if k!='commands'},'originalStart':start,'originalCleanup':cleanup,'commandOriginalLogBindings':logs,'missingConstDiagnostics':diagnostics},producerLedger=producer,rustSrcRecordedOriginal=load(BASE/'rust-src-provenance.json'),auditTool={'path':str(audit_script),'bytes':audit_script.stat().st_size,'sha256':sha(audit_script.read_bytes())},limitations=['Original CI strict Clippy remains exit101: five missing_const_for_thread_local diagnostics against already-const declarations; no workaround/source edit performed. Exact build-std provenance/root-cause investigation remains separate.','Selected installed rust-src provenance copied from producer, not independently certified or proof of complete toolchain/SDK compatibility.','Import audit shows no forbidden dynamic CRT imports among enumerated DLL imports. PE32 subsystem6.1/static-runtime configuration is not Win7 API or runtime certification.','Windows7/Windows10/Windows11/Seewo executable runtime NOT_RUN in this independent static audit.','All original raw reports/logs are unchanged. Input identity accepts only exact Git bytes or a complete exact LF-to-CRLF transformation, not normalized text comparison.'])
except BaseException as e:
 report.update(status='FAILED_INDEPENDENT_CHECK',errorType=type(e).__name__,error=str(e));OUT.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');raise
OUT.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'path':str(OUT),'bytes':OUT.stat().st_size,'sha256':sha(OUT.read_bytes()),'status':report['status'],'counts':report['counts']},ensure_ascii=False))
