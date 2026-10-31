import http.server,ssl,json,hashlib,signal,os,uuid,threading
from pathlib import Path
ROOT=Path('/tmp/fc-round30-browser-upgrade');EV=Path('/tmp/fc-round30-evidence');TOKEN=uuid.uuid4().hex
CSP="default-src 'none'; script-src 'self'; worker-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' blob: data:; media-src blob:; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'"
assets={}
for deployment in ['a','b']:
 m=json.loads((ROOT/deployment/'fc-assets.json').read_text());assets[deployment]={}
 for a in m['assets']:
  d=(ROOT/deployment/a['path']).read_bytes();assert len(d)==a['sizeBytes'] and hashlib.sha256(d).hexdigest()==a['sha256'];assets[deployment][a['path']]=(a,d)
active='b';loglock=threading.Lock()
class Handler(http.server.BaseHTTPRequestHandler):
 protocol_version='HTTP/1.1'
 def log_message(self,*args):pass
 def reply(self,status,data=b'',kind='application/json; charset=utf-8',etag=None,cache='no-store',deployment=None):
  self.send_response(status);self.send_header('Content-Type',kind);self.send_header('Content-Length',str(len(data)));self.send_header('Connection','close');self.send_header('Content-Security-Policy',CSP);self.send_header('Cache-Control',cache);self.send_header('X-Content-Type-Options','nosniff');self.send_header('Referrer-Policy','no-referrer');self.send_header('Permissions-Policy','camera=(), geolocation=(), microphone=(self)')
  if etag:self.send_header('ETag',etag)
  self.end_headers()
  try:self.wfile.write(data);self.wfile.flush()
  except (BrokenPipeError,ConnectionResetError,ssl.SSLEOFError):pass
  with loglock:
   with (EV/'browser-upgrade-server-requests.jsonl').open('a') as f:f.write(json.dumps({'method':self.command,'path':self.path,'status':status,'bytes':len(data),'deployment':deployment,'etag':etag})+'\n')
 def do_PUT(self):
  global active
  if self.path not in [f'/__qa/{TOKEN}/deployment/a',f'/__qa/{TOKEN}/deployment/b'] or self.headers.get('Content-Length','0')!='0':return self.reply(404,b'{}')
  active=self.path[-1];self.reply(200,json.dumps({'active':active}).encode())
 def do_GET(self):
  selected=active
  if not self.path.startswith('/fc/'):return self.reply(404,b'{}',deployment=selected)
  path=self.path[len('/fc/'):];path='index.html' if path=='' else path
  if path not in assets[selected]:return self.reply(404,b'{}',deployment=selected)
  a,data=assets[selected][path];etag='"'+a['sha256']+'"';cache='no-cache' if path=='index.html' else 'public, max-age=31536000, immutable'
  if self.headers.get('If-None-Match')==etag:return self.reply(304,b'',etag=etag,cache=cache,deployment=selected)
  mime={'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.svg':'image/svg+xml','.png':'image/png'}[Path(path).suffix]
  self.reply(200,data,mime,etag,cache,selected)
server=http.server.ThreadingHTTPServer(('0.0.0.0',0),Handler);context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(ROOT/'server-cert.pem',ROOT/'server-key.pem');server.socket=context.wrap_socket(server.socket,server_side=True)
info={'pid':os.getpid(),'procStart':Path('/proc/self/stat').read_text().rsplit(') ',1)[1].split()[19],'port':server.server_address[1],'controlToken':TOKEN,'scope':'owned synthetic static HTTPS fixture; no OS trust mutation'};(EV/'browser-upgrade-server-pid.json').write_text(json.dumps(info,indent=2)+'\n');print(json.dumps(info),flush=True)
signal.signal(signal.SIGTERM,lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
try:server.serve_forever()
except KeyboardInterrupt:pass
finally:server.server_close();print('OWNED_BROWSER_FIXTURE_CLOSED',flush=True)
