from pathlib import Path
import hashlib,json,re,subprocess

repo=Path('/workspace/共享/Eliora/未来教室计划')
roots=[(Path('/tmp/fc-round62-ci-evidence'),repo/'开发计划/验证记录/2026-11-01-round62-ci-phase'),(Path('/tmp/fc-round60-lifecycle-research-evidence'),repo/'开发计划/验证记录/2026-11-01-round60-native-feasibility')]
secret=[re.compile(rb'-----BEGIN (?:OPENSSH |RSA |EC )?PRIVATE KEY-----'),re.compile(rb'github_pat_[A-Za-z0-9_]{20,}'),re.compile(rb'gh[pousr]_[A-Za-z0-9]{30,}'),re.compile(rb'sk-proj-[A-Za-z0-9_-]{20,}'),re.compile(rb'https?://[^\s\"<>]+[?&](?:X-Amz-Signature|sig)=[A-Za-z0-9%/+_-]{16,}',re.I)]
summaries=[]
for source,dest in roots:
    assert not dest.exists();dest.mkdir(parents=True)
    rows=[];excluded=[]
    for f in sorted(source.rglob('*')):
        if not f.is_file():continue
        relative=f.relative_to(source);name=relative.as_posix()
        if '/tools/' in '/'+name:
            excluded.append({'path':name,'reason':'Temporary extracted tool/package; not read or followed'});continue
        assert not f.is_symlink()
        data=f.read_bytes();h=hashlib.sha256(data).hexdigest();reason=None
        if f.name=='.owner':reason='Owned temporary root marker, not project input'
        if source.name.startswith('fc-round60'):
            if name.startswith('registry-write-intent/scratch-fixture-third/'):reason='Active unfinished next candidate; not this completed batch'
            if '/tools/' in '/'+name:reason='Temporary extracted build tools/packages'
            if name.endswith(('root-native-upload-first.input.ps1','root-native-upload-second.input.ps1','root-upload-first.input.ps1')):reason='Transport input embeds excluded compiled payload bytes'
            if name.endswith('root-native-sourcehash-backup-first.stdout'):reason='Transport embeds original excluded owned Installer cache bytes'
        if f.suffix.lower() in ('.zip','.exe','.msi','.deb','.bin') or data.startswith((b'MZ',b'PK\x03\x04',b'!<arch>\n',bytes.fromhex('d0cf11e0a1b11ae1'))):reason=reason or 'Compiled executable/package/cache or binary transport; identity retained'
        fact={'path':name,'bytes':len(data),'sha256':h}
        if reason:excluded.append(dict(fact,reason=reason));continue
        if data.startswith((b'\xff\xfe',b'\xfe\xff')):
            text=data.decode('utf-16');encoding='UTF-16 original bytes'
        else:
            text=data.decode('utf-8-sig');encoding='UTF-8 original bytes'
        for pattern in secret:
            assert not pattern.search(data) and not pattern.search(text.encode()), 'Potential credential/signed URL: '+name
        target=dest/relative;target.parent.mkdir(parents=True,exist_ok=True)
        with target.open('xb') as stream:stream.write(data)
        assert target.read_bytes()==data
        rows.append(dict(fact,encoding=encoding))
    index={'schemaVersion':1,'scope':'Original bytes for completed finite native feasibility and CI failure/fix evidence; no completed product/OS/installer claim','sourceRoot':str(source),'sourceCommit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),'files':rows,'excluded':excluded,'includedFiles':len(rows),'includedBytes':sum(r['bytes'] for r in rows),'credentialsAndSignedUrlPatternScan':'PASS_INCLUDED_TEXT_OR_UTF16_VIEWS','unfinishedWork':'R63 production gate and registry third candidate remain separate, not certified by this archive'}
    path=dest/'archive-index-first.json';path.write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
    summaries.append({'archive':str(dest.relative_to(repo)),'includedFiles':len(rows),'includedBytes':index['includedBytes'],'excludedFiles':len(excluded),'indexSha256':hashlib.sha256(path.read_bytes()).hexdigest()})
report=Path('/tmp/fc-round62-ci-evidence/completed-archive-first.json');report.write_text(json.dumps(summaries,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(summaries,ensure_ascii=False),flush=True)
