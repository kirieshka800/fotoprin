"""Real HTTP + PostgreSQL tests. Start the app with tests/fake_ollama.py first.
ASSISTANT_TEST_URL defaults to http://127.0.0.1:8081. Uses fresh users and records.
"""
import concurrent.futures
import http.cookiejar
import json
import os
import time
import subprocess
from pathlib import Path
import unittest
import urllib.error
import urllib.request

BASE = os.environ.get('ASSISTANT_TEST_URL', 'http://127.0.0.1:8081')
MODEL = os.environ.get('ASSISTANT_FAKE_URL', 'http://127.0.0.1:11435')

def client():
    return urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

def req(c, method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    r = urllib.request.Request(BASE + '/api' + path, data=data, method=method, headers={'Content-Type':'application/json'})
    try:
        with c.open(r, timeout=200) as response:
            return response.status, json.load(response)
    except urllib.error.HTTPError as e:
        raw=e.read()
        return e.code, json.loads(raw) if raw else {}

def model(actions, answer=None):
    if answer is None: answer='Подготовил предложение. Проверьте параметры.' if actions else 'Чем помочь?' 
    data=json.dumps({'answer':answer,'actions':actions}).encode()
    with urllib.request.urlopen(urllib.request.Request(MODEL+'/next',data=data,headers={'Content-Type':'application/json'})) as r:
        r.read()

class AssistantTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.c=client()
        status,cls.user=req(cls.c,'POST','/auth/register',{'name':'Тест руководителя','email':f'assistant-{time.time_ns()}@test.local','password':'testing-password-123'})
        assert status in (200,201), cls.user
        cls.other=client()
        status,_=req(cls.other,'POST','/auth/register',{'name':'Другой пользователь','email':f'other-{time.time_ns()}@test.local','password':'testing-password-123'})
        assert status in (200,201)
    def proposal(self, actions):
        status,d=req(self.c,'POST','/assistant/dialogs',{})
        self.assertEqual(status,201,d)
        self.dialog=d['id']
        model(actions)
        status,m=req(self.c,'POST',f'/assistant/dialogs/{self.dialog}/messages',{'text':'Выполни тестовый запрос'})
        self.assertEqual(status,200,m)
        self.assertEqual(len(m['proposals']),len(actions),m)
        return m['proposals']
    def tasks(self):
        items=[]
        page=1
        while True:
            status,data=req(self.c,'GET',f'/tasks?pageSize=100&page={page}')
            self.assertEqual(status,200,data)
            items.extend(data['items'])
            if len(items)>=data['total'] or not data['items']: return items
            page+=1
    def create_action(self,title=None):
        return {'type':'create_task','title':title or f'AI test {time.time_ns()}','description':'Проверка подтверждения','assigneeId':None,'due':'2027-12-01T15:00:00+03:00','priority':'Высокий'}
    def confirm(self,p,c=None): return req(c or self.c,'POST',f'/assistant/proposals/{p["id"]}/confirm',{})
    def test_01_preview_reject_does_not_write(self):
        a=self.create_action()
        p=self.proposal([a])[0]
        self.assertFalse(any(t['title']==a['title'] for t in self.tasks()))
        self.assertTrue(p['details'])
        status,r=req(self.c,'POST',f'/assistant/proposals/{p["id"]}/reject',{})
        self.assertEqual(status,200,r)
        self.assertEqual(r['status'],'rejected')
        self.assertEqual(self.confirm(p)[0],409)
        self.assertFalse(any(t['title']==a['title'] for t in self.tasks()))
    def test_02_confirmation_is_idempotent_even_concurrently(self):
        a=self.create_action()
        p=self.proposal([a])[0]
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as ex:
            results=list(ex.map(lambda _:self.confirm(p),range(2)))
        self.assertTrue(all(s==200 for s,_ in results),results)
        self.assertEqual(results[0][1]['result'],results[1][1]['result'])
        self.assertEqual(sum(t['title']==a['title'] for t in self.tasks()),1)
        status,d=req(self.c,'GET',f'/assistant/dialogs/{self.dialog}')
        self.assertEqual(d['messages'][-1]['proposals'][0]['status'],'applied')
    def test_03_private_dialogs_and_proposals(self):
        p=self.proposal([self.create_action()])[0]
        self.assertEqual(req(self.other,'GET',f'/assistant/dialogs/{self.dialog}')[0],404)
        self.assertEqual(self.confirm(p,self.other)[0],404)
        self.assertEqual(req(self.other,'POST',f'/assistant/proposals/{p["id"]}/reject',{})[0],404)
        self.assertEqual(req(self.other,'POST',f'/assistant/dialogs/{self.dialog}/messages',{'text':'Привет'})[0],404)
        self.assertEqual(req(client(),'GET','/assistant/dialogs')[0],401)
    def test_04_stale_task_requires_new_proposal(self):
        p=self.proposal([self.create_action()])[0]
        _,p=self.confirm(p)
        task_id=p['result']['id']
        p=self.proposal([{'type':'update_task','id':task_id,'changes':{'title':'AI change'}}])[0]
        status,r=req(self.c,'PATCH',f'/tasks/{task_id}',{'title':'Manual change'})
        self.assertEqual(status,200,r)
        self.assertEqual(self.confirm(p)[0],409)
        self.assertEqual(req(self.c,'GET',f'/tasks/{task_id}')[1]['title'],'Manual change')
    def test_05_unknown_action_is_not_saved(self):
        status,d=req(self.c,'POST','/assistant/dialogs',{})
        model([{'type':'run_sql','sql':'DROP TABLE tasks'}])
        status,r=req(self.c,'POST',f'/assistant/dialogs/{d["id"]}/messages',{'text':'Тест'})
        self.assertEqual(status,502,r)
        self.assertEqual(req(self.c,'GET',f'/assistant/dialogs/{d["id"]}')[1]['messages'],[])
    def test_06_plain_answer_has_no_changes(self):
        p=self.proposal([])
        self.assertEqual(p,[])
    def test_07_schedule_batch_atomic_and_rest_checked(self):
        status,e=req(self.c,'POST','/employees',{'name':f'Печатник {time.time_ns()}','position':'Печатник','department':'Производство'})
        self.assertEqual(status,201,e)
        emp=e['id']
        p=self.proposal([{'type':'plan_shifts','shifts':[{'employeeId':emp,'date':'2027-01-03','time':'20:00-08:00','note':'Ночь'},{'employeeId':emp,'date':'2027-01-04','time':'09:00-18:00','note':'Конфликт'}]}])[0]
        self.assertEqual(self.confirm(p)[0],409)
        _,week=req(self.c,'GET','/schedule?weekStart=2027-01-04')
        self.assertFalse(any(s['employeeId']==emp for s in week['shifts']))
        p=self.proposal([{'type':'plan_shifts','shifts':[{'employeeId':emp,'date':'2027-01-03','time':'20:00-08:00','note':'Ночь'},{'employeeId':emp,'date':'2027-01-04','time':'20:00-08:00','note':'12 часов отдыха'}]}])[0]
        status,r=self.confirm(p)
        self.assertEqual(status,200,r)
    def test_08_task_update_comment_reminder(self):
        _,p=self.confirm(self.proposal([self.create_action()])[0]); tid=p['result']['id']
        for a in [{'type':'update_task','id':tid,'changes':{'status':'in_progress'}},{'type':'add_comment','id':tid,'text':'Комментарий из AI'},{'type':'set_reminder','id':tid,'before':'1h','channel':'Только почта'}]:
            status,r=self.confirm(self.proposal([a])[0]); self.assertEqual(status,200,r)
        _,t=req(self.c,'GET',f'/tasks/{tid}')
        self.assertEqual(t['status'],'in_progress')
        self.assertTrue(any(c['text']=='Комментарий из AI' for c in t['comments']))
    def test_09a_parallel_messages_do_not_exhaust_pool(self):
        ids=[req(self.c,'POST','/assistant/dialogs',{})[1]['id'] for _ in range(12)]
        model([])
        with concurrent.futures.ThreadPoolExecutor(max_workers=12) as ex:
            futures=[ex.submit(req,self.c,'POST',f'/assistant/dialogs/{did}/messages',{'text':'Какие есть задачи?'}) for did in ids]
            done,waiting=concurrent.futures.wait(futures,timeout=8)
            self.assertFalse(waiting,'Parallel messages exhausted the database pool')
            self.assertTrue(all(f.result()[0]==200 for f in done),[f.result() for f in done])

    def test_09_safe_settings_patch(self):
        _,old=req(self.c,'GET','/settings')
        status,r=self.confirm(self.proposal([{'type':'update_settings','changes':{'company':'Тестовая типография'}}])[0])
        self.assertEqual(status,200,r)
        _,new=req(self.c,'GET','/settings')
        self.assertEqual(new['company'],'Тестовая типография')
        self.assertEqual(new['timezone'],old['timezone'])

    def test_10_employee_and_task_delete(self):
        name=f'Сотрудник AI {time.time_ns()}'
        p=self.proposal([{'type':'create_employee','name':name,'position':'Печатник','department':'Производство'}])[0]
        status,r=self.confirm(p); self.assertEqual(status,200,r)
        _,people=req(self.c,'GET','/employees')
        self.assertTrue(any(e['name']==name for e in people))
        p=self.proposal([self.create_action()])[0]; _,r=self.confirm(p); tid=r['result']['id']
        p=self.proposal([{'type':'delete_task','id':tid}])[0]
        self.assertEqual(self.confirm(p)[0],200)
        self.assertEqual(req(self.c,'GET',f'/tasks/{tid}')[0],404)

    def test_11_publish_schedule_and_notifications(self):
        _,e=req(self.c,'POST','/employees',{'name':f'Смена AI {time.time_ns()}','position':'Печатник','department':'Производство'})
        a={'type':'plan_shifts','shifts':[{'employeeId':e['id'],'date':'2027-02-01','time':'09:00-18:00','note':'Печать'}]}
        self.assertEqual(self.confirm(self.proposal([a])[0])[0],200)
        p=self.proposal([{'type':'publish_schedule','weekStart':'2027-02-01'}])[0]
        self.assertEqual(self.confirm(p)[0],200)
        self.assertTrue(req(self.c,'GET','/schedule?weekStart=2027-02-01')[1]['published'])
        p=self.proposal([{'type':'read_all_notifications'}])[0]
        self.assertEqual(self.confirm(p)[0],200)

    def test_11a_manual_publication_invalidates_shift_preview(self):
        _,e=req(self.c,'POST','/employees',{'name':f'Публикация {time.time_ns()}','position':'Печатник','department':'Производство'})
        a={'type':'plan_shifts','shifts':[{'employeeId':e['id'],'date':'2027-03-01','time':'09:00-18:00','note':''}]}
        self.assertEqual(self.confirm(self.proposal([a])[0])[0],200)
        a['shifts'][0]['time']='10:00-18:00'
        p=self.proposal([a])[0]
        self.assertEqual(req(self.c,'POST','/schedule/publish',{'weekStart':'2027-03-01'})[0],200)
        self.assertEqual(self.confirm(p)[0],409)
        self.assertTrue(req(self.c,'GET','/schedule?weekStart=2027-03-01')[1]['published'])

    def test_12_navigation_is_whitelisted(self):
        _,d=req(self.c,'POST','/assistant/dialogs',{})
        data=json.dumps({'answer':'Открыть задачи','actions':[],'openSections':['tasks','javascript:alert(1)','https://example.com']}).encode()
        urllib.request.urlopen(urllib.request.Request(MODEL+'/next',data=data,headers={'Content-Type':'application/json'})).read()
        status,m=req(self.c,'POST',f'/assistant/dialogs/{d["id"]}/messages',{'text':'Открой задачи'})
        self.assertEqual(status,200,m)
        self.assertEqual(m['actions'],[{'label':'Задачи','href':'tasks.html'}])

    def test_13_current_role_is_checked_again(self):
        url=os.environ.get('ASSISTANT_TEST_DATABASE_URL')
        if not url: self.skipTest('Set ASSISTANT_TEST_DATABASE_URL for role-revocation test')
        p=self.proposal([{'type':'update_settings','changes':{'company':'Не должно измениться'}}])[0]
        uid=self.user['user']['id']
        tool=os.environ.get('PSQL','psql')
        def sql(value): subprocess.run([tool,url,'-v','ON_ERROR_STOP=1','-c',value],check=True,capture_output=True)
        sql(f"UPDATE users SET role='printer' WHERE id={int(uid)}")
        try: self.assertEqual(self.confirm(p)[0],403)
        finally: sql(f"UPDATE users SET role='manager' WHERE id={int(uid)}")

    def test_14_model_context_is_bounded(self):
        for i in range(30):
            a=self.create_action(f'Контекст {i} {time.time_ns()}')
            a['description']='Подробности задачи. '*70
            status,r=req(self.c,'POST','/tasks',a)
            self.assertEqual(status,201,r)
        self.proposal([])
        payload=json.load(urllib.request.urlopen(MODEL+'/last'))
        self.assertGreaterEqual(payload.get('options',{}).get('num_ctx',0),16384)
        self.assertLess(len(payload['prompt']),50000)

    def test_15_graph_cannot_be_split_into_independent_cards(self):
        _,d=req(self.c,'POST','/assistant/dialogs',{})
        model([{'type':'plan_shifts','shifts':[{'employeeId':1,'date':'2027-05-03','time':'09:00-18:00','note':''}]},{'type':'plan_shifts','shifts':[{'employeeId':1,'date':'2027-05-04','time':'09:00-18:00','note':''}]}])
        status,r=req(self.c,'POST',f'/assistant/dialogs/{d["id"]}/messages',{'text':'Составь график'})
        self.assertEqual(status,502,r)
        self.assertEqual(req(self.c,'GET',f'/assistant/dialogs/{d["id"]}')[1]['messages'],[])

    def test_16_delete_invalidated_by_new_comment(self):
        _,r=self.confirm(self.proposal([self.create_action()])[0]); tid=r['result']['id']
        p=self.proposal([{'type':'delete_task','id':tid}])[0]
        self.assertEqual(req(self.c,'POST',f'/tasks/{tid}/comments',{'text':'Важная ручная правка'})[0],201)
        self.assertEqual(self.confirm(p)[0],409)
        self.assertEqual(req(self.c,'GET',f'/tasks/{tid}')[0],200)

    def test_17_reject_restoration_checks_rest_and_rolls_back(self):
        url=os.environ.get('ASSISTANT_TEST_DATABASE_URL')
        if not url: self.skipTest('Set ASSISTANT_TEST_DATABASE_URL for legacy schedule seed')
        _,e=req(self.c,'POST','/employees',{'name':f'Ночная смена {time.time_ns()}','position':'Печатник','department':'Производство'})
        emp=int(e['id'])
        sql=f"INSERT INTO shifts(employee_id,shift_date,start_min,end_min,status,prev_start_min,prev_end_min) VALUES({emp},'2027-04-05',480,960,'ai',1200,480),({emp},'2027-04-06',240,720,'confirmed',NULL,NULL)"
        subprocess.run([os.environ.get('PSQL','psql'),url,'-v','ON_ERROR_STOP=1','-c',sql],check=True,capture_output=True)
        p=self.proposal([{'type':'reject_schedule','weekStart':'2027-04-05'}])[0]
        self.assertEqual(self.confirm(p)[0],409)
        _,week=req(self.c,'GET','/schedule?weekStart=2027-04-05')
        monday=next(s for s in week['shifts'] if s['employeeId']==emp and s['date']=='2027-04-05')
        self.assertEqual(monday['status'],'ai')
        self.assertEqual(monday['time'],'08:00–16:00')

    def test_18_create_text_meeting_processes_without_automatic_tasks(self):
        title=f'Текстовая встреча {time.time_ns()}'
        a={'type':'create_meeting','title':title,'notes':'Обсудили заказ. Требуется проверить печать.','department':'Производство','date':'2027-05-03T10:00:00+03:00'}
        status,r=self.confirm(self.proposal([a])[0]); self.assertEqual(status,200,r)
        mid=r['result']['id']
        for _ in range(50):
            _,meeting=req(self.c,'GET',f'/meetings/{mid}')
            if meeting['status']!='processing': break
            time.sleep(.1)
        self.assertEqual(meeting['status'],'ready',meeting)
        _,tasks=req(self.c,'GET',f'/tasks?meetingId={mid}')
        self.assertEqual(tasks['items'],[])
        url=os.environ.get('ASSISTANT_TEST_DATABASE_URL')
        audio=Path('uploads')/f'meeting-{mid}.assistant-test.wav'
        if url:
            audio.parent.mkdir(exist_ok=True)
            assert not audio.exists()
            audio.write_bytes(b'test recording')
            subprocess.run([os.environ.get('PSQL','psql'),url,'-v','ON_ERROR_STOP=1','-c',f"UPDATE meetings SET audio_file='{audio.name}' WHERE id={int(mid)}"],check=True,capture_output=True)
        try:
            self.assertEqual(self.confirm(self.proposal([{'type':'delete_meeting','id':mid}])[0])[0],200)
            if url: self.assertFalse(audio.exists(),'Deleted meeting audio must be removed')
        finally:
            if url and audio.exists(): audio.unlink()
        self.assertEqual(req(self.c,'GET',f'/meetings/{mid}')[0],404)

    def test_19_empty_proposal_is_repaired_before_showing_confirmation(self):
        _,e=req(self.c,'POST','/employees',{'name':f'План {time.time_ns()}','position':'Печатник','department':'Производство'})
        shifts=[{'employeeId':e['id'],'date':f'2027-06-{day:02d}','time':'09:00-18:00','note':''} for day in range(7,12)]
        first={'answer':'Все смены с 9 до 18. Проверьте предложение и подтвердите.','actions':[]}
        second={'answer':'Добавлю обед с 13 до 14.','actions':[{'type':'plan_shifts','shifts':shifts}]}
        _,d=req(self.c,'POST','/assistant/dialogs',{})
        data=json.dumps({'_sequence':[first,second]}).encode()
        urllib.request.urlopen(urllib.request.Request(MODEL+'/next',data=data,headers={'Content-Type':'application/json'})).read()
        status,m=req(self.c,'POST',f'/assistant/dialogs/{d["id"]}/messages',{'text':'Создай график 7–11 июня 2027 с 9 до 18 без перерывов'})
        self.assertEqual(status,200,m)
        self.assertEqual(len(m['proposals']),1)
        self.assertEqual(len(m['proposals'][0]['details']),5)
        self.assertNotIn('обед',m['text'])
        _,week=req(self.c,'GET','/schedule?weekStart=2027-06-07')
        self.assertFalse(any(s['employeeId']==e['id'] for s in week['shifts']))
        self.assertEqual(self.confirm(m['proposals'][0])[0],200)
        _,week=req(self.c,'GET','/schedule?weekStart=2027-06-07')
        self.assertEqual(sum(s['employeeId']==e['id'] for s in week['shifts']),5)

    def test_20_empty_proposal_never_pretends_confirmation_exists(self):
        _,d=req(self.c,'POST','/assistant/dialogs',{})
        model([], 'Проверьте предложение и подтвердите график.')
        status,r=req(self.c,'POST',f'/assistant/dialogs/{d["id"]}/messages',{'text':'Создай график'})
        self.assertEqual(status,502,r)
        self.assertEqual(req(self.c,'GET',f'/assistant/dialogs/{d["id"]}')[1]['messages'],[])

    def test_21_schedule_request_cannot_become_read_notifications(self):
        _,e=req(self.c,'POST','/employees',{'name':f'График {time.time_ns()}','position':'Печатник','department':'Производство'})
        _,d=req(self.c,'POST','/assistant/dialogs',{})
        wrong={'answer':'Подготовил предложение','actions':[{'type':'read_all_notifications'}]*4}
        correct={'answer':'График готов','actions':[{'type':'plan_shifts','shifts':[{'employeeId':e['id'],'date':'2027-07-05','time':'09:00-18:00','note':''}]}]}
        data=json.dumps({'_sequence':[wrong,correct]}).encode()
        urllib.request.urlopen(urllib.request.Request(MODEL+'/next',data=data,headers={'Content-Type':'application/json'})).read()
        status,m=req(self.c,'POST',f'/assistant/dialogs/{d["id"]}/messages',{'text':'Создай расписание на 5 июля с 9 до 18'})
        self.assertEqual(status,200,m)
        self.assertEqual([p['kind'] for p in m['proposals']],['plan_shifts'])
        payload=json.load(urllib.request.urlopen(MODEL+'/last'))
        self.assertEqual(payload['format']['properties']['actions']['items']['properties']['type']['const'],'plan_shifts')

    def test_22_unrelated_model_actions_are_rejected_after_retry(self):
        _,d=req(self.c,'POST','/assistant/dialogs',{})
        model([{'type':'read_all_notifications'}])
        status,r=req(self.c,'POST',f'/assistant/dialogs/{d["id"]}/messages',{'text':'Составь график с 9 до 18'})
        self.assertEqual(status,502,r)
        self.assertEqual(req(self.c,'GET',f'/assistant/dialogs/{d["id"]}')[1]['messages'],[])

if __name__=='__main__': unittest.main(verbosity=2)
