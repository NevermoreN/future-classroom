#!/usr/bin/env python3
"""Independent finite frozen-source review and pure replay; never runs Windows."""
from pathlib import Path
import ast,datetime,hashlib,importlib.util,json,re,subprocess,sys,time
sys.dont_write_bytecode=True
R=Path('/tmp/fc-round70-ordinary-transfer-stop');E=R/'evidence';W=R/'worktree'
sha=lambda data:hashlib.sha256(data).hexdigest()
def main():
 freeze=E/'implementation-freeze-first.json';assert sha(freeze.read_bytes())=='a473602e8c189d2f198b2e6195f687590ead600b7951454a7c81ecf398cb37b6'
 frozen=json.loads(freeze.read_bytes());facts=[]
 for row in frozen['source']:
  p=W/row['relativePath'];data=p.read_bytes();assert sha(data)==row['after']['sha256'] and len(data)==row['after']['bytes']
  facts.append(dict(path=row['relativePath'],bytes=len(data),sha256=sha(data)))
 runner=W/'scripts/windows-ordinary-https-probe.py';command=['python3','-B',str(runner),'--self-test']
 started=time.monotonic();result=subprocess.run(command,cwd=W,capture_output=True,timeout=60)
 streams={}
 for name,data in [('stdout',result.stdout),('stderr',result.stderr)]:
  p=E/('independent-probe-pure-first.'+name+'.log')
  with p.open('xb') as output:output.write(data)
  streams[name]=dict(path=p.name,bytes=len(data),sha256=sha(data))
 assert result.returncode==0;pure=json.loads(result.stdout);assert pure['status']=='PASS'
 spec=importlib.util.spec_from_file_location('r70_independent_source_probe',runner);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
 # Invoke the ACTUAL frozen rendezvous producer with a no-I/O remote adapter.
 # The only IDs/marker below are synthetic literals, not runtime credentials.
 class LocalFake:
  def __init__(self):self.script=None
  def powershell(self,script,timeout):self.script=script;return 'OWNED_CLIENT_JOURNEY_DONE'
 remote=LocalFake();marker='FutureClassroom-Test-TLS-'+'0'*32
 m.signal_business_done(remote,'C:\\synthetic',marker,17,time.monotonic()+5,1,['1','2'])
 string_fields=[format(v,'020') for v in ('1','2')];canonical=[format(int(v),'020d') for v in ('1','2')]
 assert string_fields!=canonical and all(v in remote.script for v in string_fields)
 assert int(string_fields[0])!=1 and int(string_fields[1])>2**64-1
 native=(W/'crates/fc-bootstrap/src/application/ordinary_https_probe.rs').read_text()
 order=re.findall(r'"([a-z-]+)"',native.split('const ORDER: [&str; 31] = [',1)[1].split('];',1)[0]);assert tuple(order)==m.STEPS
 for row in facts:assert sha((W/row['path']).read_bytes())==row['sha256']
 report=dict(schemaVersion=1,status='BLOCKED_P1_FROZEN_PROBE_RENDEZVOUS_ID_ENCODING',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
  implementationFreeze=dict(path=freeze.name,sha256=sha(freeze.read_bytes())),source=facts,sourceUnchanged=True,
  pureReplay=dict(command=command,exitCode=result.returncode,elapsedSeconds=time.monotonic()-started,actualResult=pure,streams=streams,
   qualification='All166 existing pure checks pass, but they do not invoke signal_business_done with actual string upload IDs; this is not Windows or transfer execution proof.'),
  finding=dict(priority='P1',file='scripts/windows-ordinary-https-probe.py',line=340,function='signal_business_done',
   problem='validate_upload/create_material produce str IDs. f-string format020 on str right-pads zeroes, whereas native completion expects20digit canonical numeric IDs.',
   actualPureReproduction=dict(producerInvokedWithNoIoRemote=True,syntheticInputIds=['1','2'],actualStringFormattedFields=string_fields,requiredNumericFields=canonical,
    firstFieldChangesNumericId=True,secondFieldExceedsU64=True),
   consequence='Native completion reads a different stage ID or rejects u64 overflow, so the frozen first-generation actual31-step workflow cannot reach intended stage/stop validation.',
   minimalCorrection='After the existing bounded decimal ID validation, convert each ID with int and use020d numeric formatting. Add a pure actual producer/consumer roundtrip for string IDs.',
   sourceNotModified=True),
  otherReviewedContracts=[
   '31 native/Python ordered step names agree. Only initialUnavailable->Disabled fixture readiness is assigned; actual Running is observed from ordinary RuntimeOwner.',
   'FullA UPLOADED response maps to production UploadHub Stage::MaterialReady; both runner and native check its exact65536-byte original stage before Disable. PartialB sends65536 of declared1048576 and observes real stage consumption.',
   'PartialB original TLS socket and forwarding domain remain owned through first native STOPPED. Runner cancels that forward only after observing peer terminal; error-finally cleanup does not promote PASS.',
   'Native stop observes one Instant captured immediately before CONFIRM and caps submit/wait/elapsed at original5s; production State passes its own earliest observation to request_stop_at and RuntimeOwner retains min original stop_at. No phase-specific renewed stop budget.',
   'First stage deletion occurs only after teacher_https_quiescent and original network counters/nativeDrained; original App pool is neither recreated nor closed, and actual OpenWeb query is pumped before second enable.',
   'Second generation checks old cookie401 with exact unauthorized body, then opens native pairing again, approves actual selected teacher request and obtains fresh cookie/CSRF in memory.',
   'C andD both receive complete UPLOADED receipts before appendC or cancelD, so the intended slot-reuse proof is concurrent MaterialReady occupancy, not sequential reuse. Typed command69001 commits, exact retries and GET readback compare in memory; D is canceled.',
   'Real teacher credentials are random native fixtures. Code sidechannels use memory-only nonrecorded SSH read and exact removal; request headers/bodies containing cookie/CSRF/pollSecret are not placed in event reports. Only fixed synthetic MATERIAL digest plus public artifacts/status digests enter reports, not credential digests.',
   'Completion marker only coordinates an already completed client journey. It supplies canonical ID/generation identity to native checks; it is not RuntimeReady, authentication or durable commit evidence.'
  ],
  limits=['This report concerns frozen firstsource only. The P1 must be fixed and newly frozen before actual workflow acceptance.',
   'No Rust typecheck/link, actual TLS, native31 runtime, Windows/SSH, network, Cargo, Git mutation or main change performed.',
   'Partial peer terminal sample is read after native stopped receipt; no independent per-channel kernel-close timing or arbitrary scheduler guarantee is claimed.',
   'Published material bytes/hash and transport/staging lifetime remain intended runtime assertions until an actual complete terminal run. Win7/10/campus remain separate.'])
 data=(json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode();p=E/'independent-probe-source-review-first.json'
 with p.open('xb') as output:output.write(data)
 print(json.dumps(dict(status=report['status'],reportSha256=sha(data),pureChecks=pure['checks'],sourceUnchanged=True,windowsInvoked=False)))
if __name__=='__main__':main()
