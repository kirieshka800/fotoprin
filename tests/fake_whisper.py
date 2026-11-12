"""ASR contract/error stand-in. Real recognition is checked separately."""
import json
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
mode='ok'
last={}
class H(BaseHTTPRequestHandler):
 def do_GET(self):
  data=json.dumps(last).encode();self.send_response(200);self.end_headers();self.wfile.write(data)
 def do_POST(self):
  global mode,last
  raw=self.rfile.read(int(self.headers.get('Content-Length',0)))
  if self.path=='/control': mode=json.loads(raw)['mode'];data=b'{}';status=200
  elif self.path!='/v1/audio/transcriptions': data=b'{"error":"wrong endpoint"}';status=404
  else:
   last={'path':self.path,'has_file':b'name="file"' in raw,'has_model':b'test-asr' in raw,'has_language':b'ru' in raw,'mime_audio':b'audio/wav' in raw}
   status=503 if mode=='error' else 200
   result={'error':'test Whisper unavailable'} if mode=='error' else {'text':'Контрольный оттиск согласован. Заказ номер сто двадцать пять.','segments':[{'start':0,'end':3,'text':'Контрольный оттиск согласован. Заказ номер сто двадцать пять.'}],'duration':3}
   data=json.dumps(result,ensure_ascii=False).encode()
  self.send_response(status);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
 def log_message(self,*args):pass
if __name__=='__main__':ThreadingHTTPServer(('127.0.0.1',11436),H).serve_forever()
