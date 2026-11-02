#!/usr/bin/env python3
"""Root-owned composition adapter. No network/Edge is opened by importing this file.
Teacher pairing code stays private stdin. No cookie, CSRF or approved auth injection.
No replacement native actor: fixed phase callback must perform real native actions.
"""
import argparse,base64,hashlib,importlib.util,json,os,queue,re,stat,subprocess,sys,time,uuid
from pathlib import Path
sys.dont_write_bytecode=True
MAX_INPUT=65536

def digest(raw):return hashlib.sha256(raw).hexdigest()
def require(ok,code):
    if not ok:raise RuntimeError(code)
def load(path,name):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module
EXPECTED_LEGACY_SHA='03c1118a01ff7c61c0eaf3103c3d399e5b5a9127933db6eb2c33d821cc35f38c'

def observer_class(support_root):
    root=Path(support_root).resolve();path=root/'windows-native-browser-observer.py'
    require(path.is_file() and not path.is_symlink() and digest(path.read_bytes())==EXPECTED_LEGACY_SHA,'OBSERVER_SUPPORT_PIN')
    expected = {'windows-native-browser-observer.py': '03c1118a01ff7c61c0eaf3103c3d399e5b5a9127933db6eb2c33d821cc35f38c', 'windows-test-edge-owner.py': '8687830a4db9fe5a336949797ff58a0534097569680172ae55cf11c915817009', 'windows-tls-probe.py': '1a91ac72ccb3fff0a08d9aa98222a82019af86a8d1b903e9fb10eb69ad8d94df', 'windows-probe.py': 'f176a022bf90565be7074f53a2d502d6519afd1748cdd2022f608a217d485c14'}
    for name,sha in expected.items():
        support=root/name
        require(support.is_file() and not support.is_symlink() and digest(support.read_bytes())==sha,'FROZEN_EXECUTABLE_SUPPORT_CLOSURE')
    global legacy
    legacy=load(path,'g03_bounded_transport_legacy')
    return legacy

ACTIONS=('CACHE_UPGRADE','VOICE_SETTLED','MIDNIGHT','REVOKE','COMPLETE')
def decode_pairing(raw,marker,cycle):
    require(type(raw) is bytes and len(raw)<=256 and re.fullmatch(r'FutureClassroom-Test-TLS-[0-9a-f]{32}',marker),'PRIVATE_PAIR_BOUND')
    parts=raw.decode('ascii').splitlines()
    require(len(parts)==5 and parts[:3]==['G03_PAIR_V1',marker,str(cycle)] and parts[4]=='synthetic-ordinary-teacher' and re.fullmatch(r'[0-9]{8}',parts[3]),'ORIGINAL_NATIVE_PAIR_BYTES')
    return parts[3]
def action_request(marker,index,action):
    require(re.fullmatch(r'FutureClassroom-Test-TLS-[0-9a-f]{32}',marker) and type(index) is int and 0<=index<len(ACTIONS) and ACTIONS[index]==action,'FIXED_ACTION_ORDER')
    return ('G03_ACTION_V1\n'+marker+'\n'+str(index)+'\n'+action+'\n').encode('ascii')
def decode_ack(raw,marker,index):
    require(type(raw) is bytes and len(raw)<=1024 and 0<=index<len(ACTIONS),'ACK_BOUND')
    value=json.loads(raw)
    require(value.get('type')=='ACK' and value.get('marker')==marker and value.get('action')==ACTIONS[index] and value.get('status')=='PASS','EXACT_ORIGINAL_NATIVE_ACK')
    required={'CACHE_UPGRADE':{'oldOwnerDrained','sameOrigin','pairingCode'},'VOICE_SETTLED':{'activeJobs','audioOwnerReturned'},'MIDNIGHT':{'clockScope','oldDate','newDate','osClockChanged'},'REVOKE':{'actualIdentityWrite','teacherLogin'},'COMPLETE':{'nativeHttpsDrained'}}
    require(set(value)=={'type','marker','action','status'}|required[ACTIONS[index]],'ACK_NO_ADDITIONAL_DATA')
    return value

