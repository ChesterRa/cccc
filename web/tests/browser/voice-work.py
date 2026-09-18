#!/usr/bin/env python3
"""Real voice workspaces, synthetic APIs. Use the isolated Vite server from group-work.py.
Set CCCC_GROUP_WORK_BASE_URL and optionally CCCC_VOICE_WORK_OUTPUT_DIR.
Requires Chrome, requests and websocket-client; never attaches to user browsers.
"""
import base64,json,os,subprocess,tempfile,time
from pathlib import Path
import requests,websocket
phase='verified'
out=Path(os.environ.get('CCCC_VOICE_WORK_OUTPUT_DIR','/tmp/cccc-voice-work'));out.mkdir(parents=True,exist_ok=True)
base=os.environ.get('CCCC_GROUP_WORK_BASE_URL','http://127.0.0.1:15559').rstrip('/')
with tempfile.TemporaryDirectory(prefix='cccc-voice-work-browser-') as profile:
 p=subprocess.Popen([os.environ.get('CHROME_BIN','/usr/bin/google-chrome'),'--headless=new','--no-sandbox','--remote-debugging-port=0','--remote-allow-origins=*','--user-data-dir='+profile,'about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 try:
  portfile=Path(profile)/'DevToolsActivePort'
  for _ in range(100):
   if portfile.exists():break
   time.sleep(.1)
  port=portfile.read_text().splitlines()[0]
  tab=requests.put(f'http://127.0.0.1:{port}/json/new?about:blank',timeout=5).json()
  sock=websocket.create_connection(tab['webSocketDebuggerUrl'],timeout=25);seq=0
  def cdp(m,params={}):
   global seq
   seq+=1;sock.send(json.dumps({'id':seq,'method':m,'params':params}))
   while True:
    r=json.loads(sock.recv())
    if r.get('id')==seq:
     if 'error' in r:raise Exception(r)
     return r['result']
  def js(code):
   r=cdp('Runtime.evaluate',{'expression':code,'returnByValue':True,'awaitPromise':True})
   if 'exceptionDetails' in r:raise Exception(r['exceptionDetails'])
   return r.get('result',{}).get('value')
  def wait(code):
   for _ in range(100):
    if js(code):return
    time.sleep(.1)
   raise Exception('timeout '+code+' '+str(js('document.body.innerText.slice(0,2000)')))
  def shot(name):
   out.joinpath(phase+'-'+name+'.png').write_bytes(base64.b64decode(cdp('Page.captureScreenshot',{'format':'png'})['data']))
  def press(key):
   for kind in ['keyDown','keyUp']:
    cdp('Input.dispatchKeyEvent',{'type':kind,'key':key,'code':key,'windowsVirtualKeyCode':{'Enter':13,'Tab':9,'Escape':27}[key],**({'text':'\r'} if key=='Enter' and kind=='keyDown' else {})})
   time.sleep(.1)
  def value(selector,text):
   js("(()=>{const e=document.querySelector("+json.dumps(selector)+");Object.getOwnPropertyDescriptor(e.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype,'value').set.call(e,"+json.dumps(text)+");e.dispatchEvent(new Event('input',{bubbles:true}));})()")
   time.sleep(.15)
  def click(selector):
   js("document.querySelector("+json.dumps(selector)+").click()")
   time.sleep(.15)
  results=[]
  # Regressions: explicit artifact navigation is independent from capture mode.
  for width,height,lang,scale in [(1440,900,'en',100),(320,568,'ja',125),(390,667,'en',100)]:
   for mode in ['instruction','prompt']:
    cdp('Emulation.setDeviceMetricsOverride',{'width':width,'height':height,'deviceScaleFactor':1,'mobile':width<640})
    cdp('Page.navigate',{'url':base+f'/ui/tests/browser/voice-workspace-mobile.html?mode={mode}&lang={lang}&scale={scale}&extra=15'})
    wait("!!document.querySelector('[data-voice-document-link]')")
    links='[data-voice-activity-item=first] button[data-voice-document-link]'
    # The default already-selected document must also reveal its panel.
    js("document.querySelector("+json.dumps(links+':first-child')+").focus()")
    press('Enter')
    wait("!!document.querySelector('[data-voice-document-panel]')")
    assert js("document.querySelector('[data-voice-mobile-sheet]').dataset.voiceSheetMode")==mode
    assert js("document.activeElement.hasAttribute('data-voice-back-to-activity')")
    press('Enter')
    wait("!document.querySelector('[data-voice-document-panel]')")
    assert js("document.activeElement.dataset.voiceDocumentLink")=='voice/9e141dcf216f49d7.md'
    # Synthetic recording stays running while a different artifact is viewed.
    click('[data-voice-record]')
    wait('voiceWorkspaceProbe.starts===1')
    writes=js('voiceWorkspaceProbe.writes.length')
    click(links+':last-child')
    wait("document.querySelector('[data-voice-document-title]')?.textContent==='Linked activity document'")
    assert js("document.querySelector('[data-voice-mobile-sheet]').dataset.voiceSheetMode")==mode
    assert js('voiceWorkspaceProbe.stops')==0
    assert js('voiceWorkspaceProbe.writes.length')==writes
    if width>=1024:
     assert js("document.querySelector('[data-voice-document-target][data-state=default]').closest('[role=button]').textContent.includes('voice/9e141dcf216f49d7.md')")
    shot('linked-'+mode+'-'+str(width))
    click('[data-voice-back-to-activity]')
    wait("!document.querySelector('[data-voice-document-panel]')")
    assert js("document.activeElement.dataset.voiceDocumentLink")=='voice/linked-activity.md'
    click('[data-voice-record]')
    wait('voiceWorkspaceProbe.stops===1')
    assert js('voiceWorkspaceProbe.errors')==[],js('voiceWorkspaceProbe.errors')
    results.append({'regression':'linked-document','mode':mode,'width':width,'height':height})

  def reachable(selector):
   return js("(()=>{const e=document.querySelector("+json.dumps(selector)+");const r=e.getBoundingClientRect();return r.width>0&&r.height>0&&e.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2))})()")
  touch_scroll=False
  def wheel_to(selector):
   # User scrolling only: scrollIntoView can move overflow:hidden boxes and mask clipping.
   for _ in range(25):
    if reachable(selector):return
    dims=js("""(() => {
      const e = document.querySelector("""+json.dumps(selector)+""");
      const r = e.getBoundingClientRect();
      const middle = r.y + r.height / 2;
      const chain = [];
      for (let p = e.parentElement; p; p = p.parentElement) {
        if (/auto|scroll/.test(getComputedStyle(p).overflowY) && p.scrollHeight > p.clientHeight + 1)
          chain.push(p);
      }
      // Reveal outer panels before scrolling a result within its own feed.
      for (const p of chain.reverse()) {
        const b = p.getBoundingClientRect();
        let top = Math.max(0, b.top), bottom = Math.min(innerHeight, b.bottom);
        for (let a = p.parentElement; a; a = a.parentElement) {
          if (/auto|scroll|hidden/.test(getComputedStyle(a).overflowY)) {
            const q = a.getBoundingClientRect();
            top = Math.max(top, q.top);
            bottom = Math.min(bottom, q.bottom);
          }
        }
        const delta = middle > (top + bottom) / 2 ? 65 : -65;
        if (bottom <= top || (middle >= top && middle <= bottom) ||
            (delta > 0 && p.scrollTop + p.clientHeight >= p.scrollHeight - 1) ||
            (delta < 0 && p.scrollTop === 0)) continue;
        return {x: b.right - 3, y: (top + bottom) / 2, delta};
      }
      return null;
    })()""")
    if dims is None:break
    if touch_scroll:
     cdp('Input.dispatchTouchEvent',{'type':'touchStart','touchPoints':[{'x':dims['x']-10,'y':dims['y']}]})
     for step in range(1,9):
      cdp('Input.dispatchTouchEvent',{'type':'touchMove','touchPoints':[{'x':dims['x']-10,'y':dims['y']-dims['delta']*step/8}]})
      time.sleep(.02)
     cdp('Input.dispatchTouchEvent',{'type':'touchEnd','touchPoints':[]})
    else:
     cdp('Input.dispatchMouseEvent',{'type':'mouseWheel','x':dims['x'],'y':dims['y'],'deltaX':0,'deltaY':dims['delta']})
    time.sleep(.08)
   shot('unreachable')
   raise AssertionError('unreachable by scrolling: '+selector)
  def pointer_click(selector):
   wheel_to(selector)
   pos=js("(()=>{const r=document.querySelector("+json.dumps(selector)+").getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()")
   for kind in ['mousePressed','mouseReleased']:
    cdp('Input.dispatchMouseEvent',{'type':kind,**pos,'button':'left','clickCount':1})
   time.sleep(.15)
  for theme in ['light','dark']:
   touch_scroll=theme=='dark'
   cdp('Emulation.setTouchEmulationEnabled',{'enabled':touch_scroll,'maxTouchPoints':1})
   for width,height,lang,scale in [(320,568,'ja',125),(390,667,'en',100)]:
    cdp('Emulation.setDeviceMetricsOverride',{'width':width,'height':height,'deviceScaleFactor':1,'mobile':True})
    cdp('Page.navigate',{'url':base+f'/ui/tests/browser/voice-workspace-mobile.html?mode=prompt&theme={theme}&lang={lang}&scale={scale}&extra=15'})
    wait("!!document.querySelector('[data-voice-activity-item=first]')")
    click('.voice-mobile-prompt-toggle')
    pointer_click('[data-voice-workspace-optimize]')
    wait("voiceWorkspaceProbe.writes.some(r=>r.body.kind==='prompt_refine')")
    wheel_to('[data-voice-workspace-optimize]')
    shot('short-optimize-'+theme+'-'+str(width))
    pointer_click('[data-voice-activity-item=first] button[data-voice-document-link]:last-child')
    wait("document.querySelector('[data-voice-document-title]')?.textContent==='Linked activity document'")
    click('[data-voice-back-to-activity]')
    wait("!document.querySelector('[data-voice-document-panel]')")
    assert reachable('[data-voice-activity-item=first] button[data-voice-document-link]:last-child')
    # Activity itself still scrolls, including older results.
    js("document.querySelector('[data-voice-activity-scroll]').scrollTop=0")
    pos=js("(()=>{const r=document.querySelector('[data-voice-activity-item=first]').getBoundingClientRect();return {x:r.x+r.width/2,y:Math.min(innerHeight-50,r.y+20)}})()")
    cdp('Input.dispatchMouseEvent',{'type':'mouseWheel',**pos,'deltaX':0,'deltaY':300})
    time.sleep(.2)
    assert js("document.querySelector('[data-voice-activity-scroll]').scrollTop")>0
    assert js('voiceWorkspaceProbe.errors')==[],js('voiceWorkspaceProbe.errors')
    shot('short-activity-'+theme+'-'+str(width))
    results.append({'regression':'short-prompt','theme':theme,'width':width,'height':height})

  cdp('Emulation.setTouchEmulationEnabled',{'enabled':False})
  # Same-document navigation preserves unsaved text; canceling a switch keeps the current view.
  cdp('Emulation.setDeviceMetricsOverride',{'width':1440,'height':900,'deviceScaleFactor':1,'mobile':False})
  cdp('Page.navigate',{'url':base+'/ui/tests/browser/voice-workspace-mobile.html?mode=document&lang=en'})
  wait("!!document.querySelector('[data-voice-document-panel] h1')")
  click('[data-voice-document-actions] button:last-child')
  value('[data-voice-document-panel] textarea','Unsaved linked document notes')
  click('[data-voice-mode-tabs] button:nth-child(2)')
  wait("!document.querySelector('[data-voice-document-panel]')")
  click('[data-voice-activity-item=first] button[data-voice-document-link]:first-child')
  wait("!!document.querySelector('[data-voice-document-panel] textarea')")
  assert js("document.querySelector('[data-voice-document-panel] textarea').value")=='Unsaved linked document notes'
  click('[data-voice-back-to-activity]')
  js('window.confirm=()=>false')
  click('[data-voice-activity-item=first] button[data-voice-document-link]:last-child')
  assert not js("!!document.querySelector('[data-voice-document-panel]')")
  js('window.confirm=()=>true')
  click('[data-voice-activity-item=first] button[data-voice-document-link]:last-child')
  wait("document.querySelector('[data-voice-document-title]')?.textContent==='Linked activity document'")
  assert not js("!!document.querySelector('[data-voice-document-panel] textarea')")
  assert not js("voiceWorkspaceProbe.writes.some(r=>r.url.includes('/settings'))")
  assert js('voiceWorkspaceProbe.errors')==[]
  results.append({'regression':'linked-document-draft-guard'})
  for theme in ['light','dark']:
   for width,lang in [(1440,'en'),(390,'zh'),(320,'ja')]:
    for mode in ['document','instruction','prompt']:
     cdp('Emulation.setDeviceMetricsOverride',{'width':width,'height':1000,'deviceScaleFactor':1,'mobile':width<640})
     cdp('Page.navigate',{'url':base+'/ui/tests/browser/voice-workspace-mobile.html?mode='+mode+'&theme='+theme+'&lang='+lang+'&scale='+('125' if width==320 else '100')+'&extra=15'})
     wait("!!document.querySelector('[data-voice-mobile-sheet]') && !!window.voiceWorkspaceProbe")
     wait("!!document.querySelector('[data-voice-document-panel] h1')" if mode=='document' else "!!document.querySelector('[data-voice-activity-item]')")
     time.sleep(.2)
     assert js("!!document.querySelector('[data-voice-document-panel]')")==(mode=='document')
     box=js("(()=>{const e=document.querySelector('[data-voice-mobile-sheet]');const r=e.getBoundingClientRect();return {left:r.left,right:r.right,scroll:e.scrollWidth,client:e.clientWidth}})()")
     assert box['left']>=0 and box['right']<=width+.5 and box['scroll']<=box['client']+1,(theme,width,mode,box)
     if mode=='document':
      click('[data-voice-document-actions] button:last-child')
      wait("!!document.querySelector('[data-voice-document-panel] textarea')")
      value('[data-voice-document-panel] textarea','Unsaved meeting notes')
      click('[data-voice-mode-tabs] button:nth-child(2)')
      wait("!!document.querySelector('[data-voice-body-mode=instruction]')")
      assert not js("!!document.querySelector('[data-voice-document-panel]')")
      click('[data-voice-mode-tabs] button:first-child')
      wait("!!document.querySelector('[data-voice-document-panel] textarea')")
      assert js("document.querySelector('[data-voice-document-panel] textarea').value")=='Unsaved meeting notes'
     elif mode=='instruction':
      value('[data-voice-instruction-input]','Keep this request while checking the document')
      click('[data-voice-mode-tabs] button:first-child')
      wait("!!document.querySelector('[data-voice-document-panel]')")
      click('[data-voice-mode-tabs] button:nth-child(2)')
      wait("!!document.querySelector('[data-voice-instruction-input]')")
      assert js("document.querySelector('[data-voice-instruction-input]').value")=='Keep this request while checking the document'
     else:
      if width<640:click('.voice-mobile-prompt-toggle')
      assert js("document.querySelector('[data-voice-workspace-optimize]').getBoundingClientRect().width")>0
      assert js("document.querySelector('[data-voice-prompt-description]').textContent.includes('测试草稿')")
      assert not js("document.querySelector('[data-voice-workspace-optimize]').disabled")
      click('[data-voice-workspace-optimize]')
      wait("voiceWorkspaceProbe.writes.some(r=>r.body.kind==='prompt_refine')")
      request=js("voiceWorkspaceProbe.writes.find(r=>r.body.kind==='prompt_refine')")
      assert request['body']['composer_text']=='测试草稿' and request['body']['operation']=='replace_with_refined_prompt',request
      assert request['body']['trigger']['trigger_kind']=='composer_prompt_refine',request
      assert js("document.querySelector('[data-voice-workspace-optimize]').disabled")
     assert js('voiceWorkspaceProbe.starts')==0
     assert js('voiceWorkspaceProbe.errors')==[],js('voiceWorkspaceProbe.errors')
     shot(theme+'-'+mode+'-'+str(width))
     results.append({'theme':theme,'mode':mode,'width':width,'lang':lang})
    cdp('Page.navigate',{'url':base+'/ui/tests/browser/codex-voice-duplex.html?theme='+theme+'&lang='+lang+'&scale='+('125' if width==320 else '100')})
    wait("!!document.querySelector('[aria-controls=codex-voice-settings-page]')")
    click('[aria-controls=codex-voice-settings-page]')
    wait("!!document.querySelector('#codex-voice-settings-notifications-tab')")
    click('#codex-voice-settings-notifications-tab')
    wait("document.querySelectorAll('#codex-voice-settings-notifications-panel select').length===2")
    pane='#codex-voice-settings-notifications-panel'
    assert js("document.querySelector('"+pane+"').getBoundingClientRect().width")<=1120
    value(pane+' input[type=search]','no-such-group')
    assert js("document.querySelector('"+pane+" [role=status]')!==null")
    assert js("document.querySelectorAll('"+pane+" select').length")==0
    value(pane+' input[type=search]','')
    js("(()=>{const e=document.querySelector('"+pane+" select');e.value='to_user';e.dispatchEvent(new Event('change',{bubbles:true}));})()")
    wait("document.querySelector('"+pane+" select').value==='to_user'")
    js("voiceDuplexProbe.failNextSave=true;const e=document.querySelector('"+pane+" select');e.value='all_chat';e.dispatchEvent(new Event('change',{bubbles:true}));")
    wait("!!document.querySelector('"+pane+" [role=alert]')")
    assert js("document.querySelector('"+pane+" select').value")=='to_user'
    shot(theme+'-notifications-'+str(width))
    assert js('voiceDuplexProbe.errors')==[],js('voiceDuplexProbe.errors')
    results.append({'theme':theme,'mode':'notifications','width':width,'lang':lang})
  out.joinpath('results.json').write_text(json.dumps(results,indent=2))
  print(json.dumps({'passed':len(results),'output':str(out)}))
  sock.close()
 finally:
  p.terminate();p.wait(timeout=10)
