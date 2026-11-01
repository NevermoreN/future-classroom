from pathlib import Path
import hashlib
old=Path('/tmp/fc-round55-ci-evidence/verify-ci-artifact-first.py')
p=Path('/tmp/fc-round57-ci-inventory-evidence/verify-ci-artifact-expanded-first.py')
b=old.read_bytes();assert hashlib.sha256(b).hexdigest()=='cf3780a6d08dec8f395383a20a9a1992b5e7d5a4b70f5cf865c16ce0b5e13e9c';assert not p.exists()
s=b.decode().replace('R46 static original-artifact audit.','R57 finite 20,000-source-path original-artifact audit.').replace('python3 verify-ci-artifact-first.py','python3 verify-ci-artifact-expanded-first.py')
s=s.replace("CI_SHA256 = 'ed26c9932325d377bbbf4eaf63aad29d75fc0cc8ad77561b1121f3389024c418'","CI_SHA256 = '0cef8840cd51b1383e252b08df14b5c19f8dea9c3903583f97a4e6d4e5f365bd'\nMAX_SOURCE_PATHS = 20000 # Source inventory only; artifact/ZIP and byte caps stay unchanged.")
s=s.replace('def file_fact(path):',"def ci_source_matches(data):\n    return digest(data) == CI_SHA256\n\ndef file_fact(path):")
replacements=[('    unique_rows(rows)\n    return rows','    unique_rows(rows, max_rows=MAX_SOURCE_PATHS)\n    return rows'),('need(len(paths) <= 10000 and','need(len(paths) <= MAX_SOURCE_PATHS and'),("actual = unique_rows(tree.get('files')); expected = unique_rows(expected_rows)","actual = unique_rows(tree.get('files'), max_rows=MAX_SOURCE_PATHS); expected = unique_rows(expected_rows, max_rows=MAX_SOURCE_PATHS)"),('dict(paths=10000, per_file_bytes=MAX_FILE','dict(paths=MAX_SOURCE_PATHS, per_file_bytes=MAX_FILE'),('sha_rows = unique_rows(hashed)','sha_rows = unique_rows(hashed, max_rows=MAX_SOURCE_PATHS)'),("hist_rows = unique_rows(hist.get('checkout_metadata'))","hist_rows = unique_rows(hist.get('checkout_metadata'), max_rows=MAX_SOURCE_PATHS)"),('metadata=unique_rows(expected);direct=crlf=0','metadata=unique_rows(expected, max_rows=MAX_SOURCE_PATHS);direct=crlf=0'),("self.check('exact R45 CI helper pinned source',digest(self.ci_source)==CI_SHA256)","self.check('exact R57 CI helper pinned source',ci_source_matches(self.ci_source))")]
for oldtext,newtext in replacements:
 assert oldtext in s,oldtext;s=s.replace(oldtext,newtext)
