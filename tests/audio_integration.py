"""Real PostgreSQL + Rust + multipart audio + ASR HTTP contract."""
import json,time,io,wave,urllib.request,unittest,os,subprocess,concurrent.futures
from assistant_integration import client,req,BASE,MODEL,model
ASR=os.environ.get('WHISPER_TEST_URL','http://127.0.0.1:11436')
def control(mode):
 urllib.request.urlopen(urllib.request.Request(ASR+'/control',data=json.dumps({'mode':mode}).encode(),headers={'Content-Type':'application/json'})).read()
def upload(c,notes='',title=None):
 b=io.BytesIO()
 with wave.open(b,'wb') as w:w.setparams((1,2,16000,0,'NONE','none'));w.writeframes(b'\0\0'*48000)
 boundary='----audio-test-boundary'
 fields={'title':title or f'Аудио тест {time.time_ns()}','date':'2026-11-12T12:00:00+03:00','notes':notes,'actions':'{}','participantIds':'[]'}
 body=b''
 for k,v in fields.items():body+=f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode()
 body+=f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="clip.wav"\r\nContent-Type: audio/wav\r\n\r\n'.encode()+b.getvalue()+f'\r\n--{boundary}--\r\n'.encode()
 with c.open(urllib.request.Request(BASE+'/api/meetings',method='POST',data=body,headers={'Content-Type':'multipart/form-data; boundary='+boundary})) as r:return json.load(r)
def wait(c,mid):
 for _ in range(100):
  _,m=req(c,'GET',f'/meetings/{mid}')
  if m['status']!='processing':return m
  time.sleep(.1)
 raise AssertionError('processing timed out')
class AudioTests(unittest.TestCase):
 def setUp(self):
  self.c=client();status,r=req(self.c,'POST','/auth/register',{'name':'Аудио','email':f'audio-http-{time.time_ns()}@test.local','password':'testing-password-123'});self.assertIn(status,(200,201),r)
 def test_01_recognized_audio_is_stored_and_given_to_ai(self):
  control('ok');m=upload(self.c);m=wait(self.c,m['id'])
  self.assertEqual(m['status'],'ready',m);self.assertEqual(m['audioDuration'],3)
  self.assertIn('Контрольный оттиск',m['transcript'][0]['text'])
  sent=json.load(urllib.request.urlopen(ASR+'/last'));self.assertTrue(all(sent[k] for k in ['has_file','has_model','has_language','mime_audio']),sent)
  status,r=req(self.c,'POST',f'/meetings/{m["id"]}/ask',{'question':'Что согласовали?'})
  self.assertEqual(status,200,r)
  payload=json.load(urllib.request.urlopen(MODEL+'/last'));self.assertIn('Контрольный оттиск',payload['prompt'])
  _,d=req(self.c,'POST','/assistant/dialogs',{});model([], 'Контрольный оттиск согласован')
  status,r=req(self.c,'POST',f'/assistant/dialogs/{d["id"]}/messages',{'text':f'Что было сказано в аудиозаписи встречи {m["id"]}?'})
  self.assertEqual(status,200,r)
  payload=json.load(urllib.request.urlopen(MODEL+'/last'));self.assertIn('Контрольный оттиск',payload['prompt'])
  self.assertIn('transcriptionAvailable',payload['prompt'])
 def test_02_error_is_visible_and_saved_audio_can_be_retried(self):
  control('error');m=upload(self.c,'Заметки без расшифровки');m=wait(self.c,m['id'])
  self.assertEqual(m['status'],'ready',m);self.assertIn('503',m['errorMessage']);self.assertEqual(m['transcript'],[])
  status,answer=req(self.c,'POST',f'/meetings/{m["id"]}/ask',{'question':'Что сказано в аудио?'})
  self.assertEqual(status,200,answer);self.assertIn('не распознано',answer['answer'])
  control('ok');status,r=req(self.c,'POST',f'/meetings/{m["id"]}/transcribe',{});self.assertEqual(status,202,r)
  m=wait(self.c,m['id']);self.assertEqual(m['status'],'ready',m);self.assertTrue(m['transcript']);self.assertIsNone(m['errorMessage'])
  status,r=req(client(),'POST',f'/meetings/{m["id"]}/transcribe',{});self.assertEqual(status,401,r)
 def test_03_audio_without_notes_fails_honestly_and_retry_missing_file(self):
  control('error');m=wait(self.c,upload(self.c)['id']);self.assertEqual(m['status'],'error',m);self.assertIn('503',m['errorMessage'])
  status,r=req(self.c,'POST','/meetings/99999999/transcribe',{});self.assertEqual(status,404,r)
 def test_04_retry_creates_requested_tasks_once(self):
  control('error');m=wait(self.c,upload(self.c)['id']);self.assertEqual(m['status'],'error')
  self.assertEqual(req(self.c,'GET',f'/tasks?meetingId={m["id"]}')[1]['total'],0)
  control('ok');self.assertEqual(req(self.c,'POST',f'/meetings/{m["id"]}/transcribe',{})[0],202)
  m=wait(self.c,m['id']);self.assertEqual(m['status'],'ready',m)
  count=req(self.c,'GET',f'/tasks?meetingId={m["id"]}')[1]['total'];self.assertGreater(count,0)
  self.assertEqual(req(self.c,'POST',f'/meetings/{m["id"]}/transcribe',{})[0],202)
  m=wait(self.c,m['id']);self.assertEqual(req(self.c,'GET',f'/tasks?meetingId={m["id"]}')[1]['total'],count)
  with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
   results=list(executor.map(lambda _:req(self.c,'POST',f'/meetings/{m["id"]}/transcribe',{}),range(2)))
  self.assertTrue(all(status in (202,409) for status,_ in results),results)
  m=wait(self.c,m['id']);self.assertEqual(req(self.c,'GET',f'/tasks?meetingId={m["id"]}')[1]['total'],count)
 def test_05_final_db_error_is_a_terminal_failure(self):
  url=os.environ.get('ASSISTANT_TEST_DATABASE_URL')
  if not url:self.skipTest('Set ASSISTANT_TEST_DATABASE_URL for database-failure injection')
  tool=os.environ.get('PSQL','psql')
  sql="""CREATE OR REPLACE FUNCTION audio_test_ready_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test final update failure'; END $$;
CREATE TRIGGER audio_test_ready_failure BEFORE UPDATE ON meetings FOR EACH ROW WHEN (NEW.status='ready' AND NEW.title LIKE 'DBfail audio %') EXECUTE FUNCTION audio_test_ready_failure();"""
  subprocess.run([tool,url,'-v','ON_ERROR_STOP=1','-c',sql],check=True,capture_output=True)
  try:
   control('ok');m=wait(self.c,upload(self.c,title=f'DBfail audio {time.time_ns()}')['id'])
   self.assertEqual(m['status'],'error',m);self.assertIn('сохранить',m['errorMessage'])
  finally:subprocess.run([tool,url,'-c','DROP TRIGGER IF EXISTS audio_test_ready_failure ON meetings; DROP FUNCTION IF EXISTS audio_test_ready_failure();'],check=True,capture_output=True)

if __name__=='__main__':unittest.main(verbosity=2)
