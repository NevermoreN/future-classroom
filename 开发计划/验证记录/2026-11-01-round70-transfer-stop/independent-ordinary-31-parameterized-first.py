#!/usr/bin/env python3
"""Prepare-only until root invokes with final frozen source and terminal originals.
Audits actual ordinary31 report/raw native stdout using its frozen runner's pure
assertions. Never runs Windows/SSH/Cargo/network or rewrites an original result.
"""
from pathlib import Path
import argparse,ast,base64,datetime,hashlib,importlib.util,json,re,ssl,struct,sys
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parent.parent
E=ROOT/'evidence'
W=ROOT/'worktree'
class Waiting(Exception):pass
def need(ok,code):
 if not ok:raise AssertionError(code)
def sha(data):return hashlib.sha256(data).hexdigest()
def read(path):
 p=Path(path)
 if not p.is_file():raise Waiting('MISSING_'+p.name)
 need(not p.is_symlink(),'nonregular input')
 try:v=json.loads(p.read_text(encoding='utf-8-sig'))
 except json.JSONDecodeError:raise Waiting('INCOMPLETE_'+p.name) from None
 if isinstance(v,dict) and v.get('status') in ('RUNNING','STARTED'):raise Waiting('RUNNING_'+p.name)
 return v
def fact(path):
 p=Path(path)
 if not p.is_file():raise Waiting('MISSING_'+p.name)
 need(not p.is_symlink(),'nonregular fact')
 h=hashlib.sha256();count=0
 with p.open('rb') as stream:
  while data:=stream.read(1024*1024):h.update(data);count+=len(data)
 return dict(path=str(p),bytes=count,sha256=h.hexdigest())
def raw_entry(entry):
 data=base64.b64decode(entry['base64'],validate=True)
 need(len(data)==entry['bytes'] and sha(data)==entry['sha256'],'exact encoded raw identity')
 if 'observedBytes' in entry:need(entry['observedBytes']==len(data) and entry['captureScope']=='COMPLETE','complete original raw capture')
 return data
def text(data):return data.decode('utf-16') if data.startswith((b'\xff\xfe',b'\xfe\xff')) else data.decode('utf-8-sig')
def provenance(path,freeze_sha):
 v=read(path);need(v.get('status') in ('PASS','PASS_COMMAND') and type(v.get('exitCode')) is int and v['exitCode']==0,'original controller terminal exit0')
 need(v.get('sourceUnchanged') is True and v.get('sourceFreezeSha256')==freeze_sha and v.get('workingFileCount')==595,'actual controller exact frozen source')
 need(v.get('originalBudgetSeconds')==900 and v.get('elapsedSeconds',901)<=900,'original900s controller budget')
 streams={}
 for key in ('stdout','stderr'):
  p=Path(path).with_suffix('.'+key+'.log');identity=fact(p)
  need(identity['bytes']==v[key+'Bytes'] and identity['sha256']==v[key+'Sha256'],'actual original controller raw '+key)
  streams[key]=identity
 return v,dict(report=fact(path),streams=streams)
