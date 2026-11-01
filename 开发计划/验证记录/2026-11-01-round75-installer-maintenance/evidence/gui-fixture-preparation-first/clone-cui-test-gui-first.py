"""R75 test-only PE32 CUI3 to GUI2 fixture. No normal product conversion.
Only --execute writes a new owned fixture directory. --self-test uses memory
bytes exclusively; no actual EXE, Cargo, Windows or network execution.
"""
from pathlib import Path
import argparse, datetime, hashlib, json, re, stat, struct, sys
sys.dont_write_bytecode=True
ROOT=Path('/tmp/fc-round75-installer-maintenance')
PREP=ROOT/'evidence/gui-fixture-preparation-first'
MAX_PE=128*1024*1024
def need(v,c):
    if not v:raise AssertionError(c)
def sha(data):return hashlib.sha256(data).hexdigest()
def u16(data,offset):return struct.unpack_from('<H',data,offset)[0]
def u32(data,offset):return struct.unpack_from('<I',data,offset)[0]
def inspect(data,subsystem):
    need(isinstance(data,bytes) and 64<=len(data)<=MAX_PE,'FINITE_PE_BYTES')
    need(data[:2]==b'MZ','DOS_SIGNATURE')
    pe=u32(data,60);need(64<=pe<=len(data)-24,'PE_OFFSET_BOUNDS')
    need(data[pe:pe+4]==b'PE\0\0','PE_SIGNATURE')
    need(u16(data,pe+4)==0x14c,'I386_ONLY')
    sections=u16(data,pe+6);need(1<=sections<=96,'SECTION_COUNT')
    characteristics=u16(data,pe+22)
    need(characteristics&0x0002 and not characteristics&0x2000,'EXECUTABLE_NOT_DLL')
    opt=pe+24;opt_size=u16(data,pe+20)
    need(opt_size>=136 and opt+opt_size<=len(data),'OPTIONAL_HEADER_BOUNDS')
    need(u16(data,opt)==0x10b,'PE32_ONLY')
    directories=u32(data,opt+92)
    need(5<=directories<=16 and opt_size>=96+8*directories,'DATA_DIRECTORY_BOUNDS')
    need(u32(data,opt+128)==u32(data,opt+132)==0,'UNSIGNED_CERTIFICATE_TABLE_EMPTY')
    need(not u16(data,opt+70)&0x0080,'FORCE_INTEGRITY_FORBIDDEN')
    offset=opt+68;need(u16(data,offset)==subsystem,'EXACT_EXPECTED_SUBSYSTEM')
    table=opt+opt_size;headers=u32(data,opt+60)
    need(table+sections*40<=headers<=len(data),'SECTION_TABLE_AND_HEADERS_BOUNDS')
    entry=u32(data,opt+16);need(entry!=0,'NONZERO_IMAGE_ENTRY_POINT')
    raw_ranges=[];entry_sections=[]
    for i in range(sections):
        sec=table+i*40;virtual_size=u32(data,sec+8);rva=u32(data,sec+12);size=u32(data,sec+16);raw=u32(data,sec+20);flags=u32(data,sec+36)
        if size:
            need(headers<=raw<=len(data) and size<=len(data)-raw,'SECTION_RAW_BOUNDS')
            need(all(raw+size<=a or b<=raw for a,b in raw_ranges),'NONOVERLAPPING_RAW_SECTIONS');raw_ranges.append((raw,raw+size))
        if rva<=entry<rva+max(size,virtual_size):entry_sections.append(flags)
    need(len(entry_sections)==1 and entry_sections[0]&0x20000000,'ENTRY_IN_ONE_EXECUTABLE_SECTION')
    return dict(peOffset=pe,optionalHeaderOffset=opt,subsystemOffset=offset,checksumOffset=opt+64,checksum=u32(data,opt+64),entryPointRva=entry,machine=0x14c,optionalMagic=0x10b,subsystem=subsystem,sectionCount=sections,certificateTableEmpty=True,forceIntegrity=False)
def clone_bytes(data):
    before=inspect(data,3);offset=before['subsystemOffset'];changed=bytearray(data);changed[offset:offset+2]=b'\x02\x00';out=bytes(changed);after=inspect(out,2)
    need(out[:offset]==data[:offset] and out[offset+2:]==data[offset+2:] and len(out)==len(data),'ALL_OTHER_BYTES_IDENTICAL')
    need(before['checksum']==after['checksum'] and before['entryPointRva']==after['entryPointRva'],'CHECKSUM_AND_ENTRY_UNCHANGED')
    return out,before,after
