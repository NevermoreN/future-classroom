from pathlib import Path
w=Path('/tmp/fc-round68-production-https/worktree')
p=w/'crates/fc-bootstrap/src/application/https_runtime_controller.rs';s=p.read_text()
s=s.replace('''                || self
                    .teacher_https
                    .lease''','''                || self.teacher_https.prepare_deadline.is_some_and(|deadline| Instant::now() >= deadline)
                || self
                    .teacher_https
                    .lease''',1)
s=s.replace('''            if readiness == web_editor::Readiness::Faulted && !self.teacher_https.quiescent() {
                self.teacher_https.stop();
            }''','''            if readiness == web_editor::Readiness::Faulted && !self.teacher_https.quiescent() {
                self.teacher_https.stop();
            }
            if status.error == Some(runtime::RuntimeError::StopTimeout) {
                // The HTTP domain's original stop interval already elapsed.
                // Do not grant another interval when the UI observes Faulted.
                self.force_teacher_https_exit(Instant::now());
            }''',1)
s=s.replace('''            self.set_web_readiness(web_editor::Readiness::Faulted);
            self.terminate_required = true;
            self.begin_shutdown();''','''            let expired = self.teacher_https.stop_deadline.expect("observed original stop deadline");
            self.force_teacher_https_exit(expired);''',1)
s=s.replace('''    pub(super) fn poll_teacher_https(&mut self) -> std::result::Result<(), String> {''','''    fn force_teacher_https_exit(&mut self, expired: Instant) {
        self.set_web_readiness(web_editor::Readiness::Faulted);
        self.terminate_required = true;
        self.begin_shutdown();
        // begin_shutdown also coordinates unrelated owners, but may not renew
        // the already expired network budget. Actual exit retains all owners.
        self.shutdown_deadline = Some(self.shutdown_deadline.map_or(expired, |d| d.min(expired)));
    }
    pub(super) fn poll_teacher_https(&mut self) -> std::result::Result<(), String> {''',1)
p.write_text(s)
p=w/'scripts/windows-ordinary-https-probe.py';s=p.read_text()
s=s.replace('''    fields = text.split("\\n")
    require(len(fields)''','''    require(time.monotonic() < deadline, "owned pairing read before original deadline")
    return parse_pairing_secret(text, marker, generation)


def parse_pairing_secret(text, marker, generation):
    fields = text.split("\\n")
    require(len(fields)''',1)
# Record explicit trust facts without certificate or authentication secrets.
s=s.replace('''        context = tls.context(pem)
        tunnel''','''        context = tls.context(pem)
        require(context.check_hostname and context.verify_mode == ssl.CERT_REQUIRED, "strict IP-SAN verification")
        report["clientVerification"]={"checkHostname":True,"verifyMode":"CERT_REQUIRED","serverName":"127.0.0.1",
            "minimumTls":"TLSv1.2","maximumTls":"TLSv1.2","peerCertificate":"EXACT_OWNED_DER",
            "trustSource":"only owned public PEM","cleanTlsEof":"REQUIRED","systemTrust":"UNCHANGED"}
        tunnel''',1)
# Protect only the newly owned fixture directory, before writing any sidechannel.
s=s.replace(''';New-Item -ItemType Directory -Path $r|Out-Null;[IO.File]::WriteAllText((Join-Path $r '.tls-probe-root'),''',''';New-Item -ItemType Directory -Path $r|Out-Null;$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User;$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetOwner($sid);$acl.SetAccessRuleProtection($true,$false);$rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,[Security.AccessControl.FileSystemRights]::FullControl,([Security.AccessControl.InheritanceFlags]::ContainerInherit-bor [Security.AccessControl.InheritanceFlags]::ObjectInherit),[Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow);$acl.AddAccessRule($rule);Set-Acl -LiteralPath $r -AclObject $acl;if(!(Get-Acl -LiteralPath $r).AreAccessRulesProtected){throw 'owned root ACL not protected'};[IO.File]::WriteAllText((Join-Path $r '.tls-probe-root'),''',1)
needle='''    print(json.dumps({"status": "PASS", "scope": "pure validators only", "checks": 20,'''
replace='''    public_pair = {"cycle":1,"marker":marker,"port":23456,"generation":17,"origin":"https://127.0.0.1:23456"}
    validate_pair_ready(public_pair,marker,23456,17)
    for invalid in [dict(public_pair,pairingCode="00000000"),dict(public_pair,generation=18),dict(public_pair,generation=True),
                    dict(public_pair,marker=marker+"x"),dict(public_pair,cycle=2),dict(public_pair,origin="https://localhost:23456")]:
        try:
            validate_pair_ready(invalid,marker,23456,17)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("secret-bearing/foreign pairing rendezvous accepted")
    secret = f"FC_ORDINARY_PAIR_V1\\n{marker}\\n{17:020}\\n00000000\\nsynthetic-voice-settings-admin\\n"
    require(parse_pairing_secret(secret,marker,17)==("00000000","synthetic-voice-settings-admin"), "pure synthetic secret channel parser")
    for invalid in [secret+"x",secret.replace(f"{17:020}",f"{18:020}"),secret.replace("00000000\\nsynthetic", "not-code\\nsynthetic"),
                    secret.replace(marker,marker+"x"),secret.replace("FC_ORDINARY_PAIR_V1","unrecognized"),secret.replace("synthetic-voice-settings-admin","foreign")]:
        try:
            parse_pairing_secret(invalid,marker,17)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("foreign/unbounded/malformed secret channel accepted")
    print(json.dumps({"status": "PASS", "scope": "pure validators only", "checks": 34,'''
assert needle in s;s=s.replace(needle,replace,1)
p.write_text(s)
