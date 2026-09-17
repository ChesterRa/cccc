#!/usr/bin/env python3
"""Search, settings and Presentation browser regressions against synthetic AppShell transports.

Use the isolated Vite setup documented in group-work.py. Requires Chrome, requests and
websocket-client. Optional env: CHROME_BIN, CCCC_GROUP_WORK_BASE_URL,
CCCC_TASK_SURFACES_OUTPUT_DIR. Always uses a new temporary browser profile.
Keep frontend files and node_modules unchanged while the fixture server is running.
"""
import base64, json, os, subprocess, tempfile, time
from pathlib import Path
import requests, websocket
out=Path(os.environ.get('CCCC_TASK_SURFACES_OUTPUT_DIR') or tempfile.mkdtemp(prefix='cccc-task-surfaces-'));out.mkdir(parents=True,exist_ok=True)
base_url=os.environ.get('CCCC_GROUP_WORK_BASE_URL','http://127.0.0.1:15559').rstrip('/')
with tempfile.TemporaryDirectory(prefix='cccc-ui-polish-chrome-',ignore_cleanup_errors=True) as profile:
 browser=subprocess.Popen([os.environ.get('CHROME_BIN','/usr/bin/google-chrome'),'--headless=new','--no-sandbox','--remote-debugging-port=0','--remote-allow-origins=*','--user-data-dir='+profile,'about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 sock=None
 try:
  for _ in range(100):
   if (Path(profile)/'DevToolsActivePort').exists():break
   time.sleep(.1)
  port=(Path(profile)/'DevToolsActivePort').read_text().splitlines()[0]
  tab=requests.put(f'http://127.0.0.1:{port}/json/new?about:blank',timeout=5).json()
  sock=websocket.create_connection(tab['webSocketDebuggerUrl'],timeout=20);seq=0
  def cdp(method,params=None):
   global seq
   seq+=1;sock.send(json.dumps({'id':seq,'method':method,'params':params or {}}))
   while True:
    r=json.loads(sock.recv())
    if r.get('id')==seq:
     if 'error' in r:raise RuntimeError(r['error'])
     return r['result']
  def js(expr):
   r=cdp('Runtime.evaluate',{'expression':expr,'returnByValue':True,'awaitPromise':True})
   if 'exceptionDetails' in r:raise RuntimeError(r['exceptionDetails'])
   return r.get('result',{}).get('value')
  def wait(expr):
   for _ in range(100):
    if js(expr):return
    time.sleep(.1)
   raise AssertionError(expr+'\n'+str(js('document.body.innerText.slice(-2500)')))
  def shot(name):out.joinpath(name+'.png').write_bytes(base64.b64decode(cdp('Page.captureScreenshot',{'format':'png'})['data']))
  def key(k,modifiers=0):
   codes={'Enter':13,'Escape':27,'Tab':9,'ArrowRight':39,'ArrowLeft':37,'ArrowDown':40,'ArrowUp':38,'a':65}
   for t in ['keyDown','keyUp']:cdp('Input.dispatchKeyEvent',{'type':t,'key':k,'code':k,'windowsVirtualKeyCode':codes.get(k,0),'modifiers':modifiers,**({'text':'\r'} if k=='Enter' and t=='keyDown' else {})})
   time.sleep(.06)
  def click(sel):
   point=js(f"(()=>{{const e=document.querySelector({json.dumps(sel)}); e.scrollIntoView({{block:'nearest'}}); const r=e.getBoundingClientRect();return {{x:r.x+r.width/2,y:r.y+r.height/2}};}})()")
   cdp('Input.dispatchMouseEvent',{'type':'mousePressed','button':'left','clickCount':1,**point});cdp('Input.dispatchMouseEvent',{'type':'mouseReleased','button':'left','clickCount':1,**point});time.sleep(.12)
  def typein(sel,text):
   click(sel);key('a',2);cdp('Input.insertText',{'text':text});time.sleep(.1)
  def size(w,h):
   cdp('Emulation.setDeviceMetricsOverride',{'width':w,'height':h,'deviceScaleFactor':1,'mobile':w<640});time.sleep(.2)
  def dialog_ok():
   return js("(()=>{const e=document.querySelector('[role=dialog][aria-modal=true]');const r=e.getBoundingClientRect();return {left:r.left,right:r.right,top:r.top,bottom:r.bottom,width:innerWidth,height:innerHeight,scroll:e.scrollWidth,client:e.clientWidth}})()")
  size(1440,1000);cdp('Page.navigate',{'url':base_url+'/ui/tests/browser/group-work.html'})
  wait('!!window.groupWorkProbe && !!document.querySelector("textarea")')
  js('groupWorkProbe.openSearch()');wait('document.activeElement?.id==="message-search-query"')
  shot('search-initial')
  assert 'No results' not in js('document.querySelector("[role=dialog][aria-modal=true]").innerText')
  typein('#message-search-query','Release');key('Enter');wait('!!document.querySelector("article mark")');shot('search-results')
  # The native sender selector supports the keyboard and keeps the outer dialog open.
  js('document.querySelector("select[aria-label=By]").focus()');key('ArrowDown');wait('document.querySelector("select").value==="user"')
  key('ArrowDown');wait('document.querySelector("select").value==="system"')
  key('ArrowDown');wait('document.querySelector("select").value==="actor-1"')
  assert js('document.querySelector("select").selectedOptions[0].textContent')=='Foreman'
  key('Enter');key('Escape');assert js('!!document.querySelector("[role=dialog][aria-modal=true]")')
  shot('search-sender-menu')
  typein('#message-search-query','another draft');assert js('document.querySelector("mark").textContent')=='Release'
  js('groupWorkProbe.setSearchMode("empty")');key('Enter');wait('document.querySelector("[role=dialog][aria-modal=true]").textContent.includes("No results")')
  js('groupWorkProbe.setSearchMode("error")');key('Enter');wait('!!document.querySelector("[role=alert]")');assert 'No results' not in js('document.querySelector("[role=dialog][aria-modal=true]").innerText')
  key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
  for lang,w in [('en',320),('zh',390),('ja',768)]:
   size(w,844);js(f'groupWorkProbe.language("{lang}");groupWorkProbe.openSearch()');wait('document.activeElement?.id==="message-search-query"')
   box=dialog_ok();assert box['scroll']<=box['client']+1 and box['right']<=w+.5,(lang,box)
   for _ in range(8):
    key('Tab');assert js('!!document.activeElement.closest("[role=dialog][aria-modal=true]")')
   key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
  size(1440,1000);js('groupWorkProbe.language("en")')
  js('groupWorkProbe.openSettings("global","branding")');wait('!!document.querySelector("#branding-product-name")');time.sleep(.3);shot('branding-desktop')
  typein('#branding-product-name','Release Studio');key('Enter');wait('document.querySelector("[aria-labelledby=settings-modal-title] [role=status]")?.textContent.includes("saved")')
  assert js('groupWorkProbe.requests.some(r=>r.path==="/api/v1/branding"&&r.method!=="GET")')
  matrix=[]
  for lang in ['en','zh','ja']:
   js(f'groupWorkProbe.language("{lang}")')
   for w,h in [(1440,1000),(1024,768),(768,1024),(390,844),(320,720)]:
    size(w,h)
    for dark in [False,True]:
     js(f'groupWorkProbe.setDark({str(dark).lower()})');time.sleep(.08)
     box=dialog_ok();assert box['left']>=-.5 and box['right']<=w+.5 and box['top']>=-.5 and box['bottom']<=h+.5 and box['scroll']<=box['client']+1,(lang,dark,box)
     overflow=js("[...document.querySelectorAll('[role=dialog][aria-modal=true] button,[role=dialog][aria-modal=true] input')].filter(e=>e.getBoundingClientRect().width>0&&!e.closest('.scrollbar-hide')).filter(e=>{const r=e.getBoundingClientRect();return r.left<0||r.right>innerWidth+1}).map(e=>e.textContent)")
     assert not overflow,(lang,w,dark,overflow)
     if w<640:
      assert js("(()=>{const e=[...document.querySelectorAll('[aria-current=page]')].find(e=>e.getBoundingClientRect().width>0);const r=e.getBoundingClientRect();return r.left>=15&&r.right<=innerWidth-20})()"),(lang,w,'active tab under edge fade')
     matrix.append({'lang':lang,'width':w,'dark':dark,'box':box})
    if lang=='ja' and w==390:shot('branding-mobile-ja-dark')
  size(1440,1000);js('groupWorkProbe.setDark(false);groupWorkProbe.language("en")');time.sleep(.2)
  tabs=[('global','account'),('global','actorProfiles'),('global','webAccess'),('global','webModels'),('global','developer'),('global','capabilities'),('group','delivery'),('group','messaging'),('group','transcript'),('group','im'),('group','copyGroups'),('group','assistants'),('group','space'),('group','guidance'),('group','automation')]
  for scope,tab in tabs:
   js(f'groupWorkProbe.openSettings("{scope}","{tab}")');time.sleep(.5)
   assert js('!!document.querySelector("[role=dialog][aria-modal=true]")'),tab
   box=dialog_ok();assert box['scroll']<=box['client']+1,(tab,box)
   if tab in ['account','delivery','im']:shot('settings-'+tab)
  assert js('groupWorkProbe.errors')==[],js('groupWorkProbe.errors')
  key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
  click('[data-group-presentation-trigger]');wait('!!document.querySelector("[data-presentation-density=compact]")');shot('presentation-compact')
  click('[data-presentation-density=compact] button[aria-label*="slot 1"]');wait('!!document.querySelector("[data-presentation-slot-navigation]")');shot('presentation-reading')
  js("groupWorkProbe.group.setState(s=>({groupPresentation:{...s.groupPresentation,slots:[...s.groupPresentation.slots,{slot_id:'slot-2',card:{title:'Review notes',card_type:'markdown',content:{mode:'inline',markdown:'# Findings\\n\\nSecond slot with independent content.'},published_at:'second'}}]}}))")
  click('[data-presentation-slot-navigation] button:nth-child(2)');wait('document.body.innerText.includes("Second slot with independent content")')
  assert js('document.activeElement.matches("[data-presentation-slot-navigation] button[aria-pressed=true]")')
  click('button[aria-label="Open in window"]');wait('!!document.querySelector("[role=dialog][aria-modal=true] [data-presentation-slot-navigation]")')
  click('[role=dialog][aria-modal=true] [data-presentation-slot-navigation] button:first-child');wait('document.querySelector("[role=dialog][aria-modal=true]").textContent.includes("All checks have finished")')
  key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
  # Modal preference remains effective on the next slot click.
  click('[data-presentation-density] button[aria-label*="slot 2"]');wait('!!document.querySelector("[role=dialog][aria-modal=true]")')
  for w in [390,320]:
   size(w,844);box=dialog_ok();assert box['scroll']<=box['client']+1 and box['right']<=w+.5,box
   for _ in range(10):
    key('Tab');assert js('!!document.activeElement.closest("[role=dialog][aria-modal=true]")')
  shot('presentation-mobile')
  size(1440,1000);click('button[aria-label="Open beside chat"]');wait('!document.querySelector("[role=dialog][aria-modal=true]")&&!!document.querySelector("[data-presentation-slot-navigation]")')
  for width in [280,600,360]:
   js(f'groupWorkProbe.ui.getState().setChatSidePanelLayout("g1",{{width:{width}}})');time.sleep(.2)
   assert js("[...document.querySelectorAll('[data-presentation-slot-navigation] button')].every(e=>{const r=e.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth})")
  click('button[aria-label="Collapse to compact slots"]');wait('!!document.querySelector("[data-presentation-density=compact]")');assert js('document.activeElement.closest("[data-presentation-density=compact]")!==null')
  shot('presentation-returned')
  click('[data-presentation-density=compact] button[aria-label="Expand Presentation"]');wait('!!document.querySelector("[data-presentation-slot-navigation]")')
  js('groupWorkProbe.setReadOnly(true)');time.sleep(.15)
  assert js('document.querySelector("[data-presentation-slot-navigation] button:nth-child(3)").disabled')
  click('[data-presentation-slot-navigation] button:first-child');wait('document.activeElement.matches("[data-presentation-slot-navigation] button[aria-pressed=true]")')
  js('groupWorkProbe.setReadOnly(false)');time.sleep(.15)
  click('[data-presentation-slot-navigation] button:nth-child(3)');assert js('groupWorkProbe.modals.getState().presentationPin.slotId')=='slot-3'
  js('groupWorkProbe.modals.getState().setPresentationPin(null)')
  click('button[aria-label="Hide presentation"]');wait('!document.querySelector("#group-side-panel")')
  assert js('groupWorkProbe.group.getState().groupPresentation.slots.filter(s=>s.card).length')==2
  assert js('groupWorkProbe.errors')==[],js('groupWorkProbe.errors')
  out.joinpath('proof.json').write_text(json.dumps({'matrix':matrix,'errors':js('groupWorkProbe.errors'),'requests':js('groupWorkProbe.requests')},ensure_ascii=False,indent=2))
  print('PASS search lifecycle, settings matrix, all settings tabs, compact/read/switch/collapse Presentation')
 except Exception:
  try:shot('failure');print(js('groupWorkProbe.errors'));print(js("[...document.querySelectorAll('[role=dialog]')].map(e=>({label:e.getAttribute('aria-labelledby'),modal:e.getAttribute('aria-modal'),text:e.textContent.slice(-200)}))"))
  except:pass
  raise
 finally:
  if sock:sock.close()
  browser.terminate();browser.wait(timeout=10)