def scoped(path,exe=False):
    p=Path(path);need(p.is_absolute() and p.resolve().is_relative_to(ROOT),'SOURCE_INSIDE_EXACT_R75_ROOT')
    need(not any(q.is_symlink() for q in [p,*p.parents] if q==ROOT or ROOT in q.parents),'NO_SOURCE_SYMLINKS')
    need(stat.S_ISREG(p.lstat().st_mode),'REGULAR_SOURCE')
    need(p.suffix.lower() not in {'.key','.pfx','.p12'},'PRIVATE_MATERIAL_NOT_READ')
    if exe:need(p.suffix.lower()=='.exe','EXACT_PE_SOURCE_SUFFIX')
    return p
def fact(path):
    p=scoped(path);size=p.stat().st_size;need(size<=MAX_PE,'FINITE_ORIGINAL_FILE');data=p.read_bytes();need(len(data)==size,'STABLE_ORIGINAL_SIZE');return dict(path=str(p),bytes=len(data),sha256=sha(data))
def self_test():
    # A synthetic file-format buffer, never saved to disk or executed.
    buf=bytearray(1024);buf[:2]=b'MZ';struct.pack_into('<I',buf,60,128);buf[128:132]=b'PE\0\0';struct.pack_into('<HH',buf,132,0x14c,1);struct.pack_into('<HH',buf,148,224,0x102)
    o=152;struct.pack_into('<H',buf,o,0x10b);struct.pack_into('<I',buf,o+16,0x1000);struct.pack_into('<I',buf,o+60,512);struct.pack_into('<I',buf,o+64,0x12345678);struct.pack_into('<H',buf,o+68,3);struct.pack_into('<H',buf,o+70,0x140);struct.pack_into('<I',buf,o+92,16)
    s=o+224;buf[s:s+8]=b'.text\0\0\0';struct.pack_into('<IIII',buf,s+8,512,0x1000,512,512);struct.pack_into('<I',buf,s+36,0x60000020);buf[512:]=bytes(range(256))*2
    original=bytes(buf);out,before,after=clone_bytes(original);off=before['subsystemOffset'];need(out[off:off+2]==b'\x02\x00' and original[off:off+2]==b'\x03\x00','POSITIVE_EXACT_SUBSYSTEM_FIELD')
    need([i for i,(a,b) in enumerate(zip(original,out)) if a!=b]==[off] and before['checksum']==after['checksum']==0x12345678,'POSITIVE_ONLY_LOW_BYTE_DIFF_WITH_FULL_FIELD_PROOF')
    positives=['valid_unsigned_i386_CUI_to_GUI','all_other_code_import_entry_bytes_identical','nonzero_checksum_preserved_not_recomputed']
    negatives=[]
    def reject(name,data,code):
        try:clone_bytes(data)
        except AssertionError as error:need(str(error)==code,'NEGATIVE_EXPECTED_REASON_'+name);negatives.append(dict(name=name,reason=code))
        else:raise AssertionError('NEGATIVE_ACCEPTED_'+name)
    def altered(offset,fmt,value):
        data=bytearray(original);struct.pack_into(fmt,data,offset,value);return bytes(data)
    reject('short_dos',b'MZ','FINITE_PE_BYTES');reject('wrong_dos',b'NO'+original[2:],'DOS_SIGNATURE');reject('pe_header_overflow',altered(60,'<I',len(original)-4),'PE_OFFSET_BOUNDS');reject('pe_inside_dos',altered(60,'<I',32),'PE_OFFSET_BOUNDS');reject('bad_pe_signature',original[:128]+b'NOPE'+original[132:],'PE_SIGNATURE')
    reject('wrong_machine',altered(132,'<H',0x8664),'I386_ONLY');reject('zero_sections',altered(134,'<H',0),'SECTION_COUNT');reject('too_many_sections',altered(134,'<H',97),'SECTION_COUNT');reject('dll',altered(150,'<H',0x2102),'EXECUTABLE_NOT_DLL');reject('not_executable',altered(150,'<H',0x100),'EXECUTABLE_NOT_DLL')
    reject('optional_header_short',altered(148,'<H',128),'OPTIONAL_HEADER_BOUNDS');reject('optional_header_overflow',altered(148,'<H',0xffff),'OPTIONAL_HEADER_BOUNDS');reject('pe32plus',altered(o,'<H',0x20b),'PE32_ONLY');reject('directories_missing_security_slot',altered(o+92,'<I',4),'DATA_DIRECTORY_BOUNDS');reject('directories_overflow',altered(o+92,'<I',17),'DATA_DIRECTORY_BOUNDS')
    reject('signed_cert_offset',altered(o+128,'<I',512),'UNSIGNED_CERTIFICATE_TABLE_EMPTY');reject('signed_cert_size',altered(o+132,'<I',8),'UNSIGNED_CERTIFICATE_TABLE_EMPTY');reject('force_integrity',altered(o+70,'<H',0x1c0),'FORCE_INTEGRITY_FORBIDDEN');reject('already_GUI',out,'EXACT_EXPECTED_SUBSYSTEM');reject('other_subsystem',altered(o+68,'<H',1),'EXACT_EXPECTED_SUBSYSTEM')
    reject('section_table_outside_headers',altered(o+60,'<I',400),'SECTION_TABLE_AND_HEADERS_BOUNDS');reject('zero_entry',altered(o+16,'<I',0),'NONZERO_IMAGE_ENTRY_POINT');reject('entry_outside_sections',altered(o+16,'<I',0x4000),'ENTRY_IN_ONE_EXECUTABLE_SECTION');reject('nonexecutable_entry_section',altered(s+36,'<I',0x40000040),'ENTRY_IN_ONE_EXECUTABLE_SECTION');reject('raw_section_overlaps_headers',altered(s+20,'<I',500),'SECTION_RAW_BOUNDS');reject('raw_section_outside_file',altered(s+16,'<I',513),'SECTION_RAW_BOUNDS')
    return dict(status='PASS_PURE_IN_MEMORY_PE_FORMAT_TESTS_ONLY',positiveCases=positives,negativeCases=negatives,actualExeWritten=False,WindowsOrCargoExecuted=False,scope='Synthetic in-memory file-format checks only. No PE loader, GUI/null-stdio, CLI, producer provenance or actual artifact acceptance.')
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--self-test',action='store_true');p.add_argument('--execute',action='store_true')
    p.add_argument('--source-git-base');p.add_argument('--candidate-source-producer',type=Path);p.add_argument('--candidate-source-producer-sha256');p.add_argument('--candidate-source-freeze',type=Path);p.add_argument('--candidate-source-freeze-sha256');p.add_argument('--source-cui',type=Path);p.add_argument('--source-cui-bytes',type=int);p.add_argument('--source-cui-sha256');p.add_argument('--output-dir',type=Path);a=p.parse_args()
    if a.self_test:
        need(not a.execute,'NO_SELFTEST_WITH_ARTIFACT_EXECUTION');print(json.dumps(self_test()));return 0
    for n in ('source_git_base','candidate_source_producer','candidate_source_producer_sha256','candidate_source_freeze','candidate_source_freeze_sha256','source_cui','source_cui_bytes','source_cui_sha256','output_dir'):need(getattr(a,n) is not None,'REQUIRED_'+n)
    need(re.fullmatch('[0-9a-f]{40}',a.source_git_base),'EXPLICIT_GIT_BASE_ONLY')
    for h in (a.candidate_source_producer_sha256,a.candidate_source_freeze_sha256,a.source_cui_sha256):need(re.fullmatch('[0-9a-f]{64}',h),'EXPLICIT_ORIGINAL_SHA256')
    need(0<a.source_cui_bytes<=MAX_PE,'FINITE_EXPECTED_CUI_BYTES')
    if not a.execute:print(json.dumps(dict(status='PREPARED_TEST_FIXTURE_NOT_CREATED',sourceGitBase=a.source_git_base,normalProduct=False,sourcePayloadRead=False)));return 0
    need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n' and (PREP/'.owner').read_bytes()==b'future-classroom-owned-r75-gui-fixture-preparation/v1\n','EXACT_R75_OWNERS')
    producer_fact=fact(a.candidate_source_producer);freeze_fact=fact(a.candidate_source_freeze)
    need(producer_fact['sha256']==a.candidate_source_producer_sha256 and freeze_fact['sha256']==a.candidate_source_freeze_sha256,'EXACT_CANDIDATE_PRODUCER_FREEZE')
    producer_bytes=scoped(a.candidate_source_producer).read_bytes();need(sha(producer_bytes)==a.candidate_source_producer_sha256,'EXACT_PARSED_PRODUCER_BYTES');producer=json.loads(producer_bytes)
    need(producer['status']=='PASS_CODEGEN_NOT_RUNTIME' and producer['sourceUnchanged'] is True and producer['sourceBefore']==producer['sourceAfter'] and producer['freezeSha256']==a.candidate_source_freeze_sha256,'ACTUAL_FROZEN_N_PRODUCER_SCHEMA')
    tests=producer['testExecutables'];normals=producer['normalExecutables'];need(producer['testHarnessCount']==len(tests)==22 and producer['normalExecutableCount']==len(normals)==2 and {r['target'] for r in normals}=={'fc-bootstrap','fc-installer-lifecycle'},'ORIGINAL22_TESTS_TWO_CLOSED_NORMALS')
    all_rows=tests+normals;need(len({r['path'] for r in all_rows})==24,'UNIQUE24_ORIGINAL_ARTIFACTS')
    before={}
    for row in all_rows:
        path=scoped(row['path'],True);f=fact(path);need(f['bytes']==row['bytes'] and f['sha256']==row['sha256'] and row['profile']['test'] is (row in tests) and row['peSubsystem']==(3 if row in tests else 2),'ALL_ORIGINAL_ARTIFACTS_MATCH_PRODUCER');before[str(path)]=f
    source=scoped(a.source_cui,True);source_fact=fact(source);need(source_fact==dict(path=str(source),bytes=a.source_cui_bytes,sha256=a.source_cui_sha256),'EXACT_REQUESTED_CUI_IDENTITY')
    rows=[r for r in tests if Path(r['path'])==source and r['target']=='fc_windows' and r['kind']==['lib'] and r['profile']['test'] is True and Path(r['manifestPath']).parts[-3:]==('crates','fc-windows','Cargo.toml') and '/crates/fc-windows#' in r['packageId']];need(len(rows)==1,'EXACT_FC_WINDOWS_LIB_TEST_CARGO_ARTIFACT')
    original=source.read_bytes();need(len(original)==a.source_cui_bytes and sha(original)==a.source_cui_sha256,'EXACT_PATCHED_ORIGINAL_BYTES');gui,pe_before,pe_after=clone_bytes(original)
    outdir=a.output_dir;need(outdir.is_absolute() and outdir.parent==ROOT and re.fullmatch('gui-test-fixture-[0-9a-f]{32}',outdir.name) and not outdir.exists() and not outdir.is_symlink(),'CREATE_NEW_CLOSED_OWNED_FIXTURE_DIRECTORY')
    outdir.mkdir();owner=outdir/'.owner'
    with owner.open('xb') as f:f.write(b'future-classroom-owned-r75-cui3-gui2-fixture/v1\n')
    exe=outdir/'fc_windows_gui_test_fixture.exe'
    with exe.open('xb') as f:f.write(gui)
    need(exe.read_bytes()==gui and source.read_bytes()==original,'CLONE_AND_ORIGINAL_READBACK_EXACT')
    need(before=={str(scoped(r['path'],True)):fact(r['path']) for r in all_rows} and fact(a.candidate_source_producer)==producer_fact and fact(a.candidate_source_freeze)==freeze_fact,'ALL24_ORIGINALS_AND_PROVENANCE_UNCHANGED')
    ledger=dict(status='PASS_EXACT_CUI3_TO_GUI2_TEST_FIXTURE_CLONE',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),sourceGitBase=a.source_git_base,candidateSourceProducerSha256=a.candidate_source_producer_sha256,candidateSourceFreezeSha256=a.candidate_source_freeze_sha256,sourceCui=source_fact,guiFixture=fact(exe),cargoArtifact=dict(package='fc-windows',target='fc_windows',role='tests',cargo_profile_test=True,sha256=a.source_cui_sha256),changedFields=[dict(name='Subsystem',offset=pe_before['subsystemOffset'],beforeHex='0300',afterHex='0200')],normalProduct=False,profileTest=True,checksum=dict(offset=pe_before['checksumOffset'],originalValue=pe_before['checksum'],fixtureValue=pe_after['checksum'],recomputed=False),allOtherBytesIdentical=True,original24ArtifactsUnchanged=True,originalArtifactIdentities=list(before.values()),producer=producer_fact,sourceFreeze=freeze_fact,originalPe=pe_before,fixturePe=pe_after,scope='Test-only cloned fc_windows libtest image, not a normal product or rebuilt/signed image. Only Subsystem field changed. Checksum deliberately retains its original value; no checksum verification claim. Windows GUI/null-stdio/CLI behavior requires separate root actual execution. Candidate producer/freezefile bind uncommitted bytes; sourceGitBase is only a base, not an actual candidate source commit.')
    report=outdir/'fixture-ledger.json'
    with report.open('x') as f:json.dump(ledger,f,indent=2);f.write('\n')
    print(json.dumps(dict(status=ledger['status'],ledger=fact(report),guiFixture=ledger['guiFixture'],normalProduct=False)));return 0
if __name__=='__main__':raise SystemExit(main())
