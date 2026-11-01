from pathlib import Path
r=Path('/tmp/fc-round68-production-https/worktree');e=r.parent/'evidence'
for rel in ['crates/fc-bootstrap/src/backend/account_management_tests.rs','crates/fc-bootstrap/src/backend/profiles/tests.rs']:
 p=r/rel;b=p.read_bytes();out=e/'b-before-fifth'/rel;out.parent.mkdir(parents=True,exist_ok=True)
 with out.open('xb') as f:f.write(b)
 s=b.decode();old='''        fixture.seed(
            Some(admin),''';assert s.count(old)==3
 s=s.replace(old,'''        // The preceding synthetic local login may have revoked the old admin.
        let admin = fixture.issue(1, TrustedOrigin::Local);
        fixture.seed(
            Some(admin),''')
 old='''            let lease = self.issue(principal.get(), TrustedOrigin::Web);''';assert s.count(old)==1
 s=s.replace(old,'''            // Initial password change is a local component operation. No Web
            // pairing/listener readiness is fabricated to complete this seed.
            let lease = self.issue(principal.get(), TrustedOrigin::Local);''')
 p.write_text(s)
