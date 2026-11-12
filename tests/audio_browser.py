"""Browser regression: actual 3-second media beats stale 60-second metadata."""
import io, wave, math, struct, time, os, re
from playwright.sync_api import sync_playwright
BASE=os.environ.get('ASSISTANT_TEST_URL','http://127.0.0.1:8081')
def wav(seconds=3):
    b=io.BytesIO()
    with wave.open(b,'wb') as w:
        w.setparams((1,2,16000,0,'NONE','not compressed'))
        w.writeframes(b''.join(struct.pack('<h',int(1000*math.sin(2*math.pi*220*i/16000))) for i in range(int(16000*seconds))))
    return b.getvalue()
def audio_response(route, seconds=3):
    """Serve byte ranges like the Rust static-file service, so seeking is real."""
    body=wav(seconds)
    headers={'Accept-Ranges':'bytes','Content-Type':'audio/wav'}
    match=re.fullmatch(r'bytes=(\d+)-(\d*)',route.request.headers.get('range',''))
    if match:
        start=int(match[1]);end=min(int(match[2]) if match[2] else len(body)-1,len(body)-1)
        headers['Content-Range']=f'bytes {start}-{end}/{len(body)}'
        route.fulfill(status=206,body=body[start:end+1],headers=headers)
    else:
        route.fulfill(body=body,headers=headers)
with sync_playwright() as p:
    browser=p.chromium.launch(executable_path=os.environ.get('CHROMIUM_PATH','/usr/bin/chromium'),headless=True,args=['--no-sandbox','--autoplay-policy=no-user-gesture-required'])
    c=browser.new_context()
    r=c.request.post(BASE+'/api/auth/register',data={'name':'Аудио тест','email':f'audio-{time.time_ns()}@test.local','password':'testing-password-123'})
    assert r.ok,r.text()
    page=c.new_page();errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
    fixture={'id':987654,'title':'3 секунды','date':'2026-11-12T12:00:00Z','durationMin':1,'participantIds':[],'status':'ready','audioUrl':'/test-audio.wav','audioDuration':60,'summary':{'highlight':'Тест','topics':[],'decisions':[],'source':''},'transcript':[],'tasks':[],'errorMessage':'Whisper не настроен'}
    page.route('**/api/meetings/987654',lambda route:route.fulfill(json=fixture))
    page.route('**/test-audio.wav',lambda route:audio_response(route))
    page.goto(BASE+'/meeting.html?id=987654')
    page.wait_for_function("document.querySelector('#time').textContent === '00:00 / 00:03'",timeout=6000)
    assert page.evaluate("document.querySelector('#audio').volume") == 0.8
    page.locator('#mute').click()
    assert page.evaluate("document.querySelector('#audio').muted")
    assert page.locator('#mute').get_attribute('aria-label') == 'Включить звук'
    page.locator('#volume').evaluate("el => {el.value='0.25';el.dispatchEvent(new Event('input',{bubbles:true}));}")
    assert page.evaluate("document.querySelector('#audio').volume === 0.25 && !document.querySelector('#audio').muted")
    page.locator('#forward-10').click()
    page.wait_for_function("document.querySelector('#audio').currentTime >= 2.9")
    assert page.evaluate("document.querySelector('#audio').currentTime <= 3.01")
    page.locator('#back-10').click()
    page.wait_for_function("document.querySelector('#audio').currentTime === 0")
    page.locator('[data-tab=transcript]').click()
    assert page.get_by_role('button',name='Распознать аудио',exact=True).is_visible()
    assert page.get_by_text('Whisper не настроен',exact=False).count()>0
    page.locator('#play').click()
    page.wait_for_function("document.querySelector('#audio').ended",timeout=6000)
    assert page.locator('#play').get_attribute('aria-label')=='Воспроизвести'
    assert 'playing' not in (page.locator('#wave').get_attribute('class') or '').split()
    assert page.locator('#time').inner_text()=='00:03 / 00:03'
    # Repeat and seek: time is clamped and playback is based on the media element.
    page.locator('#play').click();page.wait_for_function("!document.querySelector('#audio').paused")
    page.evaluate('seekTo(999)');page.wait_for_timeout(300)
    assert page.evaluate("document.querySelector('#audio').currentTime <= 3.01")
    page.unroute('**/test-audio.wav')
    page.route('**/test-audio.wav',lambda route:audio_response(route,3.96))
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
    fixture['status']='ready';fixture['audioUrl']=None
    page.reload()
    page.wait_for_selector('#content:not(.hidden)')
    for selector in ['#play','#speed','#back-10','#forward-10','#mute','#volume']:
        assert page.locator(selector).is_disabled(),selector
    assert not errors,errors
    browser.close()
    print('PASS: actual 3-second duration, mute/volume/skip, ended/replay state, clamped seek, transcription warning/retry, no-audio controls disabled; no JS errors')