def main():
 p=argparse.ArgumentParser(description=__doc__)
 for name in ('actual-report','freeze','product-provenance','operation-provenance'):p.add_argument('--'+name,type=Path,required=True)
 p.add_argument('--freeze-sha256',required=True);p.add_argument('--output',type=Path,default=E/'independent-ordinary-31-first.json');a=p.parse_args()
 need(a.output.parent.resolve()==E and not a.output.exists(),'fresh exclusive output in R70 evidence required')
 out=dict(schemaVersion=1,status='FAIL_ORDINARY31_INDEPENDENT_AUDIT',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
 try:
  need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r70-ordinary-transfer-stop/v1\n','exact owned R70 root')
  need(re.fullmatch('[0-9a-f]{64}',a.freeze_sha256) and fact(a.freeze)['sha256']==a.freeze_sha256,'exact supplied final freeze SHA')
  frozen=read(a.freeze);need(len(frozen['workingFiles'])==595,'595 frozen working inputs')
  source={row['path']:row for row in frozen['workingFiles']}
  for rel,row in source.items():
   got=fact(W/rel);need(got['bytes']==row['bytes'] and got['sha256']==row['sha256'],'frozen source changed')
  actual=read(a.actual_report)
  need(actual.get('status')==actual.get('cleanup')=='PASS' and actual.get('materialReadyStop')=='PASS','original successful report required; FAIL never promoted')
  op,op_identity=provenance(a.operation_provenance,a.freeze_sha256)
  product,product_identity=provenance(a.product_provenance,a.freeze_sha256)
  cmd=op['command'];need(isinstance(cmd,list) and cmd.count('--report')==1 and Path(cmd[cmd.index('--report')+1]).resolve()==a.actual_report.resolve(),'actual runner controller/result binding')
  runner=W/'scripts/windows-ordinary-https-probe.py';need(str(runner) in cmd and sha(runner.read_bytes())==actual['runnerSourceSha256'],'exact frozen ordinary runner')
  need(actual['ownershipSourceSha256']==sha((W/'scripts/windows-tls-probe.py').read_bytes()) and actual['businessHelperSourceSha256']==sha((W/'scripts/windows-tls-business-probe.py').read_bytes()),'exact ownership/business helper source')
  # Select only the actual unique non-test normal from original product CargoJSON.
  candidates=[]
  for number,line in enumerate(Path(product_identity['streams']['stdout']['path']).read_bytes().splitlines(keepends=True),1):
   try:item=json.loads(line)
   except (ValueError,UnicodeError):continue
   if item.get('reason')=='compiler-artifact' and item.get('executable') and item.get('target',{}).get('name')=='fc-bootstrap' and item.get('profile',{}).get('test') is False:candidates.append((number,line,item))
  need(len(candidates)==1,'one actual Cargo normal product')
  number,line,producer=candidates[0];exe=Path(producer['executable']);need(exe.resolve().is_relative_to((ROOT/'target').resolve()) and str(exe) in producer.get('filenames',[]),'owned original product path')
  normal=fact(exe);need(normal['sha256']==actual['exeSha256'] and cmd.count('--exe')==1 and Path(cmd[cmd.index('--exe')+1])==exe,'actual supplied/uploaded normal SHA')
  with exe.open('rb') as stream:
   dos=stream.read(64);need(len(dos)==64 and dos[:2]==b'MZ','normal DOS header');offset=struct.unpack_from('<I',dos,60)[0];stream.seek(offset);pe=stream.read(96)
  need(len(pe)==96 and pe[:4]==b'PE\0\0' and struct.unpack_from('<H',pe,4)[0]==0x14c and struct.unpack_from('<H',pe,24)[0]==0x10b and struct.unpack_from('<H',pe,92)[0]==2,'actual I386 PE32 GUI normal')
  # Import only frozen pure validators; __main__ is never entered.
  spec=importlib.util.spec_from_file_location('fc_r70_actual31_readonly',runner);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
  need(actual['fixtureSetupSourceSha256']==sha(m.tls.SETUP.encode()),'exact original fixture setup source')
  native=(W/'crates/fc-bootstrap/src/application/ordinary_https_probe.rs').read_text()
  order=re.findall(r'"([a-z-]+)"',native.split('const ORDER: [&str; 31] = [',1)[1].split('];',1)[0]);need(len(order)==31 and tuple(order)==m.STEPS,'exact31 native and Python steps')
  need('ORDINARY_MATERIAL_PREPARATION' not in (W/'crates/fc-bootstrap/src/web_application/material_files.rs').read_text(),'temporary diagnostic must be removed before final acceptance')
  preservation=actual['rawPreservation'];need(preservation['status']=='PASS','actual native raw preservation')
  raw_dir=Path(preservation['directory']);need(raw_dir.resolve().is_relative_to(E.resolve()),'owned raw evidence directory')
  raw_files={};raw_identities=[]
  need(set(preservation['files'])==set(actual['remoteRawEvidence']),'complete actual native raw set')
  for name,entry in preservation['files'].items():
   need('/' not in name and '\\' not in name and name not in ('.','..'),'fixed raw basename')
   path=raw_dir/name;need(Path(entry['artifact'])==path and entry['status']=='PASS','exact raw path/status')
   data=path.read_bytes();need(len(data)==entry['remote']['bytes']==entry['downloaded']['bytes'] and sha(data)==entry['remote']['sha256']==entry['downloaded']['sha256'],'native raw source/downloaded identity')
   need(data==raw_entry({**entry['remote'],'base64':entry['base64']}) and entry['remote']==actual['remoteRawEvidence'][name],'native raw inline exact original')
   raw_files[name]=data;raw_identities.append(fact(path))
  native_lines=text(raw_files['tls.stdout']).splitlines();transcript=m.validate_transcript(native_lines)
  need(transcript==actual['transcript'],'actual full native stdout31/footer matches report')
  need('ORDINARY_MATERIAL_PREPARATION' not in text(raw_files['tls.stderr']),'diagnostic output absent from final native run')
  marker,port=actual['marker'],actual['configuredPort'];ready=[];pairs=[];stops=[]
  for line in native_lines:
   for prefix,dest in [('ORDINARY_HTTPS_READY ',ready),('ORDINARY_HTTPS_PAIR_READY ',pairs),('ORDINARY_HTTPS_STOPPED ',stops)]:
    if line.startswith(prefix):dest.append(json.loads(line[len(prefix):]))
  need(len(ready)==len(pairs)==len(stops)==2 and ready==actual['ready'] and stops==actual['stops'],'two actual raw READY/pair/stop observations')
  for i in range(2):
   m.validate_ready(ready[i],marker,port,i+1,previous=ready[0]['generation'] if i else None)
   m.validate_pair_ready(pairs[i],marker,port,ready[i]['generation'],i+1);m.validate_stop(stops[i],marker,ready[i]['generation'],i+1)
  events=actual['events'];by_case={v['case']:v for v in events};need(len(by_case)==len(events),'unique actual event identities')
  def upload_id(case):
   match=re.fullmatch(r'materials/uploads/([1-9][0-9]{0,19})/bytes',by_case[case]['route']);need(match is not None,'exact upload event path');return match[1]
  ids=[upload_id('r70-bytes-a'),actual['partialUploadStop']['uploadId'],upload_id('r70-bytes-c'),upload_id('r70-bytes-d')]
  journey=m.validate_transfer_journey(events,ready,actual['authenticatedBusiness'],actual['secondPairing'],ids,marker,port,stops,actual['partialUploadStop'])
  need(journey==actual['wireJourney'],'strict full actual journey matches original report')
  before=actual['materialBeforeStop'];need(set(before)=={'status','readyStageBytes','partialStageBytes','appendBeforeStop'} and before['status']=='PASS' and before['readyStageBytes']==65536 and 0<before['partialStageBytes']<1048576 and before['appendBeforeStop'] is False,'actual original ready and partial stages before stop')
  reuse=actual['materialReuse'];need(reuse==dict(status='PASS',reusedSlots=2,commandId='69001',scope={'ownerTeacherId':'2','subjectId':'1'},receipt='COMMITTED',exactRetry=True,resourceVersionId=ids[2],sha256=m.b.digest(m.MATERIAL),bytes=65536),'typed synthetic material commit/retry/readback/cancel summary')
  # Exact ordered validators require C/D full upload200 before either append or
  # cancel; first/second poll ranges remain1..8, with final200 and preceding202.
  need(actual['nativeExit']=={'code':0},'actual original native successful exit')
  launch=actual['sshLauncherRaw'];launch_stdout=raw_entry(launch['stdout']);raw_entry(launch['stderr'])
  exits=[json.loads(s[len(m.tls.EXIT):]) for s in text(launch_stdout).splitlines() if s.startswith(m.tls.EXIT)];need(exits==[actual['nativeExit']],'actual supervisor one exit0')
  launch_lines=text(launch_stdout).splitlines()
  need(launch_lines[:len(native_lines)]==native_lines,'complete native line sequence retained before supervisor exit; raw streams independently hashed')
  for stream in actual['sshTunnelRaw'].values():raw_entry(stream)
  verification=actual['clientVerification'];need(verification==dict(checkHostname=True,verifyMode='CERT_REQUIRED',serverName='127.0.0.1',minimumTls='TLSv1.2',maximumTls='TLSv1.2',peerCertificate='EXACT_OWNED_DER',trustSource='only owned public PEM',cleanTlsEof='REQUIRED',systemTrust='UNCHANGED'),'strict exact owned IP-SAN TLS policy')
  der=ssl.PEM_cert_to_DER_cert(raw_files['tls-public.pem'].decode('ascii'));need(hashlib.sha1(der).hexdigest().upper()==actual['fixture']['thumbprint'].upper(),'original owned certificate identity')
  cleanup=actual['cleanupDetails'];need(cleanup['status']=='PASS' and cleanup['rootAbsent'] is True and cleanup['cngKeyExists'] is False and cleanup['myDigestRestored'] is True and cleanup['ownedProcessesRemaining']==0 and cleanup['certificatesRemoved']==1,'actual owned root/cert/CNG cleanup')
  need(all(cleanup[k]=='UNTOUCHED' for k in ('httpSys','sshConfiguration','rootTrustStore','firewall')) and actual['pairingSecretCleanup']=='PASS' and actual['ownedProcessCleanup']['status']=='PASS' and actual['ownedProcessCleanup']['ownedProcessesRemaining']==0,'exact secret/process cleanup and persistent boundaries')
  need(actual['campus']=='NOT_VALIDATED' and actual['win7']==actual['win10']=='NOT_RUN' and actual['productionSchoolSettings']=='UNCHANGED','finite actual scope retained')
  need('client_deadline=client_deadline or time.monotonic()+30' in runner.read_text() and 'Duration::from_secs(120)' in native and 'Duration::from_secs(5)' in native,'original client/native/stop budgets')
  for rel,row in source.items():need(fact(W/rel)['sha256']==row['sha256'],'source changed during independent audit')
  out.update(status='PASS_FINAL_FROZEN_ORDINARY31_FINITE_WIN11_ONLY',freeze=fact(a.freeze),actualReport=fact(a.actual_report),productProvenance=product_identity,operationProvenance=op_identity,
   normal={**normal,'cargoLine':number,'cargoLineSha256':sha(candidates[0][1])},runner=fact(runner),rawOriginals=raw_identities,
   nativeStepNames=order,transcript=transcript,twoActualGenerations=[v['generation'] for v in ready],stops=stops,journey=journey,materialBeforeStop=before,materialReuse=reuse,
   teacher=dict(principalId='2',role='TEACHER_ADMIN',nativeApprovals=2,secondSession='ACTUAL_FROZEN_RUNNER_MEMORY_ONLY_FRESH_COOKIE_AND_CSRF_ASSERTION',oldCookie='401'),
   cleanup='PASS',sourceUnchanged=True,scope='Only supplied final source/binary/report and complete actual ordinary31 loopback composition. No Win7/10 OS, classroom/campus or whole-product certification.',
   secrecy='No credential-bearing DTO/header/body or credential digest copied. Typed session, exact material69001 response/retry/readback and fresh cookie/CSRF assertions run in the frozen original client; archived report keeps safe event/status/count and synthetic material digest only.',
   limitation='Independent audit replays archived strict metadata and raw native transcript. It does not reconstruct redacted secret DTOs, measure kernel close timing or rerun product tests.')
 except Waiting as error:
  print(json.dumps(dict(status='WAIT',reasonCode=str(error),reportCreated=False)));return 3
 except Exception as error:
  out['error']=dict(kind=type(error).__name__,failureCode=str(error) if isinstance(error,AssertionError) else 'INPUT_OR_FROZEN_VALIDATOR_CHECK_FAILED')
 data=(json.dumps(out,ensure_ascii=False,indent=2)+'\n').encode()
 with a.output.open('xb') as stream:stream.write(data)
 print(json.dumps(dict(status=out['status'],report=str(a.output),sha256=sha(data))))
 return 0 if out['status']=='PASS_FINAL_FROZEN_ORDINARY31_FINITE_WIN11_ONLY' else 1
if __name__=='__main__':raise SystemExit(main())
