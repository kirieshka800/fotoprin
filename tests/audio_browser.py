"""Browser regression: actual 3-second media beats stale 60-second metadata."""
import io, wave, math, struct, time, os
from playwright.sync_api import sync_playwright
BASE=os.environ.get('ASSISTANT_TEST_URL','http://127.0.0.1:8081')
def wav(seconds=3):
    b=io.BytesIO()
    with wave.open(b,'wb') as w:
        w.setparams((1,2,16000,0,'NONE','not compressed'))
        w.writeframes(b''.join(struct.pack('<h',int(1000*math.sin(2*math.pi*220*i/16000))) for i in range(int(16000*seconds))))
    return b.getvalue()
with sync_playwright() as p:
    browser=p.chromium.launch(executable_path=os.environ.get('CHROMIUM_PATH','/usr/bin/chromium'),headless=True,args=['--no-sandbox','--autoplay-policy=no-user-gesture-required'])
    c=browser.new_context()
    r=c.request.post(BASE+'/api/auth/register',data={'name':'Аудио тест','email':f'audio-{time.time_ns()}@test.local','password':'testing-password-123'})
    assert r.ok,r.text()
    page=c.new_page();errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
    fixture={'id':987654,'title':'3 секунды','date':'2026-11-12T12:00:00Z','durationMin':1,'participantIds':[],'status':'ready','audioUrl':'/test-audio.wav','audioDuration':60,'summary':{'highlight':'Тест','topics':[],'decisions':[],'source':''},'transcript':[],'tasks':[],'errorMessage':'Whisper не настроен'}
    page.route('**/api/meetings/987654',lambda route:route.fulfill(json=fixture))
    page.route('**/test-audio.wav',lambda route:route.fulfill(body=wav(),content_type='audio/wav'))
    page.goto(BASE+'/meeting.html?id=987654')
    page.wait_for_function("document.querySelector('#time').textContent === '00:00 / 00:03'",timeout=6000)
    page.locator('[data-tab=transcript]').click()
    assert page.get_by_role('button',name='Распознать аудио',exact=True).is_visible()
    assert page.get_by_text('Whisper не настроен',exact=False).count()>0
    page.locator('#play').click()
    page.wait_for_function("document.querySelector('#audio').ended",timeout=6000)
    assert page.locator('#play').get_attribute('aria-label')=='Воспроизвести'
    assert page.locator('#time').inner_text()=='00:03 / 00:03'
    # Repeat and seek: time is clamped and playback is based on the media element.
    page.locator('#play').click();page.wait_for_function("!document.querySelector('#audio').paused")
    page.evaluate('seekTo(999)');page.wait_for_timeout(300)
    assert page.evaluate("document.querySelector('#audio').currentTime <= 3.01")
    page.unroute('**/test-audio.wav')
    page.route('**/test-audio.wav',lambda route:route.fulfill(body=wav(3.96),content_type='audio/wav'))
    page.reload()
    page.wait_for_function("document.querySelector('#time').textContent === '00:00 / 00:03'")
    page.locator('#play').click()
    page.wait_for_function("document.querySelector('#audio').ended",timeout=6500)
    assert page.locator('#time').inner_text()=='00:03 / 00:03'
    fixture['status']='error';fixture['errorMessage']='Whisper временно недоступен'
    page.reload()
    page.wait_for_selector('#state-box audio[controls]')
    assert page.get_by_role('button',name='Распознать аудио',exact=True).is_visible()
    assert page.locator('#state-box audio').get_attribute('src')=='/test-audio.wav'
    assert not errors,errors
    browser.close()
    print('PASS: actual 3-second duration, ended/replay state, clamped seek, transcription warning and retry control, no JS errors')
