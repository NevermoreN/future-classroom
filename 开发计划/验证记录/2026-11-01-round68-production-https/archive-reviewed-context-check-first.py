#!/usr/bin/env python3
"""Pure local exact-context classifier checks and retained-source scan; no archive copy."""
from pathlib import Path
import ast,base64,hashlib,json,re,runpy,datetime
ROOT=Path('/tmp/fc-round68-production-https/evidence')
HELPER=ROOT/'archive-evidence-third.py'

def main():
 ns=runpy.run_path(str(HELPER));need=ns['need'];sha=ns['sha']
 contexts=ns['validate_reviewed_contexts']();checks=[]
 static=(ROOT/ns['STATIC_READY_PATH']).read_bytes();text=static.decode('utf-8')
 short=re.compile(r'(?i)\\?[\x22\x27]pairingCode\\?[\x22\x27]\s*:\s*\\?[\x22\x27]([0-9]{8})\\?[\x22\x27]')
 matches=list(short.finditer(text));need(len(matches)==1,'single static fixture match');m=matches[0]
 static_label='source:'+ns['STATIC_READY_PATH']
 for prefix in ('source:','stored:'):
  need(ns['reviewed_static_ready'](static,prefix+ns['STATIC_READY_PATH'],'utf8',text,m),'exact static source/stored classifier')
  checks.append('static exact '+prefix[:-1])
 for name,data,label,encoding,view in [
  ('different path',static,'source:elsewhere.py','utf8',text),
  ('decoded nested location',static,static_label+'::/base64','utf8',text),
  ('changed bytes',static+b'\n',static_label,'utf8',text),
  ('different encoding',static,static_label,'utf-16-le',text),
  ('different line',static,static_label,'utf8','\n'+text)]:
  # For line shift, the regexp points at its real shifted occurrence.
  candidate=short.search(view)
  need(not ns['reviewed_static_ready'](data,label,encoding,view,candidate),'static negative '+name);checks.append('reject static '+name)
 report=json.loads((ROOT/ns['SSH_REPORT_PATH']).read_bytes());raw=base64.b64decode(report['sshTunnelRaw']['stderr']['base64'],validate=True);view=raw.decode('utf-8-sig')
 pattern=ns['PATTERNS']['openai-secret'];matches=list(pattern.finditer(view));need(len(matches)==1,'single original diagnostic match');m=matches[0]
 ssh_label='source:'+ns['SSH_REPORT_PATH']+'::/sshTunnelRaw/stderr/base64'
 for prefix in ('source:','stored:'):
  need(ns['reviewed_public_algorithm']('openai-secret',raw,prefix+ns['SSH_REPORT_PATH']+'::/sshTunnelRaw/stderr/base64','utf8',view,m),'exact SSH source/stored classifier');checks.append('public algorithm exact '+prefix[:-1])
 for name,kind,data,label,encoding,text_view in [
  ('different path','openai-secret',raw,'source:elsewhere.json::/base64','utf8',view),
  ('different JSON location','openai-secret',raw,ssh_label+'/other','utf8',view),
  ('changed bytes','openai-secret',raw+b'\n',ssh_label,'utf8',view),
  ('different encoding','openai-secret',raw,ssh_label,'utf-16-le',view),
  ('different kind','github-token',raw,ssh_label,'utf8',view),
  ('different line','openai-secret',raw,ssh_label,'utf8','\n'+view)]:
  candidate=pattern.search(text_view)
  need(not ns['reviewed_public_algorithm'](kind,data,label,encoding,text_view,candidate),'algorithm negative '+name);checks.append('reject public algorithm '+name)
 # Actual credential-like samples use only in-memory generated strings; they are
 # never issued credentials and their values are neither stored nor printed.
 for name,value in [('opaque', 'sk-'+('A'*40)),('project','sk-'+'proj-'+('A'*40)),('service','sk-'+'svcacct-'+('A'*40))]:
  match=pattern.search(value);need(match is not None,'pattern recognizes generated '+name)
  need(not ns['reviewed_public_algorithm']('openai-secret',value.encode(),ssh_label,'utf8',value,match),'opaque credential remains rejected '+name)
  checks.append('reject generated credential-like '+name)
 allowed={}
 for name in ['windows-business-tls-first.json','windows-header-deadline-first.json']:
  v=json.loads((ROOT/name).read_bytes());r=Path(v['rawPreservation']['directory']);data=(r/'tls.stdout').read_bytes()
  meta=v['rawPreservation']['files']['tls.stdout']['remote'];need(len(data)==meta['bytes'] and sha(data)==meta['sha256'],'exact expired fixture original')
  txt=data.decode('utf-16') if data.startswith((b'\xff\xfe',b'\xfe\xff')) else data.decode('utf-8-sig')
  ready=[json.loads(line.split(' ',1)[1]) for line in txt.splitlines() if line.startswith('TLS_BUSINESS_READY ')]
  need(len(ready)==1 and ready[0]['marker']==v['marker'],'expired fixture identity')
  allowed[name]={ready[0]['pairingCode']};allowed[r.relative_to(ROOT).as_posix()+'/']={ready[0]['pairingCode']}
 tree=ast.parse(HELPER.read_text());main_node=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='main')
 scan_node=next(n for n in main_node.body if isinstance(n,ast.FunctionDef) and n.name=='scan')
 env=dict(ns);env.update(findings=[],expired_annotations=[],public_annotations=[],allowed_codes=allowed,
  stats={'sourceFiles':0,'storedFiles':0,'gzipFiles':0,'utf16Views':0,'decodedRawPayloads':0,'jsonContainers':0,'textViews':0})
 exec(compile(ast.Module(body=[scan_node],type_ignores=[]),str(HELPER),'exec'),env)
 files=[]
 for p in sorted(ROOT.rglob('*')):
  if p.is_dir() or ns['excluded'](p):continue
  data=p.read_bytes();rel=p.relative_to(ROOT).as_posix()
  env['scan'](data,'source:'+rel);env['stats']['sourceFiles']+=1
  files.append({'path':rel,'bytes':len(data),'sha256':sha(data)})
 need(not env['findings'],'source scan findings by path/kind only: '+json.dumps(env['findings']))
 out={'schemaVersion':1,'status':'PASS_PURE_EXACT_CONTEXT_CHECKS_AND_RETAINED_SOURCE_SCAN_ONLY',
  'recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
  'helper':{'path':HELPER.name,'bytes':HELPER.stat().st_size,'sha256':sha(HELPER.read_bytes())},
  'checks':checks,'checkCount':len(checks),'reviewedContexts':contexts,
  'credentialScan':{'status':'PASS_RETAINED_SOURCE_ONLY_NOT_ARCHIVE_VERIFICATION','statistics':env['stats'],'findings':[],
   'reviewedPublicContextOccurrences':env['public_annotations'],'expiredSyntheticShortCodeOccurrenceCount':len(env['expired_annotations'])},
  'sourceInventory':files,'scope':'No archive copy, source change, Windows, network or Cargo. The archive helper still independently scans actual stored files when root executes it. No matched value or hash of a pairing code is recorded.'}
 data=(json.dumps(out,ensure_ascii=False,indent=2)+'\n').encode()
 path=ROOT/'archive-reviewed-context-check-first.json'
 with path.open('xb') as f:f.write(data)
 print(json.dumps({'status':out['status'],'checkCount':len(checks),'sourceFiles':len(files),'helperSha256':out['helper']['sha256'],'reportSha256':sha(data),'archiveExecuted':False}))
if __name__=='__main__':main()
