"""Attribute two known N3 child libtest scopes without editing original output."""
from pathlib import Path
import hashlib,json,runpy
EV=Path('/tmp/fc-round73-installer-broker/evidence')
TEMPLATE=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py')
CHILD='installer_lifecycle::native::broker::tests::pipe_process_fixture_child'
PRODUCER_SHA='858d3c62e4d3c4e6739a5763a77e26ccf368038a22d4ef150f5d284a080b5b07'
NATIVE_SHA='998ae30c1c70a60fe6b25b49090d7db3ca4ba161a033656b5bd03e947df70104'
def sha(data):return hashlib.sha256(data).hexdigest()
def native_harness(item):
    assert sha(TEMPLATE.read_bytes())=='0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5'
    p=EV/'n-release-third.json';assert sha(p.read_bytes())==PRODUCER_SHA
    producer=json.loads(p.read_bytes());assert producer['status']=='PASS_CODEGEN_NOT_RUNTIME' and producer['sourceUnchanged']
    source=next(r for r in producer['sourceBefore'] if r['path']=='crates/fc-windows/src/installer_lifecycle/native/broker.rs')
    path=EV.parent/'worktree'/source['path'];assert sha(path.read_bytes())==source['sha256']
    assert item['binarySha256']==NATIVE_SHA and item['name']=='fc_windows-ac67b5f1df5ec8b2.exe'
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
if __name__=='__main__':
    p=EV/'root-windows-native-harness-third.json';v=json.loads(p.read_bytes())
    assert v['status']==v['cleanup']=='PASS' and len(v['tests'])==1
    result=native_harness(v['tests'][0])
    out=EV/'root-native-child-attribution-first.json';assert not out.exists()
    report=dict(status='PASS_SOURCE_BOUND_NATIVE_PARENT_AND_CHILD_ATTRIBUTION_ONLY',actualReport=dict(path=p.name,sha256=sha(p.read_bytes())),parent=result,helperSha256=sha(Path(__file__).read_bytes()),scope='Native261 and known child subruns only; no full22/public20/MSI or Win7/10 OS acceptance.')
    with out.open('x') as s:json.dump(report,s,indent=2);s.write('\n')
    print(json.dumps(dict(status=report['status'],passed=result['passed'],failed=result['failed'],childScopes=len(result['childScopes']),sha256=sha(out.read_bytes()))))
