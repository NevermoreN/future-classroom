from pathlib import Path
import hashlib
src=Path('/tmp/fc-round59-ci-evidence/verify-ci-artifact-first.py')
dst=Path('/tmp/fc-round61-gui/evidence/verify-ci-artifact-gui-first.py')
original=src.read_bytes()
assert hashlib.sha256(original).hexdigest()=='de7d52d7ca2c04a72b4dcd4de727fd1b1b2882259024e5bbc8b184007679fbb9'
assert not dst.exists()
s=original.decode()
def replace(old,new):
 global s
 assert s.count(old)==1,(old[:100],s.count(old))
 s=s.replace(old,new)
replace('R57 finite 20,000-source-path original-artifact audit.', 'R61 GUI-product/CUI-test finite 20,000-source-path original-artifact audit.')
s=s.replace('verify-ci-artifact-expanded-first.py','verify-ci-artifact-gui-first.py')
replace("CI_SHA256 = '0cef8840cd51b1383e252b08df14b5c19f8dea9c3903583f97a4e6d4e5f365bd'", "CI_SHA256 = '43f6bc63800b36162d64285a77913d33a2aa30f0f34c07c9349f6836642813d3'")
replace("    return ([command, '-Z', 'build-std'] if command == 'clippy' else ['-Z', 'build-std', command]) + args", "    result = ([command, '-Z', 'build-std'] if command == 'clippy' else ['-Z', 'build-std', command]) + args\n    return result + ['--','-C','link-arg=/SUBSYSTEM:WINDOWS,6.01'] if command == 'rustc' else result")
replace("'product-compile': cargo_arguments('build'", "'product-compile': cargo_arguments('rustc'")
replace('def pe_header(data, baseline):', '''def expected_subsystem(role):
    need(role in ('product', 'tests'), 'INVALID_PE_ROLE')
    return 2 if role == 'product' else 3

def producer_subsystem_matches(row, baseline):
    return (row.get('role') in ('product', 'tests')
        and type(row.get('pe_subsystem')) is int
        and row['pe_subsystem'] == expected_subsystem(row['role'])
        and row.get('pe_subsystem_version') == ('6.1' if baseline == 'Win7' else None))

def pe_header(data, baseline, role):''')
replace("    need(struct.unpack_from('<H', data, opt)[0] == 0x10b and struct.unpack_from('<H', data, opt+68)[0] == 3, 'PE32_CUI')", "    subsystem = expected_subsystem(role)\n    need(struct.unpack_from('<H', data, opt)[0] == 0x10b, 'PE32_MAGIC')\n    need(struct.unpack_from('<H', data, opt+68)[0] == subsystem, 'PE_ROLE_SUBSYSTEM')")
replace("return dict(machine='0x014c', optionalMagic='0x010b', subsystem=3, subsystemVersion=version)","return dict(machine='0x014c', optionalMagic='0x010b', subsystem=subsystem, subsystemVersion=version)")
replace("self.check('exact R57 CI helper pinned source'", "self.check('exact R61 GUI CI helper pinned source'")
replace("self.check('producer baseline subsystem claim '+name,row.get('pe_subsystem_version')==('6.1' if self.args.baseline=='Win7' else None))", "self.check('producer exact role and baseline subsystem claim '+name,producer_subsystem_matches(row,self.args.baseline))")
replace('header=pe_header(data,self.args.baseline)',"header=pe_header(data,self.args.baseline,row['role'])")
replace("parsed=parse(inspect.stdout.decode('utf-8'))", "parsed=parse(inspect.stdout.decode('utf-8'), 'gui' if row['role']=='product' else 'console')")
replace('''        need('--no-run' in required_commands('Modern')['workspace-test-compile'],'MODEL_NO_RUN')''','''        need('--no-run' in required_commands('Modern')['workspace-test-compile'],'MODEL_NO_RUN')
        for baseline in ['Modern','Win7']:
            target=BASELINES[baseline]['target']
            args=['-vv','--release','--locked','--target',target,'-p','fc-bootstrap','--bin','fc-bootstrap','--message-format=json']
            expected=(['rustc']+args if baseline=='Modern' else ['-Z','build-std','rustc']+args+['--','-C','link-arg=/SUBSYSTEM:WINDOWS,6.01'])
            need(required_commands(baseline)['product-compile']==expected,'MODEL_PRODUCT_RUSTC_FINAL_ARGS')
            need('link-arg=/SUBSYSTEM:WINDOWS,6.01' not in required_commands(baseline)['workspace-test-compile']
                and 'link-arg=/SUBSYSTEM:WINDOWS,6.01' not in required_commands(baseline)['clippy'],'MODEL_GUI_LEAKS_TO_TESTS')''')
