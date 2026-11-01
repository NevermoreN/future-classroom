#!/usr/bin/env python3
"""Prepare-only retained source scan with the actual archive scanner; no copy."""
from pathlib import Path
import ast,datetime,hashlib,json,runpy
R=Path(__file__).resolve().parent
sha=lambda b:hashlib.sha256(b).hexdigest()
def main():
 helper=R/'archive-evidence-first.py';ns=runpy.run_path(str(helper));tree=ast.parse(helper.read_text())
 main_node=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='main')
 scanner=next(n for n in main_node.body if isinstance(n,ast.FunctionDef) and n.name=='scan')
 env=dict(ns);env.update(findings=[],stats={'sourceFiles':0,'storedFiles':0,'gzipFiles':0,'utf16Views':0,'decodedRawPayloads':0,'jsonContainers':0,'textViews':0,'decodedJsonStringViews':0})
 exec(compile(ast.Module(body=[scanner],type_ignores=[]),str(helper),'exec'),env)
 rows=[]
 for p in sorted(R.rglob('*')):
  if p.is_dir() or ns['excluded'](p):continue
  data=p.read_bytes();rel=p.relative_to(R).as_posix();env['scan'](data,'source:'+rel);env['stats']['sourceFiles']+=1
  rows.append(dict(path=rel,bytes=len(data),sha256=sha(data)))
 result=dict(schemaVersion=1,status='PASS_SOURCE_SCAN_ONLY_NOT_ARCHIVE' if not env['findings'] else 'FAIL_SOURCE_SCAN_FINDINGS',
  recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),helper=dict(path=helper.name,sha256=sha(helper.read_bytes())),
  statistics=env['stats'],patterns=list(ns['PATTERNS']),findings=env['findings'],sourceInventory=rows,
  scope='Retained source bytes only, before this receipt. No original value printed or changed; no archive/Windows/Cargo/network operation. Actual stored counterparts, subsequent final acceptance and this receipt remain subject to full root archive scan.')
 data=(json.dumps(result,ensure_ascii=False,indent=2)+'\n').encode();p=R/'archive-source-scan-first.json'
 with p.open('xb') as f:f.write(data)
 print(json.dumps(dict(status=result['status'],sourceFiles=len(rows),findings=env['findings'],reportSha256=sha(data))))
if __name__=='__main__':main()