class _TeacherMethods:
    def __init__(self,remote,local_root,report_path,marker,old_manifest,new_manifest,expected_csp,identity,ssh_target,support_root):
        legacy.BrowserObserver.__init__(self,remote,local_root,report_path,marker,old_manifest,expected_csp,identity,ssh_target)
        self.support_root=Path(support_root).resolve();self.new_manifest=new_manifest
        self.report['credentialScope']='Actual DOM pairing; short-lived code private stdin; no injected cookie/CSRF'
    def prepare(self, public_pem, deadline):
        require(not self.started and not self.closed, "OBSERVER_ALREADY_STARTED")
        self.started = True
        require(type(deadline) in (int,float) and deadline>time.monotonic(),"ORIGINAL_NATIVE_DEADLINE_REQUIRED")
        self.native_deadline=deadline
        deadline = min(deadline,time.monotonic() + 60)
        phase = "prepare-public-spki"
        try:
            require(re.fullmatch(r"FutureClassroom-Test-TLS-[0-9a-f]{32}", self.marker)
                    and self.remote.host == self.ssh_target, "BROWSER_COMPOSITION")
            pem = public_pem.encode("ascii") if isinstance(public_pem, str) else bytes(public_pem)
            require(len(pem) <= MAX_INPUT and pem.startswith(b"-----BEGIN CERTIFICATE-----"), "PUBLIC_PEM_BOUND")
            public = subprocess.run(["openssl", "x509", "-pubkey", "-noout"], input=pem,
                                    capture_output=True, timeout=min(5, self._remaining(deadline)))
            require(public.returncode == 0 and len(public.stdout) <= MAX_INPUT, "PUBLIC_KEY_EXTRACTION")
            der = subprocess.run(["openssl", "pkey", "-pubin", "-outform", "DER"], input=public.stdout,
                                 capture_output=True, timeout=min(5, self._remaining(deadline)))
            require(der.returncode == 0 and 0 < len(der.stdout) <= MAX_INPUT, "PUBLIC_SPKI_EXTRACTION")
            spki = base64.b64encode(hashlib.sha256(der.stdout).digest()).decode("ascii")
            self.helper = legacy.load_helpers()
            directory = self.support_root
            owner_source = Path(__file__).resolve().with_name("windows-teacher-edge-owner.py")
            node_source = Path(__file__).resolve().with_name("windows-teacher-browser-harness.mjs")
            self._node_source_sha = digest(node_source.read_bytes())
            self.report["sourceSha256"] = {
                "observer": digest(Path(__file__).read_bytes()), "edgeOwner": digest(owner_source.read_bytes()),
                "nodeHarness": self._node_source_sha, "tlsOwnershipHelpers": digest((directory / "windows-tls-probe.py").read_bytes()),
            }
            self.report["spkiSha256Base64"] = spki
            prefix = self.report_path.resolve()
            prefix.parent.mkdir(parents=True, exist_ok=True)
            self.edge_report = prefix.with_name(prefix.name + ".browser-edge-owner.json")
            self.node_report = prefix.with_name(prefix.name + ".browser-node.json")
            self.control_file = self.edge_report.with_name(self.edge_report.name + ".control.json")
            require(not any(p.exists() for p in (self.edge_report, self.node_report, self.control_file)),
                    "BROWSER_ARTIFACT_ALREADY_EXISTS")
            self.report["ownerReportPath"], self.report["nodeReportPath"] = str(self.edge_report), str(self.node_report)
            phase = "prepare-original-edge-owner"
            command = [sys.executable, "-B", str(owner_source), "--execute", "run", "--project-root", str(self.support_root), "--spki", spki,
                       "--report", str(self.edge_report), "--ssh-target", self.ssh_target]
            if self.identity:
                command += ["--identity", str(self.identity)]
            self.owner = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.owner_logs = self.helper.Logs(self.owner)
            controlled = False
            while True:
                line = self._line(self.owner, self.owner_logs, deadline)
                if line.startswith("EDGE_CACHE_CONTROL "):
                    value = json.loads(line[len("EDGE_CACHE_CONTROL "):])
                    require(not controlled and Path(value["controlFile"]) == self.control_file
                            and self._read_control() and value.get("marker") == self.owner_marker,
                            "OWNER_CONTROL_PROTOCOL")
                    controlled = True
                elif line.startswith("EDGE_CACHE_READY "):
                    value = json.loads(line[len("EDGE_CACHE_READY "):])
                    require(controlled and value.get("marker") == self.owner_marker
                            and type(value.get("port")) is int and 1 <= value["port"] <= 65535
                            and re.fullmatch(r"/devtools/browser/[0-9a-fA-F-]{16,80}", value.get("webSocketPath", ""))
                            and value.get("boundLoopback") is True, "OWNER_READY_PROTOCOL")
                    break
            self.report["edgeReady"] = value
            phase = "prepare-independent-cdp-tunnel"
            self.cdp_root = self.local_root / ("browser-cdp-" + uuid.uuid4().hex[:12])
            self.cdp_root.mkdir(mode=0o700, parents=False, exist_ok=False)
            require(self._remaining(deadline) > 12, "CDP_PREPARE_BUDGET")
            self.tunnel = self.helper.Tunnel(self.remote, self.cdp_root)
            port = self.tunnel.open(value["port"])
            cdp_url = "ws://127.0.0.1:" + str(port) + value["webSocketPath"]
            self._secrets.append(cdp_url.encode())
            self.cdp_url=cdp_url
            self.prepared=True
            self.report["status"]="PREPARED"
        except Exception as error:
            self.report["status"]="FAIL"
            self.report["errors"].append(legacy.failure(error,phase))
            raise RuntimeError("TEACHER_BROWSER_PREPARE_FAILED") from None

    def run_teacher(self,init,native_phase,deadline):
        require(self.prepared and not self.closed and not self.result,'OBSERVER_RUN_STATE')
        require(deadline<=self.native_deadline,'NO_NATIVE_DEADLINE_RENEWAL')
        maximum=min(120000,int((deadline-time.monotonic()-2)*1000))
        require(maximum>0,'ORIGINAL_NATIVE_DEADLINE')
        source=Path(__file__).with_name('windows-teacher-browser-harness.mjs')
        require(digest(source.read_bytes())==self._node_source_sha,'ORIGINAL_NODE_SOURCE_CHANGED')
        self.node=subprocess.Popen(['node',str(source),'--execute'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        self.node_logs=self.helper.Logs(self.node)
        require(self._read_control(),'ORIGINAL_OWNER_CONTROL_REQUIRED')
        state=json.loads(self.control_file.read_bytes())
        self.material_expected={'bytes':init['materialBytes'],'sha256':init['materialSha256']}
        message={**init,'browserOwnerMarker':state['marker'],'downloadPath':state['remoteRoot']+'\\downloads','type':'INIT','cdpUrl':self.cdp_url,'marker':self.marker,'oldManifest':self.manifest,'newManifest':self.new_manifest,'expectedCsp':self.expected_csp,'maxMillis':maximum}
        require(set(message)=={'type','cdpUrl','marker','origin','maxMillis','pairingCode','loginName','materialPath','materialName','materialBytes','materialSha256','browserOwnerMarker','downloadPath','oldManifest','newManifest','expectedCsp'},'PRIVATE_INPUT_SHAPE')
        require(re.fullmatch(r'[0-9]{8}',message['pairingCode']) is not None,'PAIR_CODE')
        # Do not log raw code even on a failed helper. It is never persisted in the report.
        self._secrets.append(message['pairingCode'].encode())
        self._send(message,deadline)
        order=['CACHE_UPGRADE','VOICE_SETTLED','MIDNIGHT','REVOKE','COMPLETE'];index=0
        while True:
            line=self._line(self.node,self.node_logs,deadline);self._safe(line.encode());value=json.loads(line)
            if value.get('type')=='RESULT':
                self.result=value['report'];break
            require(value=={'type':'NATIVE','action':order[index] if index<len(order) else None,'marker':self.marker},'FIXED_NATIVE_RENDEZVOUS')
            ack=native_phase(value['action'],deadline)
            require(type(ack) is dict and ack.get('action')==value['action'] and ack.get('status')=='PASS','ACTUAL_NATIVE_PHASE_REQUIRED')
            if ack.get('pairingCode'):
                require(re.fullmatch(r'[0-9]{8}',ack['pairingCode']) is not None,'SECOND_PAIR_CODE')
                self._secrets.append(ack['pairingCode'].encode())
            self._send({**ack,'type':'ACK','marker':self.marker},deadline);index+=1
        self.node.stdin.close();self.node.wait(timeout=self._remaining(deadline));self.report['nodeActualExitCode']=self.node.returncode
        self._node_raw()
        # Preserve original FAIL without synthesizing a new browser result.
        raw=json.dumps(self.result,ensure_ascii=False,indent=2).encode()+b'\n';self._safe(raw)
        fd=os.open(self.node_report,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
        with os.fdopen(fd,'wb') as f:f.write(raw)
        if self.result.get('status')!='PASS_TEACHER_DOM_FINITE_WIN11_ONLY':
            self.report.update(status='FAIL',result=self.result)
            code=self.result.get('error',{}).get('code','UNKNOWN_DOM_FAILURE')
            require(type(code) is str and re.fullmatch(r'[A-Z0-9_]{1,80}',code),'CLOSED_DOM_FAILURE_CODE')
            raise RuntimeError('TEACHER_DOM_FAILED_'+code) from None
        validator=load(Path(__file__).with_name('verify-teacher-browser-report.py'),'g03_report_validator')
        validator.validate(self.result,self.manifest,self.new_manifest)
        require(self.node.returncode==0 and index==len(order),'NODE_ACTUAL_EXIT_OR_PHASES')
        self.report.update(status='PASS',result=self.result)
        return self.result

    def close(self):
        report=legacy.BrowserObserver.close(self)
        if self.result.get('status')=='PASS_TEACHER_DOM_FINITE_WIN11_ONLY':
            try:
                original,_identity=self._file(self.edge_report,limit=16*1024*1024)
                files=original.get('rawPreservation',{}).get('files',{})
                names=[name for name in files if name.startswith('downloads'+chr(92))]
                expected='downloads'+chr(92)+self.result['download']['guid']
                require(names==[expected] and files[expected].get('status')=='PASS' and all(files[expected].get(k)==v for k,v in self.material_expected.items()),'ACTUAL_BROWSER_FILE_DOWNLOADED_BYTE_EQUAL_AFTER_JOB_EXIT')
                self.report['materialBrowserFileVerifiedAfterPhysicalExit']=True
            except Exception:
                self.report.update(status='FAIL',materialBrowserFileVerifiedAfterPhysicalExit=False)
                raise RuntimeError('BROWSER_DOWNLOADED_ORIGINAL_FILE_MISMATCH') from None
        return report

# Names referenced by copied prepare code are closed aliases, no executable input.
# They are initialized only when observer_class checks and imports the pinned support.
def make_observer(support_root,*args,**kwargs):
    observer_class(support_root)
    globals().update(require=legacy.require,digest=legacy.digest,failure=legacy.failure,MAX_INPUT=legacy.MAX_INPUT)
    cls=type('TeacherBrowserObserver',(_TeacherMethods,legacy.BrowserObserver),{})
    return cls(*args,support_root=support_root,**kwargs)

if __name__=='__main__':
    print(json.dumps({'status':'NOT_RUN','mode':'ROOT_COMPOSITION_API_ONLY','windows':'NOT_RUN'}))
