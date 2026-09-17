#!/usr/bin/env python3
"""Connect mention regression using the synthetic group-work fixture.

Start an isolated Vite server with HMR disabled and proxy={} on port 15584,
then run this script. Override CCCC_CONNECT_MENTION_BASE_URL for another port.
All HTTP/WebSocket transports are fixture-owned; no live Group or Actor is used.
"""
import base64, json, os, subprocess, tempfile, time
from pathlib import Path
import requests, websocket
out = Path(os.environ.get('CCCC_CONNECT_MENTION_OUT', '/tmp/cccc-connect-mentions-browser'))
out.mkdir(parents=True, exist_ok=True)
base = os.environ.get('CCCC_CONNECT_MENTION_BASE_URL', 'http://127.0.0.1:15584').rstrip('/')
with tempfile.TemporaryDirectory(prefix='cccc-mention-chrome-', ignore_cleanup_errors=True) as profile:
    browser = subprocess.Popen(['/usr/bin/google-chrome', '--headless=new', '--no-sandbox', '--remote-debugging-port=0', '--remote-allow-origins=*', '--user-data-dir=' + profile, 'about:blank'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    sock = None
    try:
        for _ in range(100):
            portfile = Path(profile) / 'DevToolsActivePort'
            if portfile.exists():
                break
            time.sleep(0.1)
        port = portfile.read_text().splitlines()[0]
        tab = requests.put(f'http://127.0.0.1:{port}/json/new?about:blank').json()
        sock = websocket.create_connection(tab['webSocketDebuggerUrl'], timeout=20)
        seq = 0

        def cdp(method, params=None):
            global seq
            seq += 1
            sock.send(json.dumps({'id': seq, 'method': method, 'params': params or {}}))
            while True:
                message = json.loads(sock.recv())
                if message.get('id') == seq:
                    if 'error' in message:
                        raise RuntimeError(message['error'])
                    return message['result']

        def js(expression):
            result = cdp('Runtime.evaluate', {'expression': expression, 'returnByValue': True, 'awaitPromise': True})
            if 'exceptionDetails' in result:
                raise RuntimeError(result['exceptionDetails'])
            return result.get('result', {}).get('value')

        def wait(expression):
            for _ in range(100):
                if js(expression):
                    return
                time.sleep(0.1)
            raise AssertionError(expression + ' ' + str(js('document.body.innerText.slice(-3000)')))

        def key(k, modifiers=0):
            codes = {'Enter': 13, 'Escape': 27, 'ArrowDown': 40, 'Tab': 9, 'Backspace': 8, 'a': 65}
            for t in ['keyDown', 'keyUp']:
                cdp('Input.dispatchKeyEvent', {'type': t, 'key': k, 'code': k, 'windowsVirtualKeyCode': codes.get(k, 0), 'modifiers': modifiers})
            time.sleep(0.04)

        def input_text(text):
            js("document.querySelector('textarea').focus()")
            key('a', 2)
            cdp('Input.insertText', {'text': text})
            time.sleep(0.15)

        def select_mac():
            wait("[...document.querySelectorAll('[role=option]')].some(e=>e.textContent.includes('Mac Studio'))")
            idx = js("[...document.querySelectorAll('[role=option]')].findIndex(e=>e.textContent.includes('Mac Studio'))")
            for _ in range(idx):
                key('ArrowDown')
            key('Enter')
            wait("document.querySelector('textarea').value.includes('#Shared Team · Mac Studio')")

        def shot(name):
            out.joinpath(name + '.png').write_bytes(base64.b64decode(cdp('Page.captureScreenshot', {'format': 'png'})['data']))
        cdp('Emulation.setDeviceMetricsOverride', {'width': 1280, 'height': 900, 'deviceScaleFactor': 1, 'mobile': False})
        cdp('Page.navigate', {'url': base + '/ui/tests/browser/group-work.html'})
        wait("!!window.groupWorkProbe && !!document.querySelector('textarea')")
        assert js("groupWorkProbe.requests.filter(r=>r.path.endsWith('/connect/catalog')).length") == 0
        input_text('#')
        wait("document.querySelectorAll('[role=option]').length===4")
        assert js("document.querySelector('[role=listbox]').textContent.includes('may be outdated')")
        shot('desktop')
        select_mac()
        assert js('groupWorkProbe.composer.getState().destGroupId') == 'g1'
        cdp('Input.insertText', {'text': '@'})
        wait("[...document.querySelectorAll('[role=option]')].some(e=>e.textContent.includes('Remote worker'))")
        key('Enter')
        assert js('groupWorkProbe.composer.getState().toText') == ''
        cdp('Input.insertText', {'text': 'please coordinate'})
        key('Enter', 2)
        wait("groupWorkProbe.requests.some(r=>r.method==='POST' && r.path.endsWith('/send'))")
        sent = js("groupWorkProbe.requests.filter(r=>r.method==='POST' && r.path.endsWith('/send')).at(-1)")
        assert sent['path'] == '/api/v1/groups/g1/send', sent
        assert sent['body']['refs'][0]['instance_id'] == 'i_mac', sent
        assert 'dst_instance_id' not in sent['body'] and 'dst_group_id' not in sent['body'], sent
        wait("document.querySelector('textarea').value===''")
        input_text('#')
        select_mac()
        cdp('Input.insertText', {'text': '@'})
        wait("[...document.querySelectorAll('[role=option]')].some(e=>e.textContent.includes('Remote worker'))")
        key('Enter')
        js("groupWorkProbe.chooseGroup('g2')")
        wait("groupWorkProbe.composer.getState().activeGroupId==='g2'")
        assert js('groupWorkProbe.composer.getState().composerGroupMentionTokens.length') == 0
        # Deliver the late transcript through the real Voice draft writer, without a provider.
        assert js("import('/ui/src/pages/chat/voice-secretary/voiceComposerDraftRouting.ts').then(m=>m.routeVoiceTextToComposerGroup({groupId:'g1',text:'voice update',mode:'append'}))") == 'draft'
        assert js('groupWorkProbe.composer.getState().composerText') == ''
        js("groupWorkProbe.chooseGroup('g1')")
        wait("document.querySelector('textarea').value.endsWith('voice update')")
        assert js('groupWorkProbe.composer.getState().composerGroupMentionTokens[0].remote.instance_id') == 'i_mac'
        assert js('groupWorkProbe.composer.getState().composerAgentMentionTokens.length') == 1
        key('Enter', 2)
        wait("groupWorkProbe.requests.filter(r=>r.path.endsWith('/send')).length===2")
        voice_sent = js("groupWorkProbe.requests.filter(r=>r.path.endsWith('/send')).at(-1)")
        assert voice_sent['path'] == '/api/v1/groups/g1/send', voice_sent
        assert voice_sent['body']['refs'] == sent['body']['refs'], voice_sent
        assert voice_sent['body']['text'].endswith('voice update'), voice_sent
        wait("document.querySelector('textarea').value===''")
        input_text('#')
        select_mac()
        input_text('plain edited message')
        key('Enter', 2)
        wait("groupWorkProbe.requests.filter(r=>r.path.endsWith('/send')).length===3")
        assert js("groupWorkProbe.requests.filter(r=>r.path.endsWith('/send')).at(-1).body.refs.length") == 0
        js('groupWorkProbe.setCatalogDelay(500)')
        input_text('#')
        js("groupWorkProbe.chooseGroup('g2')")
        time.sleep(1.2)
        assert js('groupWorkProbe.composer.getState().activeGroupId') == 'g2'
        assert not js('groupWorkProbe.composer.getState().composerGroupMentionTokens.length')
        js('groupWorkProbe.setCatalogDelay(0);groupWorkProbe.setCatalogRestricted(true)')
        key('Escape')
        input_text('#')
        wait("document.querySelectorAll('[role=option]').length===2")
        assert not js("document.querySelector('[role=listbox]').textContent.includes('Shared Team')")
        js('groupWorkProbe.setCatalogRestricted(false);groupWorkProbe.setDark(true)')
        key('Escape')
        input_text('')
        cdp('Emulation.setDeviceMetricsOverride', {'width': 390, 'height': 844, 'deviceScaleFactor': 1, 'mobile': True})
        js("groupWorkProbe.language('ja')")
        input_text('#')
        wait("document.querySelectorAll('[role=option]').length===4")
        box = js("(()=>{const r=document.querySelector('[role=listbox]').getBoundingClientRect();return {left:r.left,right:r.right,top:r.top,bottom:r.bottom,width:innerWidth,height:innerHeight}})()")
        assert box['left'] >= 0 and box['right'] <= box['width'] and (box['top'] >= 0) and (box['bottom'] <= box['height']), box
        shot('mobile-dark')
        point = js("(()=>{const e=[...document.querySelectorAll('[role=option]')].find(e=>e.textContent.includes('Direct workstation'));const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()")
        cdp('Input.dispatchTouchEvent', {'type': 'touchStart', 'touchPoints': [point]})
        cdp('Input.dispatchTouchEvent', {'type': 'touchEnd', 'touchPoints': []})
        wait("groupWorkProbe.composer.getState().composerGroupMentionTokens.some(t=>t.remote?.instance_id==='i_direct')")
        assert js('groupWorkProbe.errors') == [], js('groupWorkProbe.errors')
        out.joinpath('proof.json').write_text(json.dumps({'sent': sent, 'voice_sent': voice_sent, 'mobile': box, 'errors': js('groupWorkProbe.errors')}, ensure_ascii=False, indent=2))
        print('PASS keyboard, qualified references, local send, remote Actors, per-Group drafts, late Voice updates, edits, late reads, restricted view, mobile overflow')
    except Exception:
        try:
            shot('failure')
            print(js('groupWorkProbe.errors'))
        except Exception:
            pass
        raise
    finally:
        if sock:
            sock.close()
        browser.terminate()
        browser.wait(timeout=10)
