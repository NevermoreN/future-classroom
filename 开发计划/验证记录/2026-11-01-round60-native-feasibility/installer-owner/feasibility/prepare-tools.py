#!/usr/bin/env python3
"""Extract only the seven exact already-reviewed Ubuntu packages, locally."""
import hashlib,json,os,pathlib,subprocess,time,urllib.request
ROOT=pathlib.Path(__file__).resolve().parent
REFERENCE=pathlib.Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round53-msi/tools-preparation-second.json')
def sha(b): return hashlib.sha256(b).hexdigest()
def main():
    reference_bytes=REFERENCE.read_bytes(); reference=json.loads(reference_bytes)
    tools=ROOT/'tools'; tools.mkdir(); (tools/'.owner').write_text('R60 exact pinned local tools, no system installation\n')
    downloads=tools/'packages'; downloads.mkdir(); extracted=tools/'root'; extracted.mkdir()
    report={'status':'RUNNING','reference':{'path':str(REFERENCE),'sha256':sha(reference_bytes)},'packages':[],'commands':[],'tools':[],'scope':'Exact historical official hashes, current TLS mirror transfer. No sudo/system install/global linker or Windows action.'}
    total=0
    try:
        for item in reference['packages']:
            assert item['transferBase']=='https://mirrors.edge.kernel.org/ubuntu/'
            rel=item['officialRepositoryPath']; assert rel.startswith('pool/') and '..' not in rel.split('/')
            target=downloads/pathlib.PurePosixPath(rel).name
            row={k:item[k] for k in ['name','version','bytes','sha256','officialRepositoryPath']}; row['status']='RUNNING';report['packages'].append(row)
            before=time.monotonic()
            with urllib.request.urlopen(item['transferBase']+rel, timeout=25) as response,target.open('xb') as output:
                assert response.status==200
                received=0; digest=hashlib.sha256()
                while True:
                    assert time.monotonic()-before < 120,'package original finite download deadline'
                    chunk=response.read(65536)
                    if not chunk: break
                    received+=len(chunk);total+=len(chunk)
                    assert received<=item['bytes'] and total<=512*1024*1024
                    output.write(chunk);digest.update(chunk)
            row.update(actualBytes=received,actualSha256=digest.hexdigest(),elapsedSeconds=time.monotonic()-before)
            assert received==item['bytes'] and digest.hexdigest()==item['sha256'],'exact package bytes/SHA mismatch'
            row['status']='PASS_EXACT_DOWNLOAD'
        for item in reference['packages']:
            args=['dpkg-deb','-x',str(downloads/pathlib.PurePosixPath(item['officialRepositoryPath']).name),str(extracted)]
            cp=subprocess.run(args,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=60,check=False)
            report['commands'].append({'argv':args,'actualExit':True,'exitCode':cp.returncode,'stdout':cp.stdout.decode('utf8','strict'),'stderr':cp.stderr.decode('utf8','strict')})
            assert cp.returncode==0
        env=os.environ.copy();env['LD_LIBRARY_PATH']=str(extracted/'usr/lib/x86_64-linux-gnu')
        for name in ['wixl','msiinfo','msiextract','msibuild']:
            exe=extracted/'usr/bin'/name; cp=subprocess.run([str(exe),'--version'],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=25,check=False)
            for kind,data in [('stdout',cp.stdout),('stderr',cp.stderr)]: (tools/(name+'-version.'+kind+'.log')).write_bytes(data)
            expected=next((x['sha256'] for x in reference['tools'] if pathlib.Path(x['path']).name==name),None)
            actual=sha(exe.read_bytes())
            report['tools'].append({'name':name,'path':str(exe),'bytes':exe.stat().st_size,'sha256':actual,'priorR53ToolSha256':expected,'priorR53Match':actual==expected if expected else 'NOT_RECORDED_MSIBUILD_BOUND_BY_EXACT_PACKAGE','exitCode':cp.returncode,'stdoutSha256':sha(cp.stdout),'stderrSha256':sha(cp.stderr)})
            assert expected is None or actual==expected
            # msibuild has no --version; its exact ELF is bound by the package.
            assert cp.returncode==0 or name=='msibuild'
        report['status']='PASS_EXACT_LOCAL_EXTRACTION_AND_TOOL_BINDING'
    except BaseException as error:
        report['status']='FAIL';report['error']={'kind':type(error).__name__,'message':'Pinned local tool preparation failed; original bytes and logs retained.'}
        raise
    finally:
        report['totalDownloadedBytes']=total
        (ROOT/'tools-preparation-first.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
if __name__=='__main__': main()
