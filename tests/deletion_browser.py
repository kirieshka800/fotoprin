"""Full real-API browser flows for four deletion categories and cancellation."""
import os,time
from playwright.sync_api import sync_playwright,expect
BASE=os.environ.get('ASSISTANT_TEST_URL','http://127.0.0.1:8081')
with sync_playwright() as p:
 browser=p.chromium.launch(executable_path=os.environ.get('CHROMIUM_PATH','/usr/bin/chromium'),headless=True,args=['--no-sandbox'])
 c=browser.new_context();name=f'Browser delete {time.time_ns()}'
 r=c.request.post(BASE+'/api/auth/register',data={'name':name,'email':f'bd-{time.time_ns()}@test.local','password':'testing-password-123'});assert r.ok,r.text()
 page=c.new_page();errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
 def api(method,path,data=None):
  r=c.request.fetch(BASE+'/api'+path,method=method,data=data);return r
 def confirm():page.locator('[data-confirm-removal]').click()
 def cancel():page.get_by_role('button',name='Отмена',exact=True).click()
 # List deletion: cancelling keeps the task; confirming removes it without opening the row.
 task=api('POST','/tasks',{'title':f'Browser task {time.time_ns()}','due':'2027-12-01T15:00:00+03:00'}).json();tid=task['id']
 page.goto(BASE+'/tasks.html?sort=title')
 # Ensure fixture is on the first page independently of accumulated test records.
 page.route('**/api/tasks?*',lambda route:route.fulfill(json={'items':[task] if api('GET',f'/tasks/{tid}').status==200 else [],'total':1 if api('GET',f'/tasks/{tid}').status==200 else 0,'counts':{'active':1,'all':1,'done':0}}))
 page.reload();button=page.locator(f'[data-delete-task="{tid}"]');expect(button).to_be_visible();button.click();cancel();assert api('GET',f'/tasks/{tid}').status==200
 button.click();confirm();expect(page.locator(f'[data-delete-task="{tid}"]')).to_have_count(0);assert api('GET',f'/tasks/{tid}').status==404
 page.unroute('**/api/tasks?*')
 # Detail deletion.
 t=api('POST','/tasks',{'title':'Browser detail delete','due':'2027-12-01T15:00:00+03:00'}).json()
 page.goto(BASE+f'/task.html?id={t["id"]}');page.locator('#delete-task').click();confirm();page.wait_for_url('**/tasks.html');assert api('GET',f'/tasks/{t["id"]}').status==404
 # Summary deletion preserves meeting; full deletion is separate.
 r=c.request.post(BASE+'/api/meetings',multipart={'title':f'Browser summary {time.time_ns()}','notes':'Решили проверить печать','actions':'{}','participantIds':'[]'});assert r.ok,r.text();mid=r.json()['id']
 for _ in range(100):
  m=api('GET',f'/meetings/{mid}').json()
  if m['status']!='processing':break
  time.sleep(.1)
 assert m['status']=='ready',m
 page.goto(BASE+f'/meeting.html?id={mid}');page.locator('#delete-summary').click();cancel();assert api('GET',f'/meetings/{mid}').json()['summary']
 page.locator('#delete-summary').click();confirm();page.get_by_text('Саммари отсутствует',exact=True).wait_for();assert api('GET',f'/meetings/{mid}').status==200
 page.locator('#delete-meeting').click();confirm();page.wait_for_url('**/meetings.html');assert api('GET',f'/meetings/{mid}').status==404
 # Dialog deletion and clear-all, no old conversations restored.
 page.goto(BASE+'/assistant.html');page.wait_for_function('current !== null')
 did=page.evaluate('current.id');page.locator(f'[data-delete-dialog="{did}"]').click();cancel();assert api('GET',f'/assistant/dialogs/{did}').status==200
 page.locator(f'[data-delete-dialog="{did}"]').click();confirm();page.get_by_text('Здесь пока нет диалогов',exact=True).wait_for();assert api('GET',f'/assistant/dialogs/{did}').status==404
 page.locator('#new-dialog').click();page.wait_for_function('current !== null');page.locator('#clear-dialogs').click();confirm();page.get_by_text('Здесь пока нет диалогов',exact=True).wait_for();assert api('GET','/assistant/dialogs').json()==[]
 # Generate a notice for this user from another account.
 other=browser.new_context();r=other.request.post(BASE+'/api/auth/register',data={'name':f'Other {time.time_ns()}','email':f'other-bd-{time.time_ns()}@test.local','password':'testing-password-123'});assert r.ok
 emp=next(e for e in api('GET','/employees').json() if e['name']==name)
 def seed_notice():
  r=other.request.post(BASE+'/api/tasks',data={'title':f'Notice {time.time_ns()}','assigneeId':emp['id'],'due':'2027-12-01T15:00:00+03:00'});assert r.ok,r.text()
 seed_notice();page.goto(BASE+'/notifications.html');nid=api('GET','/notifications').json()['items'][0]['id']
 page.locator(f'[data-delete-notice="{nid}"]').click();cancel();assert api('GET','/notifications').json()['counts']['all']>0
 page.locator(f'[data-delete-notice="{nid}"]').click();confirm();expect(page.locator(f'[data-delete-notice="{nid}"]')).to_have_count(0);assert page.url.endswith('/notifications.html')
 seed_notice();page.reload();page.locator('#delete-all-notifications').click();confirm();page.get_by_text('Здесь пока пусто',exact=True).wait_for();assert api('GET','/notifications').json()['counts']['all']==0
 assert not errors,errors
 browser.close();print('PASS: task list/detail, summary vs meeting, dialog and notification deletion, clear-all, cancellation, confirmations and no JS errors')
