#!/usr/bin/env python3
"""Group run-control browser regressions against synthetic AppShell transports.

Use the isolated Vite setup documented in group-work.py. Requires Chrome, requests and
websocket-client. Optional env: CHROME_BIN, CCCC_GROUP_WORK_BASE_URL,
CCCC_GROUP_RUN_OUTPUT_DIR. Always uses a new temporary browser profile.
Keep frontend files and node_modules unchanged while the fixture server is running.
"""
import base64, json, os, subprocess, tempfile, time
from pathlib import Path
import requests, websocket
out=Path(os.environ.get('CCCC_GROUP_RUN_OUTPUT_DIR') or tempfile.mkdtemp(prefix='cccc-group-run-'));out.mkdir(parents=True,exist_ok=True)
base_url=os.environ.get('CCCC_GROUP_WORK_BASE_URL','http://127.0.0.1:15559').rstrip('/')
with tempfile.TemporaryDirectory(prefix='cccc-ui-polish-chrome-',ignore_cleanup_errors=True) as profile:
 # Match Playwright's desktop pointer defaults; bare headless Chrome advertises no pointer.
 browser=subprocess.Popen([os.environ.get('CHROME_BIN','/usr/bin/google-chrome'),'--headless=new','--no-sandbox','--blink-settings=primaryHoverType=2,availableHoverTypes=2,primaryPointerType=4,availablePointerTypes=4','--remote-debugging-port=0','--remote-allow-origins=*','--user-data-dir='+profile,'about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
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
   cdp('Input.dispatchMouseEvent',{'type':'mouseMoved',**point});time.sleep(.15)
   cdp('Input.dispatchMouseEvent',{'type':'mousePressed','button':'left','clickCount':1,**point});cdp('Input.dispatchMouseEvent',{'type':'mouseReleased','button':'left','clickCount':1,**point});time.sleep(.12)
  def typein(sel,text):
   click(sel);key('a',2);cdp('Input.insertText',{'text':text});time.sleep(.1)
  def size(w,h):
   cdp('Emulation.setDeviceMetricsOverride',{'width':w,'height':h,'deviceScaleFactor':1,'mobile':w<640});time.sleep(.2)
  def dialog_ok():
   return js("(()=>{const e=document.querySelector('[role=dialog][aria-modal=true]');const r=e.getBoundingClientRect();return {left:r.left,right:r.right,top:r.top,bottom:r.bottom,width:innerWidth,height:innerHeight,scroll:e.scrollWidth,client:e.clientWidth}})()")
  def mutations():
   return js("groupWorkProbe.requests.filter(r=>/\\/(start|stop|state)$/.test(r.path)&&r.method==='POST')")
  def action(text):
   js(f"[...document.querySelectorAll('[role=menuitem]')].find(e=>e.innerText.startsWith({json.dumps(text)})).setAttribute('data-test-action','true')")
   click('[data-test-action]')
  def header():return 'header [data-group-run-control]'
  def sidebar(gid):return 'aside button[aria-haspopup="menu"][aria-label$="'+('Release workspace' if gid=='g1' else 'Research workspace')+'"]'
  size(1440,900);cdp('Page.navigate',{'url':base_url+'/ui/tests/browser/group-work.html'})
  wait('!!window.groupWorkProbe && !!document.querySelector("header [data-group-run-control]")')
  js('groupWorkProbe.language("en")')
  shot('desktop')
  assert js('document.querySelector("header [data-group-run-control]").innerText')=='Running'
  click(header());wait('!!document.querySelector("[role=menu]")');assert not mutations()
  shot('desktop-actions');key('Escape');assert js('document.activeElement.matches("header [data-group-run-control]")')
  js('document.querySelector("header [data-group-run-control]").focus()');key('ArrowDown')
  wait('document.activeElement?.getAttribute("role")==="menuitem"')
  assert js('document.activeElement.innerText.startsWith("Pause message delivery")')
  key('End');assert js('document.activeElement.innerText.startsWith("Stop Group")')
  key('Home');assert js('document.activeElement.innerText.startsWith("Pause message delivery")')
  key('Tab');assert not js('!!document.querySelector("[role=menu]")')
  # Background Group controls retain both navigation and sorting state.
  click(sidebar('g2'));assert js('groupWorkProbe.group.getState().selectedGroupId')=='g1'
  action('Pause message delivery');wait('groupWorkProbe.ui.getState().busy === ""')
  assert mutations()[-1]['path']=='/api/v1/groups/g2/state'
  assert js('groupWorkProbe.group.getState().groups.find(g=>g.group_id==="g2").state')=='paused'
  assert js('document.querySelector("header [data-group-run-control]").innerText')=='Running'
  click(sidebar('g2'));action('Resume running');wait('groupWorkProbe.group.getState().groups.find(g=>g.group_id==="g2").state==="active"')
  assert not any(r['path'].endswith('/start') for r in mutations())
  # The invisible margin remains clickable; pressing must not shrink the row away.
  p=js('(()=>{const r=document.querySelector(\'aside button[aria-haspopup="menu"][aria-label$="Research workspace"]\').getBoundingClientRect();return {x:r.x+1,y:r.y+r.height/2};})()')
  cdp('Input.dispatchMouseEvent',{'type':'mouseMoved',**p})
  cdp('Input.dispatchMouseEvent',{'type':'mousePressed','button':'left','clickCount':1,**p})
  cdp('Input.dispatchMouseEvent',{'type':'mouseReleased','button':'left','clickCount':1,**p})
  wait('!!document.querySelector("[role=menu]")')
  assert js('groupWorkProbe.group.getState().selectedGroupId')=='g1'
  key('Escape')
  # Mouse motion over the status trigger must not activate the Group row's DnD sensor.
  p=js('(()=>{const r=document.querySelector(\'aside button[aria-haspopup="menu"][aria-label$="Research workspace"]\').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()')
  cdp('Input.dispatchMouseEvent',{'type':'mousePressed','button':'left','clickCount':1,**p})
  cdp('Input.dispatchMouseEvent',{'type':'mouseMoved','button':'left','buttons':1,'x':p['x']+9,'y':p['y']+2})
  assert not js('document.querySelector("[id^=DndLiveRegion]")?.textContent?.includes("picked up")')
  cdp('Input.dispatchMouseEvent',{'type':'mouseReleased','button':'left','clickCount':1,**p});time.sleep(.15)
  assert js('groupWorkProbe.group.getState().selectedGroupId')=='g1';key('Escape')
  # Real action hook: pending spans both entrances and survives navigating away.
  js('groupWorkProbe.setRunTransport(900)');click(sidebar('g2'));action('Stop Group')
  assert js('groupWorkProbe.ui.getState().busy')=='group-stop'
  before=len(mutations());click(header());assert js('[...document.querySelectorAll("[role=menuitem]")].every(e=>e.disabled)');key('Escape')
  js('groupWorkProbe.chooseGroup("g2")');wait('groupWorkProbe.ui.getState().busy === ""')
  assert len(mutations())==before
  assert js('document.querySelector("header [data-group-run-control]").innerText')=='Stopped'
  click(header());action('Start Group');wait('groupWorkProbe.ui.getState().busy === ""')
  assert mutations()[-1]['path']=='/api/v1/groups/g2/start'
  js('groupWorkProbe.setRunTransport(0,true)');click(header());action('Pause message delivery')
  wait('groupWorkProbe.ui.getState().busy === ""')
  assert js('groupWorkProbe.ui.getState().errorMsg').startswith('Research workspace:'),js('groupWorkProbe.ui.getState().errorMsg')
  assert js('document.querySelector("header [data-group-run-control]").innerText')=='Running'
  js('groupWorkProbe.setRunTransport();groupWorkProbe.setGroupStatus("g2","paused",false)')
  click(header());action('Resume running');wait('document.querySelector("header [data-group-run-control]").innerText==="Running"')
  assert [r['path'] for r in mutations()[-2:]]==['/api/v1/groups/g2/state','/api/v1/groups/g2/start']
  js('groupWorkProbe.setGroupStatus("g2","idle",true)');click(header())
  assert js('document.querySelectorAll("[role=menuitem]").length')==3;key('Escape')
  js('groupWorkProbe.setSidebarCollapsed(true)');time.sleep(.15)
  assert not js('!!document.querySelector("aside [data-group-run-control]")')
  js('groupWorkProbe.setSidebarCollapsed(false);groupWorkProbe.setReadOnly(true)');time.sleep(.15)
  assert not js('!!document.querySelector("[data-group-run-control]")')
  js('groupWorkProbe.setReadOnly(false);groupWorkProbe.setGroupStatus("g2","active",true)')
  # Header/menu fit and focus remain intact across locales, scale, and screen sizes.
  matrix=[]
  for lang in ['en','zh','ja']:
   js(f'groupWorkProbe.language("{lang}")')
   for w,h in [(1920,1080),(1440,900),(1024,768),(768,1024),(390,844),(320,720)]:
    size(w,h)
    for scale in [100,125]:
     js(f'groupWorkProbe.setTextScale({scale})');time.sleep(.12)
     for dark in [False,True]:
      js(f'groupWorkProbe.setDark({str(dark).lower()})');time.sleep(.08)
      box=js('(()=>{const e=document.querySelector("header");return {scroll:e.scrollWidth,client:e.clientWidth,buttons:[...e.querySelectorAll("button")].filter(b=>b.getClientRects().length).map(b=>({label:b.getAttribute("aria-label"),r:b.getBoundingClientRect().toJSON()}))}})()')
      assert box['scroll']<=box['client']+1,(lang,w,scale,dark,box)
      assert all(b['r']['left']>=-.5 and b['r']['right']<=w+.5 for b in box['buttons']),(lang,w,scale,box)
      click(header());wait('!!document.querySelector("[role=menu]")')
      assert js('(()=>{const e=document.querySelector("[role=menu]");const r=e.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth&&r.top>=0&&r.bottom<=innerHeight&&e.scrollWidth<=e.clientWidth})()'),(lang,w,scale)
      if lang=='ja' and w==320 and scale==125 and dark:shot('mobile-ja-large-dark')
      key('Escape');assert js('document.activeElement.matches("header [data-group-run-control]")')
      matrix.append({'language':lang,'width':w,'scale':scale,'dark':dark})
  # Real touch input cannot bubble into row dragging either.
  size(390,844);js('groupWorkProbe.language("en");groupWorkProbe.setTextScale(100)')
  click('[data-sidebar-toggle]');wait('!!document.querySelector(\'aside button[aria-haspopup="menu"][aria-label$="Release workspace"]\')')
  cdp('Emulation.setTouchEmulationEnabled',{'enabled':True})
  p=js('(()=>{const r=document.querySelector(\'aside button[aria-haspopup="menu"][aria-label$="Release workspace"]\').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()')
  cdp('Input.dispatchTouchEvent',{'type':'touchStart','touchPoints':[p]});time.sleep(.4)
  cdp('Input.dispatchTouchEvent',{'type':'touchMove','touchPoints':[{'x':p['x']+8,'y':p['y']}]})
  cdp('Input.dispatchTouchEvent',{'type':'touchEnd','touchPoints':[]});time.sleep(.2)
  assert js('groupWorkProbe.group.getState().selectedGroupId')=='g2'
  assert not js('document.querySelector("[id^=DndLiveRegion]")?.textContent?.includes("picked up")')
  assert js('groupWorkProbe.errors')==[],js('groupWorkProbe.errors')
  out.joinpath('proof.json').write_text(json.dumps({'matrix':matrix,'errors':js('groupWorkProbe.errors'),'mutations':mutations()},ensure_ascii=False,indent=2))
  print('PASS explicit Group targets, pending/navigation, errors, lifecycle actions, mouse/touch isolation, keyboard and 72 layout cases')
 except Exception:
  try:shot('failure');print(js('({errors:groupWorkProbe.errors,body:document.body.innerText.slice(0,1600),groups:groupWorkProbe.group.getState().groups})'))
  except:pass
  raise
 finally:
  if sock:sock.close()
  browser.terminate();browser.wait(timeout=10)
