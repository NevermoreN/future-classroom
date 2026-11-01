"""Source-bound R74 MSVC child stdout attribution; no output or EXE execution.
Exactly two original source-bound child scopes remain separate from outer totals.
"""
from pathlib import Path
import hashlib,json,runpy
TEMPLATE=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py')
TEMPLATE_SHA='0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5'
CHILD='installer_lifecycle::native::broker::tests::pipe_process_fixture_child'
def sha(data):return hashlib.sha256(data).hexdigest()
def native_harness(item,actual_native_row,child_source):
    assert sha(TEMPLATE.read_bytes())==TEMPLATE_SHA
    assert child_source['gitSourceSha256']=='977a4ab8fd8cf15ee400e30bb5a6badea67f773cf500ca4d791ec4286bd96a04'
    producer=actual_native_row['producer']
    assert producer['role']=='tests' and producer['package']=='fc-windows' and producer['target']=='fc_windows' and producer['kinds']==['lib'] and producer['cargo_profile_test'] is True
    assert item['name']==Path(actual_native_row['localPath']).name and item['binarySha256']==producer['sha256']
    old=runpy.run_path(str(TEMPLATE));lines=item['output']
    runs=[(i,old['RUNNING'].fullmatch(s)) for i,s in enumerate(lines) if old['RUNNING'].fullmatch(s)]
    summaries=[(i,old['SUMMARY'].fullmatch(s)) for i,s in enumerate(lines) if old['SUMMARY'].fullmatch(s)]
    assert len(runs)==len(summaries)==3 and int(runs[0][1][1])>1
    declared=int(runs[0][1][1]);assert all(int(m[1])==1 for _,m in runs[1:])
    assert summaries[-1][0]>runs[-1][0] and list(map(int,summaries[-1][1].groups()[1:6]))==[declared,0,0,0,0]
    removed=set();children=[]
    for ordinal,((begin,_),(end,summary)) in enumerate(zip(runs[1:],summaries[:-1]),1):
        assert begin<end and (ordinal==2 or end<runs[2][0])
        assert summary[1]=='ok' and list(map(int,summary.groups()[1:6]))==[1,0,0,0,declared-1]
        fixture=[i for i in range(begin+1,end) if lines[i]=='test '+CHILD+' ... ok']
        assert len(fixture)==1
        positions=[begin,fixture[0],end]
        assert not removed.intersection(positions);removed.update(positions)
        children.append(dict(ordinal=ordinal,case=CHILD,rawLineIndices=positions,originalLines=[lines[i] for i in positions],passed=1,failed=0,ignored=0,measured=0,filteredOut=declared-1,
            identityQualification='Original child PID/creation not printed. The pinned source tests assert original process capability binding and successful original Child exit; no independent raw OS identity is claimed.'))
    original_case_positions=[i for i,s in enumerate(lines) if s=='test '+CHILD+' ... ok']
    assert len(original_case_positions)==3 and len([i for i in original_case_positions if i not in removed])==1
    derived=[s for i,s in enumerate(lines) if i not in removed]
    parent=dict(item,output=derived)
    result=old['harness'](parent)
    result.update(childScopes=children,removedControlLineIndices=sorted(removed),
        originalNormalizedLinesSha256=sha(json.dumps(lines,ensure_ascii=False,separators=(',',':')).encode()),
        derivedParentLinesSha256=sha(json.dumps(derived,ensure_ascii=False,separators=(',',':')).encode()),
        qualification='Strict unchanged R67 parser applied to an explicit derived parent view. Exactly two source-bound child running/case/summary triples attributed separately; all original raw and normalized lines remain unchanged. Child filtered counts never promoted to outer passed/ignored/filtered totals.')
    return result
