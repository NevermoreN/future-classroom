from pathlib import Path
import hashlib
E=Path('/tmp/fc-round61-gui/evidence');old=E/'verify-ci-artifact-gui-first.py';new=E/'verify-ci-artifact-gui-second.py'
b=old.read_bytes();assert hashlib.sha256(b).hexdigest()=='7d67265ae31a6684fe87d026a5bedd70fd8be171d3308d3b817c10f332b2c188';assert not new.exists();s=b.decode()
def rep(a,b):
 global s
 assert s.count(a)==1,(a[:100],s.count(a));s=s.replace(a,b)
s=s.replace('verify-ci-artifact-gui-first.py','verify-ci-artifact-gui-second.py')
rep("CI_SHA256 = '43f6bc63800b36162d64285a77913d33a2aa30f0f34c07c9349f6836642813d3'","CI_SHA256 = 'eac2e9e42cfd073aca58584e8dbc6d9cefa864a5b84711b27e0ab83d43689a7b'")
rep("    return result + ['--','-C','link-arg=/SUBSYSTEM:WINDOWS,6.01'] if command == 'rustc' else result",'    return result')
rep('def required_commands(baseline):', '''def product_rustflags(baseline):
    return ('-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01'
        if baseline == 'Win7' else BASELINES[baseline]['flags'])

def compiler_flag_phases(baseline):
    default = BASELINES[baseline]['flags']
    return {'before-vs':default, 'after-vs':default, 'before-cargo':default,
        'before-product':product_rustflags(baseline), 'after-product':default}

def required_commands(baseline):''')
a="        self.check('start fixed flags/role/no runtime',start.get('rustflags')==c['flags'] and start.get('role')==c['role'] and start.get('tests')=='COMPILE_ONLY_NO_RUN')"
b=a+'''
        for obj in [start,r]:
            self.check('actual default versus product-only environment metadata '+('start' if obj is start else 'result'),
                obj.get('rustflags')==c['flags'] and obj.get('product_rustflags')==product_rustflags(a.baseline)
                and obj.get('rustflags_scope')=='Workspace test/clippy default; product-compile uses product_rustflags only and restores the original flags.')'''
rep(a,b)
rep("        for phase in ['before-vs','after-vs','before-cargo']:","        for phase,expected_flags in compiler_flag_phases(a.baseline).items():")
rep("rf.get('value')==c['flags'] and rf.get('length')==len(c['flags'])", "rf.get('value')==expected_flags and rf.get('length')==len(expected_flags)")
rep("expected=(['rustc']+args if baseline=='Modern' else ['-Z','build-std','rustc']+args+['--','-C','link-arg=/SUBSYSTEM:WINDOWS,6.01'])", "expected=(['rustc']+args if baseline=='Modern' else ['-Z','build-std','rustc']+args)")
a="    test('baseline exact buildstd placement/no metadata fmt leakage',arguments)"
b=a+'''
    def product_environment():
        for baseline in ['Modern','Win7']:
            phases=compiler_flag_phases(baseline)
            need(set(phases)=={'before-vs','after-vs','before-cargo','before-product','after-product'},'MODEL_FLAG_PHASES')
            need(phases['before-cargo']==phases['after-product']==BASELINES[baseline]['flags'],'MODEL_PRODUCT_RESTORE')
            expected=('-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01' if baseline=='Win7' else '-C target-feature=+crt-static')
            need(phases['before-product']==expected,'MODEL_PRODUCT_ENVIRONMENT')
            if baseline=='Win7':
                need(phases['before-product']!=phases['after-product'],'MODEL_ENV_GUI_MISSING_OR_LEAKED')
                need('link-arg=/SUBSYSTEM:WINDOWS,6.01' not in required_commands(baseline)['product-compile'],'MODEL_FINAL_TAIL_RELIES_ON_ORDER')
    test('product-only GUI environment and restored default phases are distinct; no linker claim',product_environment)'''
rep(a,b)
rep("before=(EV/'bootstrap-ci-before-first/scripts/windows-msvc-ci.ps1').read_bytes()", "before=(EV/'bootstrap-ci-before-second/scripts/windows-msvc-ci.ps1').read_bytes()")
rep("after=(EV/'bootstrap-ci-after-first/scripts/windows-msvc-ci.ps1').read_bytes()", "after=(EV/'bootstrap-ci-after-second/scripts/windows-msvc-ci.ps1').read_bytes()")
rep("need(digest(before)=='0cef8840cd51b1383e252b08df14b5c19f8dea9c3903583f97a4e6d4e5f365bd','MODEL_OLD_CI_ORIGINAL_CHANGED')", "need(digest(before)=='43f6bc63800b36162d64285a77913d33a2aa30f0f34c07c9349f6836642813d3','MODEL_OLD_CI_ORIGINAL_CHANGED')")
rep("need(CI_SHA256=='43f6bc63800b36162d64285a77913d33a2aa30f0f34c07c9349f6836642813d3' and ci_source_matches(after),'MODEL_NEW_CI_PIN')", "need(CI_SHA256=='eac2e9e42cfd073aca58584e8dbc6d9cefa864a5b84711b27e0ab83d43689a7b' and ci_source_matches(after),'MODEL_NEW_CI_PIN')")
new.write_bytes(s.encode());assert old.read_bytes()==b
print(hashlib.sha256(new.read_bytes()).hexdigest())