needle="    test('baseline exact buildstd placement/no metadata fmt leakage',arguments)"
new=needle+'''
    # Real-sized pure source inventories, not a whole fake CI artifact. Both
    # variants exceed the old cap in a different partition while total M stays
    # at the new finite maximum. No payload allocation/EXE/network/CI activity.
    def source_fixture(s_count):
        rs=[]
        for i in range(MAX_SOURCE_PATHS):
            name=(f'src/input-{i:05d}.txt' if i<s_count else PREFIX+f'old-{i:05d}.json')
            rs.append(dict(path=name,mode='100644',type='blob',git_blob_oid_sha1='3'*40,git_blob_bytes=1,domain=domain(name)))
        value=[[dict(path=r['path'],bytes=1,sha256='5'*64) for r in rs[:s_count]],
            dict(schema=TREE_SCHEMA,source_sha=source,root_tree_oid_sha1=oid,files=rs),
            dict(schema=SCHEMA,source_sha=source,root_tree_oid_sha1=oid,tracked_file_count=len(rs),explicit_build_reference_policy='not hermetic',
                limits=dict(paths=MAX_SOURCE_PATHS,per_file_bytes=MAX_FILE,sha256_input_total_bytes=MAX_HASHED,metadata_output_bytes=MAX_META),
                sha256_input=dict(manifest='input-sha256.json',file_count=s_count,checkout_bytes_hashed=s_count),
                historical_git_archive=dict(exact_prefix=PREFIX,file_count=len(rs)-s_count,git_blob_bytes=len(rs)-s_count,
                    checkout_metadata_bytes=2*(len(rs)-s_count),git_manifest='input-git-tree.json',
                    checkout_metadata=[dict(path=r['path'],checkout_bytes=2) for r in rs[s_count:]],content_sha256='NOT_READ_BY_INPUT_HASHER'))]
        return rs,value
    def real_source_counts():
        rs,_=source_fixture(1)
        raw=b''.join((f"100644 blob {r['git_blob_oid_sha1']}       1\\t{r['path']}\\0").encode() for r in rs)
        paths=b''.join((r['path']+'\\0').encode() for r in rs)
        parsed=parse_git_tree(raw)
        need(len(parsed)==MAX_SOURCE_PATHS and parsed[-1]==rs[-1] and parsed[10000]==rs[10000],'MODEL_SOURCE_LAST_ENTRY')
        need(parse_paths(paths)=={r['path'] for r in rs},'MODEL_FULL_TRACKED_PATHS')
        extra=(f"100644 blob {'4'*40}       1\\textra.txt\\0").encode()
        reject('INVALID_ROW_COLLECTION',lambda:parse_git_tree(raw+extra))
        reject('INVALID_TRACKED_PATH_SET',lambda:parse_paths(paths+b'extra.txt\\0'))
    test('real 20000 Git/path entries preserve last row; 20001 rejected',real_source_counts)
    def complete_large_partitions():
        for s_count in (1,10001):
            rs,value=source_fixture(s_count)
            counts,hashed=validate_domains(source,oid,rs,value,copy.deepcopy(value))
            need(counts['fullGitMetadata']==MAX_SOURCE_PATHS and counts['actualShaSubset']==s_count
                and counts['historicalMetadataOnly']==MAX_SOURCE_PATHS-s_count and len(hashed)==s_count,'MODEL_FULL_LARGE_PARTITION')
            need(value[1]['files'][-1]['path']==rs[-1]['path'],'MODEL_PARTITION_LAST_PATH')
    test('full 20000 partitions allow over10000 SHA or historical rows without skipping',complete_large_partitions)
    def tail_and_old_limit_rejection():
        rs,value=source_fixture(1)
        changed=copy.deepcopy(value);changed[2]['historical_git_archive']['checkout_metadata'].pop()
        reject('HISTORY_NOT_COMPLETE_PARTITION',lambda:validate_domains(source,oid,rs,changed,copy.deepcopy(changed)))
        changed=copy.deepcopy(value);changed[1]['files'].pop()
        reject('FULL_GIT_METADATA_MISMATCH',lambda:validate_domains(source,oid,rs,changed,copy.deepcopy(changed)))
        changed=copy.deepcopy(value);changed[2]['limits']['paths']=10000
        reject('SOURCE_LIMITS_CHANGED',lambda:validate_domains(source,oid,rs,changed,copy.deepcopy(changed)))
        rs,value=source_fixture(10001);changed=copy.deepcopy(value);changed[0].pop()
        reject('SHA_SUBSET_NOT_COMPLETE_PARTITION',lambda:validate_domains(source,oid,rs,changed,copy.deepcopy(changed)))
    test('missing large partition tail/fullGit/SHA tail and old10000 summary rejected',tail_and_old_limit_rejection)
    def frozen_source_pin():
        # The R57 immutable before/after fixtures are real original bytes. This
        # checks the SAME predicate as Audit.sources, not an invented hash map.
        before=(EV/'windows-msvc-ci.before.ps1').read_bytes()
        after=(EV/'implementation-after-first/scripts/windows-msvc-ci.ps1').read_bytes()
        need(digest(before)=='ed26c9932325d377bbbf4eaf63aad29d75fc0cc8ad77561b1121f3389024c418','MODEL_OLD_CI_ORIGINAL_CHANGED')
        need(CI_SHA256=='0cef8840cd51b1383e252b08df14b5c19f8dea9c3903583f97a4e6d4e5f365bd' and ci_source_matches(after),'MODEL_NEW_CI_PIN')
        need(not ci_source_matches(before),'MODEL_OLD_CI_FALSELY_ADMITTED')
    test('new exact CI source pin accepts actual new bytes and rejects actual old helper',frozen_source_pin)
    def unrelated_caps_unchanged():
        need(unique_rows.__defaults__==('path',10000),'MODEL_GENERIC_ROW_CAP_CHANGED')
        reject('INVALID_ROW_COLLECTION',lambda:unique_rows([dict(path=f'a{i}') for i in range(10001)]))
        infos=[]
        for i in range(10000):
            member=zipfile.ZipInfo(f'a{i}.json');member.file_size=1;infos.append(member)
        need(len(zip_inventory(infos))==10000,'MODEL_ZIP_CAP_SHRANK')
        reject('ZIP_MEMBER_COUNT',lambda:zip_inventory(infos+[zipfile.ZipInfo('extra.json')]))
        need((MAX_FILE,MAX_HASHED,MAX_META,MAX_PAYLOAD)==(64*1024*1024,256*1024*1024,16*1024*1024,512*1024*1024),'MODEL_BYTE_CAP_CHANGED')
    test('generic rows and ZIP members stay10000; all byte limits unchanged',unrelated_caps_unchanged)
'''
assert needle in s;s=s.replace(needle,new)
p.write_text(s)
print('prepared',len(p.read_bytes()),hashlib.sha256(p.read_bytes()).hexdigest())
