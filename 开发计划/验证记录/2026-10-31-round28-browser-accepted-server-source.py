import http.server
import ssl
import json
import os
from pathlib import Path

class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory='/tmp/fc-round28-web/dist', **kwargs)

    def do_GET(self):
        if not self.path.startswith('/fc/'):
            self.send_error(404)
            return
        self.path = self.path[3:]
        return super().do_GET()

server = http.server.ThreadingHTTPServer(('0.0.0.0', 36029), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain('/tmp/fc-round28-browser-accepted/server-cert.pem', '/tmp/fc-round28-browser-accepted/server-key.pem')
server.socket = context.wrap_socket(server.socket, server_side=True)
Path('/tmp/fc-round28-evidence/browser-accepted-server-pid.json').write_text(json.dumps({'pid': os.getpid(), 'port': 36029, 'scope': 'owned synthetic static fixture; no OS trust mutation'}))
server.serve_forever()