replace('''        # The R57 immutable before/after fixtures are real original bytes. This
        # checks the SAME predicate as Audit.sources, not an invented hash map.
        before=(EV/'windows-msvc-ci.before.ps1').read_bytes()
        after=(EV/'implementation-after-first/scripts/windows-msvc-ci.ps1').read_bytes()
        need(digest(before)=='ed26c9932325d377bbbf4eaf63aad29d75fc0cc8ad77561b1121f3389024c418','MODEL_OLD_CI_ORIGINAL_CHANGED')
        need(CI_SHA256=='0cef8840cd51b1383e252b08df14b5c19f8dea9c3903583f97a4e6d4e5f365bd' and ci_source_matches(after),'MODEL_NEW_CI_PIN')''','''        # R61 immutable before/after fixtures are real original bytes. This
        # checks the SAME predicate as Audit.sources, not an invented hash map.
        before=(EV/'bootstrap-ci-before-first/scripts/windows-msvc-ci.ps1').read_bytes()
        after=(EV/'bootstrap-ci-after-first/scripts/windows-msvc-ci.ps1').read_bytes()
        need(digest(before)=='0cef8840cd51b1383e252b08df14b5c19f8dea9c3903583f97a4e6d4e5f365bd','MODEL_OLD_CI_ORIGINAL_CHANGED')
        need(CI_SHA256=='43f6bc63800b36162d64285a77913d33a2aa30f0f34c07c9349f6836642813d3' and ci_source_matches(after),'MODEL_NEW_CI_PIN')''')
replace('''        need(pe_header(data,'Win7')['subsystemVersion']==[6,1],'MODEL_PE')
        changed=bytearray(data);struct.pack_into('<H',changed,138,2)
        pe_header(changed,'Modern');reject('WIN7_SUBSYSTEM_NOT_6_1',lambda:pe_header(changed,'Win7'))
        reject('PE_TRUNCATED_OR_BAD_DOS',lambda:pe_header(data[:100],'Modern'))
    test('PE32 subsystem Modern independent/Win7 exact6.1/truncation',pe_cases)''', '''        need(pe_header(data,'Win7','tests')['subsystemVersion']==[6,1],'MODEL_PE')
        gui=bytearray(data);struct.pack_into('<H',gui,156,2)
        for baseline in ['Modern','Win7']:
            need(pe_header(gui,baseline,'product')['subsystem']==2,'MODEL_GUI')
            need(pe_header(data,baseline,'tests')['subsystem']==3,'MODEL_CUI')
            reject('PE_ROLE_SUBSYSTEM',lambda:pe_header(gui,baseline,'tests'))
            reject('PE_ROLE_SUBSYSTEM',lambda:pe_header(data,baseline,'product'))
        reject('INVALID_PE_ROLE',lambda:pe_header(gui,'Win7','any'))
        for role,original in [('product',gui),('tests',data)]:
            changed=bytearray(original);struct.pack_into('<H',changed,138,2)
            pe_header(changed,'Modern',role)
            reject('WIN7_SUBSYSTEM_NOT_6_1',lambda:pe_header(changed,'Win7',role))
            reject('PE_TRUNCATED_OR_BAD_DOS',lambda:pe_header(original[:100],'Modern',role))
    test('actual PE bytes exact GUI product/CUI tests mutual rejection and Win7 exact6.1',pe_cases)
    def producer_subsystem_cases():
        for baseline in ['Modern','Win7']:
            for role,subsystem in [('product',2),('tests',3)]:
                row=dict(role=role,pe_subsystem=subsystem,pe_subsystem_version='6.1' if baseline=='Win7' else None)
                need(producer_subsystem_matches(row,baseline),'MODEL_PRODUCER_PE_ROLE')
                for wrong in [None,True,str(subsystem),5-subsystem]:
                    need(not producer_subsystem_matches(dict(row,pe_subsystem=wrong),baseline),'MODEL_PRODUCER_FALSE_SUBSYSTEM')
                if baseline=='Win7':
                    need(not producer_subsystem_matches(dict(row,pe_subsystem_version='6.2'),baseline),'MODEL_PRODUCER_FALSE_VERSION')
    test('producer pe_subsystem must be exact integer and match role plus version',producer_subsystem_cases)''')
dst.write_bytes(s.encode())
assert src.read_bytes()==original
print('created',dst,hashlib.sha256(dst.read_bytes()).hexdigest())
