"""Local deterministic Ollama stand-in for integration and browser tests only."""
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
next_reply={'answer':'Чем помочь?','actions':[]}
last_request={}
lock=threading.Lock()
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        data=json.dumps(last_request).encode()
        self.send_response(200); self.send_header('Content-Type','application/json'); self.end_headers(); self.wfile.write(data)
    def do_POST(self):
        global next_reply, last_request
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path not in ('/next','/api/generate'):
            self.send_response(404); self.end_headers(); return
        with lock:
            if self.path=='/next': next_reply=body; reply={'ok':True}
            elif 'highlight' in body.get('format',{}).get('properties',{}):
                reply={'response':json.dumps({'highlight':'Обсудили заказ','items':[],'tasks':[{'formulation':'Проверить печать','owner':'Не указан','deadline':'завтра','basis':'Текст встречи'}]})}
            else: last_request=body; reply={'response':json.dumps(next_reply,ensure_ascii=False)}
        data=json.dumps(reply).encode()
        self.send_response(200); self.send_header('Content-Type','application/json'); self.send_header('Content-Length',str(len(data))); self.end_headers(); self.wfile.write(data)
    def log_message(self,*args): pass
if __name__=='__main__': ThreadingHTTPServer(('127.0.0.1',11435),Handler).serve_forever()
