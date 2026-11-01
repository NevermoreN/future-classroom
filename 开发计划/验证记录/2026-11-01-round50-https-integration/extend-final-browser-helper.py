from pathlib import Path
E=Path('/tmp/fc-round50-https-integration-evidence')
p=E/'verify-final-browser-readonly-first.py';s=p.read_text().replace("OUT=EV/'browser-readonly-independent-first.json'","OUT=EV/'browser-readonly-independent-second.json'")
s=s.replace("'readonly':str(readonly_path)","'readonlyReport':str(readonly_path)")
extra='''
def validate_no_failure(n):
 if n.get('status')!='PASS' or 'error' in n or any('fail' in k.lower() for k in n):raise ValueError('unexpected failure diagnostics')
def validate_api(network,n):
 rows=[x for x in network if urlsplit(x['url']).path.startswith('/fc/api/')]
 contents={urlsplit(x['url']).path for x in rows if re.fullmatch(r'/fc/api/v1/content/homework/1/1/1/[0-9]{4}-[0-9]{2}-[0-9]{2}',urlsplit(x['url']).path)}
 if len(contents)!=1:raise ValueError('scope')
 content=next(iter(contents));routes=Counter((x['method'],urlsplit(x['url']).path) for x in rows)
 expected=Counter({('GET','/fc/api/v1/status'):2,('GET','/fc/api/v1/session'):2,('GET','/fc/api/v1/content/homework/scopes/0'):2,('GET',content):2,('POST',content):1,('GET',content+'/visual'):4})
 if routes!=expected or len(rows)!=n['apiResponses'] or n['contentPosts']!=1:raise ValueError('api counts')
 flags=['servedFromCache','fromDiskCache','fromServiceWorker','fromPrefetchCache']
 if any(x['wireStatus']!=200 or x['status']!=200 or any(x[f] for f in flags) for x in rows):raise ValueError('cached API')
 return rows

def check_control_scripts(r):
 tree=ast.parse(input_bytes(SCRIPTS/'windows-tls-probe.py').decode())
 keys={'COMMON','SETUP','LAUNCH','STOP','CAPTURE','CLEANUP'}
 literals={t.id:ast.literal_eval(row.value) for row in tree.body if isinstance(row,ast.Assign) for t in row.targets if isinstance(t,ast.Name) and t.id in keys}
 for name in ['LAUNCH','STOP']:literals[name]=literals[name].replace('--tls-lifecycle-probe','--tls-business-probe')
 check('exact underlying remote templates digest',sha(''.join(literals[k] for k in ['COMMON','SETUP','LAUNCH','STOP','CAPTURE','CLEANUP']).encode())==r['remoteScriptsSha256'])
 for name,key in [('cleanup','CLEANUP'),('stop','STOP'),('capture','CAPTURE'),('setup','SETUP'),('launch','LAUNCH')]:
  rendered=literals[key]
  replacements={'__ROOT__':r['remoteRoot'],'__MARKER__':r['marker'],'__EXENAME__':'fc-bootstrap.exe','__THUMBPRINT__':r['fixture']['thumbprint'] if name=='launch' else ''}
  for token,value in replacements.items():rendered=rendered.replace(token,"'"+value.replace("'","''")+"'")
  exact=(literals['COMMON']+rendered).encode('utf-8-sig')
  check('regenerated exact uploaded native control '+name,sha(exact)==r['uploadedScriptSha256'][name])
'''
s=s.replace('\ndef audit(browser_path',extra+'\ndef audit(browser_path')
s=s.replace("    check('successful PASS contains no failure diagnostics'", "    validate_no_failure(n);validate_api(n['network'],n);check_control_scripts(r)\n    check('successful PASS contains no failure diagnostics'")
s=s.replace("  audit(R46/", "  check('modern normal source artifact exact SHA',audit46['normalExecutable']['producer']['sha256']=='bc8f224a97797311e76182486bfa6502f8c11110e3a7151893f5212ff9d911aa')\n  check('R50 normal actual429-source runtime exact SHA',prior['normalSha256']=='b6368400a58b87d4725cbd664b5bde88a4f51927a74656e5c566010e8bbf6930' and prior['sourceCount']==429)\n  audit(R46/",1)
(E/'verify-final-browser-readonly-second.py').write_text(s)
