"""Design integration against the real Rust app (test database only).

Run: python tests/frontend_browser.py. Requires Python Playwright and Chromium.
Existing assistant/audio/deletion browser suites cover preserved product flows.
"""
import os
import time
from playwright.sync_api import sync_playwright, expect

BASE = os.environ.get('ASSISTANT_TEST_URL', 'http://127.0.0.1:8081')


def run():
    with sync_playwright() as p:
        browser = p.chromium.launch(executable_path=os.environ.get('CHROMIUM_PATH', '/usr/bin/chromium'),
                                    headless=True, args=['--no-sandbox'])
        c = browser.new_context(viewport={'width': 1440, 'height': 1080}, reduced_motion='reduce')
        name = f'Front test {time.time_ns()}'
        r = c.request.post(BASE+'/api/auth/register', data={
            'name': name, 'email': f'front-{time.time_ns()}@test.local', 'password': 'testing-password-123'})
        assert r.ok, r.text()
        page = c.new_page()
        errors = []
        page.on('pageerror', lambda e: errors.append(str(e)))

        def api(method, path, data=None):
            r = c.request.fetch(BASE+'/api'+path, method=method, data=data)
            assert r.ok, r.text()
            return r.json()

        def trigger(selector):
            return page.locator(selector).locator('..').locator('.custom-select-trigger')

        def pick(selector, text):
            trigger(selector).click()
            page.locator('.custom-select-menu:not([hidden])').get_by_role('option', name=text, exact=True).click()

        page.goto(BASE+'/overview.html')
        expect(page.locator('#theme-checkbox')).to_have_count(1)
        page.locator('label[for=theme-checkbox]').click()
        expect(page.locator('html')).to_have_attribute('data-theme', 'dark')
        page.reload()
        expect(page.locator('html')).to_have_attribute('data-theme', 'dark')
        second = c.new_page()
        second.goto(BASE+'/settings.html')
        expect(second.locator('html')).to_have_attribute('data-theme', 'dark')
        second.locator('label[for=theme-checkbox]').click()
        expect(page.locator('html')).not_to_have_attribute('data-theme', 'dark')
        second.close()

        # Async native option lists receive the same visual control as static ones.
        employees = api('GET', '/employees')
        me = next(e for e in employees if e['name'] == name)
        task = api('POST', '/tasks', {'title': name, 'assigneeId': me['id'],
                                     'due': '2027-12-01T15:00:00+03:00', 'priority': 'Высокий'})
        task = api('PATCH', f'/tasks/{task["id"]}', {
            'checklist': [{'text': 'Проверить макет', 'done': False}]})
        page.goto(BASE+'/tasks.html')
        expect(trigger('#sort')).to_contain_text('По сроку')
        pick('#assignee', name)
        page.wait_for_function('(id) => state.assigneeId === String(id)', arg=me['id'])
        expect(page.locator('#table-box')).to_contain_text(name)
        trigger('#sort').focus()
        page.keyboard.press('ArrowDown')
        page.keyboard.press('End')
        page.keyboard.press('Enter')
        page.wait_for_function("state.sort === 'status'")
        expect(trigger('#sort')).to_contain_text('По статусу')
        trigger('#sort').click()
        page.keyboard.press('Escape')
        expect(page.locator('.custom-select-menu:not([hidden])')).to_have_count(0)
        # New checkbox markup keeps label clicks away from row navigation.
        row = page.locator(f'tr[data-id="{task["id"]}"]')
        row.locator('label.cl-checkbox > span').click()
        page.wait_for_function("(id) => !document.querySelector('tr[data-id=\"'+id+'\"]')", arg=str(task['id']))
        assert page.url.endswith('/tasks.html')
        assert api('GET', f'/tasks/{task["id"]}')['status'] == 'done'

        page.locator('#new-task').click()
        expect(page.locator('.modal-back .custom-select-trigger')).to_have_count(2)
        page.locator('.modal-back [name=title]').fill('Несохранённый черновик')
        pick('.modal-back [name=priority]', 'Срочный')
        trigger('.modal-back [name=assignee]').click()
        page.keyboard.press('Escape')
        expect(page.locator('.modal-back')).to_have_count(1)
        expect(page.locator('.custom-select-menu:not([hidden])')).to_have_count(0)
        expect(page.locator('.modal-back [name=title]')).to_have_value('Несохранённый черновик')
        trigger('.modal-back [name=assignee]').focus()
        page.keyboard.press('ArrowDown')
        page.keyboard.press('Escape')
        expect(page.locator('.modal-back')).to_have_count(1)
        trigger('.modal-back [name=assignee]').click()
        page.get_by_role('button', name='Отмена', exact=True).click(force=True)
        expect(page.locator('.modal-back')).to_have_count(0)
        expect(page.locator('.custom-select-menu:not([hidden])')).to_have_count(0)
        assert page.evaluate("Array.from(document.querySelectorAll('.custom-select-menu')).every(m => m._select && m._select.isConnected)")

        page.goto(BASE+f'/task.html?id={task["id"]}')
        expect(trigger('#status')).to_contain_text('Завершено')
        # Programmatic changes (.value after response) refresh visible labels.
        page.locator('#complete').click()
        expect(trigger('#status')).to_contain_text('В работе')
        page.locator('#checklist label.cl-checkbox > span').click()
        expect(page.locator('#checklist input')).to_be_checked()
        page.locator('#edit').click()
        pick('.modal-back [name=priority]', 'Срочный')
        page.locator('#task-save').click()
        expect(page.locator('.modal-back')).to_have_count(0)
        expect(trigger('#priority')).to_contain_text('Срочный')
        assert api('GET', f'/tasks/{task["id"]}')['priority'] == 'Срочный'

        page.goto(BASE+'/meetings.html')
        pick('#period', 'За всё время')
        page.evaluate('resetFilters()')
        expect(trigger('#period')).to_contain_text('За неделю')
        page.goto(BASE+'/new-meeting.html')
        expect(page.locator('#act-tasks')).to_be_checked()
        page.locator('label:has(#act-tasks) > span').click()
        expect(page.locator('#act-tasks')).not_to_be_checked()
        page.set_viewport_size({'width': 390, 'height': 844})
        page.locator('#people-btn').click()
        expect(page.locator('#people-menu')).to_be_visible()
        page.locator('#people-menu').scroll_into_view_if_needed()
        page.wait_for_function("() => {const menu=document.querySelector('#people-menu'),r=menu.getBoundingClientRect();return menu.contains(document.elementFromPoint(r.left+40,Math.min(r.bottom-10,innerHeight-5)));}", timeout=5000)
        page.locator('#people-btn').click()
        page.set_viewport_size({'width': 1440, 'height': 1080})
        # Real API still accepts all supported voice formats in the frontend.
        assert page.evaluate("validateMeetingFile({name:'voice.ogg',size:1000})") == ''

        # Search result menus remain outside card clipping and are actionable.
        page.locator('#global-search input').fill(name)
        result = page.locator('#global-search .search-results button').filter(has_text=name).first
        expect(result).to_be_visible()
        result.click()
        page.wait_for_url('**/task.html?id=*')
        expect(page.locator('#logout')).to_be_visible()

        for theme in ['dark', 'light']:
            page.evaluate('(theme) => { applyTheme(theme); localStorage.setItem("fp_theme",theme); }', theme)
            for url in ['overview.html', 'assistant.html', 'meetings.html', 'tasks.html',
                        'schedule.html', 'notifications.html', 'settings.html', 'new-meeting.html',
                        f'task.html?id={task["id"]}', 'login.html', 'register.html']:
                page.goto(BASE+'/'+url)
                expect(page.locator('h1,h2').first).to_be_visible()
                assert page.evaluate('currentTheme()') == theme, (url, theme)
                page.set_viewport_size({'width': 390, 'height': 844})
                assert page.evaluate('document.documentElement.scrollWidth <= window.innerWidth'), (url, theme, 'overflow')
                page.set_viewport_size({'width': 1440, 'height': 1080})
        page.goto(BASE+'/tasks.html')
        page.locator('#logout').click()
        page.wait_for_url('**/login.html')
        assert c.request.get(BASE+'/api/me').status == 401
        assert not errors, errors
        browser.close()
        print('PASS: theme persistence/cross-tab, async/keyboard selects, Escape preserves draft, modal cleanup, mobile participant menu, checkbox/save/reset, search, logout, 11 pages × 2 themes × desktop/mobile; no JS errors')


if __name__ == '__main__':
    run()
