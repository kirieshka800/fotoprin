"""Browser tests against the real app + fake Ollama; requires Python Playwright."""
import json
import os
import time
import urllib.request
from playwright.sync_api import sync_playwright
BASE=os.environ.get('ASSISTANT_TEST_URL','http://127.0.0.1:8081')
MODEL=os.environ.get('ASSISTANT_FAKE_URL','http://127.0.0.1:11435')
def model(actions,answer='Предлагаю создать задачу. Проверьте параметры.'):
    data=json.dumps({'answer':answer,'actions':actions}).encode()
    urllib.request.urlopen(urllib.request.Request(MODEL+'/next',data=data,headers={'Content-Type':'application/json'})).read()

def run():
    with sync_playwright() as p:
        browser=p.chromium.launch(executable_path=os.environ.get('CHROMIUM_PATH','/usr/bin/chromium'),headless=True,args=['--no-sandbox'])
        context=browser.new_context(viewport={'width':1440,'height':1000})
        response=context.request.post(BASE+'/api/auth/register',data={'name':'Браузерный тест','email':f'browser-{time.time_ns()}@test.local','password':'testing-password-123'})
        assert response.ok, response.text()
        page=context.new_page()
        errors=[]
        page.on('pageerror',lambda e:errors.append(str(e)))
        response=page.goto(BASE+'/assistant.html')
        assert response.headers.get('cache-control')=='no-store', 'HTML must not use old cached UI'
        assert context.request.get(BASE+'/assistant.js').headers.get('cache-control')=='no-store'
        page.locator('#input').wait_for()
        page.wait_for_function('current !== null')
        title=f'Задача браузера {time.time_ns()}'
        action={'type':'create_task','title':title,'description':'Проверка <script>alert(1)</script>','assigneeId':None,'due':'2027-12-01T15:00:00+03:00','priority':'Высокий'}
        model([action])
        page.locator('#input').fill('Создай задачу для теста')
        page.locator('#send').click()
        page.get_by_role('button',name='Подтвердить',exact=True).wait_for(timeout=8000)
        assert page.get_by_text(title,exact=True).is_visible()
        assert page.get_by_text('Проверка <script>alert(1)</script>',exact=True).is_visible()
        page.get_by_role('button',name='Подтвердить',exact=True).click()
        page.get_by_text('Задача создана',exact=True).wait_for()
        assert page.get_by_role('link',name='Открыть результат',exact=True).is_visible()
        page.reload()
        page.get_by_text('Задача создана',exact=True).wait_for()
        assert page.get_by_role('button',name='Подтвердить',exact=True).count()==0
        # Cancellation survives reload and never exposes an apply button.
        action['title']='Отменённая '+title
        model([action])
        page.locator('#input').fill('Подготовь ещё одну задачу')
        page.locator('#send').click()
        page.get_by_role('button',name='Отклонить',exact=True).wait_for()
        page.get_by_role('button',name='Отклонить',exact=True).click()
        page.get_by_text('Предложение отклонено',exact=True).wait_for()
        page.reload()
        page.get_by_text('Предложение отклонено',exact=True).wait_for()
        assert not errors, errors
        screenshot=os.environ.get('ASSISTANT_SCREENSHOT','/workspace/work/assistant-browser.png')
        page.screenshot(path=screenshot,full_page=True)
        # Small viewport: controls stay available without horizontal page overflow.
        page.set_viewport_size({'width':390,'height':844})
        page.reload()
        page.wait_for_function('current !== null')
        assert page.evaluate('document.documentElement.scrollWidth <= window.innerWidth'), 'horizontal overflow'
        assert page.locator('#input').is_visible()
        page.goto(BASE+'/schedule.html?weekStart=2027-02-01')
        page.wait_for_function("weekStart === '2027-02-01'", timeout=3000)
        browser.close()
        print('PASS: preview, escaped parameters, confirmation, result link, rejection, reload, mobile layout; no JS errors')
if __name__=='__main__':run()
