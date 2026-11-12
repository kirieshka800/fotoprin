"""Deletion tests use only isolated test DB and newly created records."""
import time,unittest,os,subprocess,concurrent.futures
from assistant_integration import client,req,model
from audio_integration import upload,wait,control
class DeletionTests(unittest.TestCase):
 @classmethod
 def setUpClass(cls):
  cls.c=client();cls.other=client();cls.name=f'Delete Other {time.time_ns()}'
  for c,name in [(cls.c,f'Delete Manager {time.time_ns()}'),(cls.other,cls.name)]:
   status,r=req(c,'POST','/auth/register',{'name':name,'email':f'delete-{time.time_ns()}@test.local','password':'testing-password-123'});assert status in (200,201),r
  req(cls.other,'GET','/notifications')
 def dialog(self,c=None):return req(c or self.c,'POST','/assistant/dialogs',{})[1]['id']
 def test_01_dialog_owner_and_persisted_cascade(self):
  did=self.dialog();model([{'type':'create_task','title':'Pending deleted chat','description':'','assigneeId':None,'due':'2027-12-01T15:00:00+03:00','priority':'Обычный'}])
  status,message=req(self.c,'POST',f'/assistant/dialogs/{did}/messages',{'text':'Создай задачу для теста'});self.assertEqual(status,200,message);pid=message['proposals'][0]['id']
  self.assertEqual(req(self.other,'DELETE',f'/assistant/dialogs/{did}')[0],404)
  self.assertEqual(req(self.c,'DELETE',f'/assistant/dialogs/{did}')[0],200)
  self.assertEqual(req(self.c,'GET',f'/assistant/dialogs/{did}')[0],404)
  self.assertEqual(req(self.c,'POST',f'/assistant/proposals/{pid}/confirm',{})[0],404)
  self.assertEqual(req(self.c,'POST',f'/assistant/dialogs/{did}/messages',{'text':'Привет'})[0],404)
 def test_02_clear_dialogs_only_deletes_current_owner(self):
  own=self.dialog();foreign=self.dialog(self.other)
  self.assertEqual(req(self.c,'DELETE','/assistant/dialogs')[0],200)
  self.assertEqual(req(self.c,'GET','/assistant/dialogs')[1],[])
  self.assertEqual(req(self.other,'GET',f'/assistant/dialogs/{foreign}')[0],200)
 def test_03_notification_delete_and_clear_are_owner_scoped(self):
  _,people=req(self.c,'GET','/employees');emp=next(e for e in people if e['name']==self.name)
  for i in range(2):
   status,t=req(self.c,'POST','/tasks',{'title':f'Delete notice {time.time_ns()}','assigneeId':emp['id'],'due':'2027-12-01T15:00:00+03:00'});self.assertEqual(status,201,t)
  _,data=req(self.other,'GET','/notifications');self.assertGreaterEqual(len(data['items']),2);nid=data['items'][0]['id']
  self.assertEqual(req(self.c,'DELETE',f'/notifications/{nid}')[0],404)
  self.assertEqual(req(self.other,'DELETE',f'/notifications/{nid}')[0],200)
  self.assertEqual(req(self.c,'DELETE','/notifications')[0],200)
  self.assertGreater(req(self.other,'GET','/notifications')[1]['counts']['all'],0)
  self.assertEqual(req(self.other,'DELETE','/notifications')[0],200)
  self.assertEqual(req(self.other,'GET','/notifications')[1]['counts']['all'],0)
 def test_04_delete_summary_preserves_audio_transcript_and_tasks(self):
  control('ok');m=wait(self.c,upload(self.c)['id']);self.assertEqual(m['status'],'ready',m)
  mid=m['id'];self.assertTrue(m['summary']);tasks=req(self.c,'GET',f'/tasks?meetingId={mid}')[1]['total']
  task_ids=[t['id'] for t in req(self.c,'GET',f'/tasks?meetingId={mid}')[1]['items']]
  self.assertEqual(req(self.c,'DELETE',f'/meetings/{mid}/summary')[0],200)
  _,fresh=req(self.c,'GET',f'/meetings/{mid}');self.assertIsNone(fresh['summary']);self.assertEqual(fresh['audioUrl'],m['audioUrl']);self.assertEqual(fresh['transcript'],m['transcript'])
  self.assertEqual(req(self.c,'GET',f'/tasks?meetingId={mid}')[1]['total'],tasks)
  self.assertEqual(req(self.c,'DELETE',f'/meetings/{mid}')[0],200)
  self.assertEqual(req(self.c,'GET',f'/meetings/{mid}')[0],404)
  self.assertEqual(req(self.c,'GET',f'/tasks?meetingId={mid}')[1]['total'],0)
  for tid in task_ids:
   status,t=req(self.c,'GET',f'/tasks/{tid}');self.assertEqual(status,200,t);self.assertIsNone(t['meetingId'])
 def test_05_task_delete_removes_reminder(self):
  _,t=req(self.c,'POST','/tasks',{'title':f'Delete task {time.time_ns()}','due':'2027-12-01T15:00:00+03:00'});tid=t['id']
  self.assertEqual(req(self.c,'PUT',f'/tasks/{tid}/reminder',{'before':'1h','channel':'Только почта'})[0],200)
  url=os.environ.get('ASSISTANT_TEST_DATABASE_URL');rid=None
  if url:
   rid=subprocess.run([os.environ.get('PSQL','psql'),url,'-tAc',f'SELECT id FROM reminders WHERE task_id={int(tid)}'],capture_output=True,text=True,check=True).stdout.strip()
  self.assertEqual(req(self.c,'DELETE',f'/tasks/{tid}')[0],200)
  self.assertEqual(req(self.c,'GET',f'/tasks/{tid}')[0],404)
  if rid:
   remaining=subprocess.run([os.environ.get('PSQL','psql'),url,'-tAc',f'SELECT COUNT(*) FROM reminders WHERE id={int(rid)}'],capture_output=True,text=True,check=True).stdout.strip()
   self.assertEqual(remaining,'0')

 def test_06_logged_out_delete_is_rejected(self):
  for path in ['/assistant/dialogs','/assistant/dialogs/1','/notifications','/notifications/1','/meetings/1/summary','/meetings/1','/tasks/1']:
   self.assertEqual(req(client(),'DELETE',path)[0],401,path)

 def test_07_current_role_blocks_workspace_deletion_but_allows_own_chats(self):
  url=os.environ.get('ASSISTANT_TEST_DATABASE_URL')
  if not url:self.skipTest('Set test database URL for role check')
  uid=req(self.other,'GET','/me')[1]['id'];did=self.dialog(self.other)
  tool=os.environ.get('PSQL','psql')
  def sql(text):subprocess.run([tool,url,'-v','ON_ERROR_STOP=1','-c',text],check=True,capture_output=True)
  sql(f"UPDATE users SET role='printer' WHERE id={int(uid)}")
  try:
   for path in ['/meetings/999999','/meetings/999999/summary','/tasks/999999']:
    self.assertEqual(req(self.other,'DELETE',path)[0],403,path)
   self.assertEqual(req(self.other,'DELETE',f'/assistant/dialogs/{did}')[0],200)
   self.assertEqual(req(self.other,'DELETE','/notifications')[0],200)
  finally:sql(f"UPDATE users SET role='manager' WHERE id={int(uid)}")
 def test_08_task_delete_and_confirmation_do_not_deadlock(self):
  url=os.environ.get('ASSISTANT_TEST_DATABASE_URL')
  if not url:self.skipTest('Set test database URL for race check')
  tool=os.environ.get('PSQL','psql')
  def sql(text):subprocess.run([tool,url,'-v','ON_ERROR_STOP=1','-c',text],check=True,capture_output=True)
  _,t=req(self.c,'POST','/tasks',{'title':f'Delete race {time.time_ns()}','due':'2027-12-01T15:00:00+03:00'});tid=t['id']
  req(self.c,'PUT',f'/tasks/{tid}/reminder',{'before':'1h','channel':'Только почта'})
  did=self.dialog();model([{'type':'create_task','title':f'Parallel create {time.time_ns()}','description':'','assigneeId':None,'due':'2027-12-01T15:00:00+03:00','priority':'Обычный'}])
  status,m=req(self.c,'POST',f'/assistant/dialogs/{did}/messages',{'text':'Создай задачу для теста'});self.assertEqual(status,200,m);pid=m['proposals'][0]['id']
  sql(f"""CREATE OR REPLACE FUNCTION delete_test_pause() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(0.8); RETURN OLD; END $$;
CREATE TRIGGER delete_test_pause BEFORE DELETE ON reminders FOR EACH ROW WHEN(OLD.task_id={int(tid)}) EXECUTE FUNCTION delete_test_pause();""")
  try:
   with concurrent.futures.ThreadPoolExecutor(max_workers=2) as ex:
    deletion=ex.submit(req,self.c,'DELETE',f'/tasks/{tid}')
    time.sleep(.2)
    confirmation=ex.submit(req,self.c,'POST',f'/assistant/proposals/{pid}/confirm',{})
    results=[deletion.result(timeout=15),confirmation.result(timeout=15)]
   self.assertEqual([r[0] for r in results],[200,200],results)
  finally:sql('DROP TRIGGER IF EXISTS delete_test_pause ON reminders; DROP FUNCTION IF EXISTS delete_test_pause();')
 def test_09_same_names_cannot_share_or_delete_notifications(self):
  name=f'Same Name {time.time_ns()}';a=client();b=client()
  for c in [a,b]:
   status,r=req(c,'POST','/auth/register',{'name':name,'email':f'same-name-{time.time_ns()}@test.local','password':'testing-password-123'});self.assertIn(status,(200,201),r)
  req(a,'GET','/notifications')
  emp=next(e for e in req(self.c,'GET','/employees')[1] if e['name']==name)
  req(self.c,'POST','/tasks',{'title':f'Private notice {time.time_ns()}','assigneeId':emp['id'],'due':'2027-12-01T15:00:00+03:00'})
  first=req(a,'GET','/notifications')[1];self.assertGreater(first['counts']['all'],0);nid=first['items'][0]['id']
  self.assertEqual(req(b,'GET','/notifications')[1]['counts']['all'],0)
  self.assertEqual(req(b,'DELETE',f'/notifications/{nid}')[0],404)
  self.assertEqual(req(b,'DELETE','/notifications')[0],200)
  self.assertGreater(req(a,'GET','/notifications')[1]['counts']['all'],0)
 def test_10_concurrent_equal_names_cannot_claim_one_employee(self):
  url=os.environ.get('ASSISTANT_TEST_DATABASE_URL')
  if not url:self.skipTest('Set database URL for employee-claim race')
  name=f'Unbound Same {time.time_ns()}';a=client();b=client()
  for c in [a,b]:
   status,r=req(c,'POST','/auth/register',{'name':name,'email':f'claim-{time.time_ns()}@test.local','password':'testing-password-123'});self.assertIn(status,(200,201),r)
  _,emp=req(self.c,'POST','/employees',{'name':name,'position':'Печатник','department':'Производство'})
  req(self.c,'POST','/tasks',{'title':f'Private before claim {time.time_ns()}','assigneeId':emp['id'],'due':'2027-12-01T15:00:00+03:00'})
  tool=os.environ.get('PSQL','psql')
  def sql(text):subprocess.run([tool,url,'-v','ON_ERROR_STOP=1','-c',text],check=True,capture_output=True)
  sql(f"""CREATE OR REPLACE FUNCTION claim_test_pause() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(0.4); RETURN NEW; END $$;
CREATE TRIGGER claim_test_pause BEFORE UPDATE ON employees FOR EACH ROW WHEN(OLD.id={int(emp['id'])} AND OLD.email IS NULL) EXECUTE FUNCTION claim_test_pause();""")
  try:
   with concurrent.futures.ThreadPoolExecutor(max_workers=2) as ex:results=list(ex.map(lambda c:req(c,'GET','/notifications'),[a,b]))
   self.assertEqual([r[0] for r in results],[200,200],results)
   self.assertEqual(sorted(r[1]['counts']['all'] for r in results),[0,1],results)
  finally:sql('DROP TRIGGER IF EXISTS claim_test_pause ON employees; DROP FUNCTION IF EXISTS claim_test_pause();')
