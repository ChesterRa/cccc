#!/usr/bin/env python3
"""Reading controls, scaling and form continuity with isolated synthetic transports.

Use the isolated Vite setup documented in group-work.py. Requires Chrome, requests and
websocket-client. Optional env: CHROME_BIN, CCCC_GROUP_WORK_BASE_URL,
CCCC_READING_SURFACES_OUTPUT_DIR. Always uses a new temporary browser profile.
Keep frontend files and node_modules unchanged while the fixture server is running.
"""
import base64, json, os, subprocess, tempfile, time
from pathlib import Path
import requests, websocket
out=Path(os.environ.get('CCCC_READING_SURFACES_OUTPUT_DIR') or tempfile.mkdtemp(prefix='cccc-task-surfaces-'));out.mkdir(parents=True,exist_ok=True)
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
    if r.get('method')=='Fetch.requestPaused':
     svg='<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900"><rect width="1600" height="900" fill="#edf2f7"/><text x="80" y="180" font-size="70">Reference snapshot</text></svg>'
     sock.send(json.dumps({'id':1000000,'method':'Fetch.fulfillRequest','params':{'requestId':r['params']['requestId'],'responseCode':200,'responseHeaders':[{'name':'Content-Type','value':'image/svg+xml'}],'body':base64.b64encode(svg.encode()).decode()}}));continue
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
   codes={'Enter':13,'Escape':27,'Tab':9,'ArrowRight':39,'ArrowLeft':37,'ArrowDown':40,'ArrowUp':38,'a':65,' ':32}
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
  # Action labels honor the same text scale as the body, without changing terminal ANSI.
  js('groupWorkProbe.setDark(true);groupWorkProbe.setTextScale(100)');time.sleep(.2)
  action="[...document.querySelectorAll('button')].find(e=>e.textContent==='Copy text')"
  original=js(f'parseFloat(getComputedStyle({action}).fontSize)')
  js('groupWorkProbe.setTextScale(125)');time.sleep(.2)
  scaled=js(f'parseFloat(getComputedStyle({action}).fontSize)')
  assert original>=12 and abs(scaled/original-1.25)<.02,(original,scaled)
  shot('messages-dark-scaled')
  js('groupWorkProbe.setReadingExamples(true)');wait('!!document.querySelector("[data-cccc-mermaid-target] svg")')
  matrix=[]
  def check_dialog():
   box=dialog_ok();assert box['scroll']<=box['client']+1 and box['right']<=box['width']+.5 and box['left']>=-.5,box
   overflow=js("[...document.querySelectorAll('[role=dialog][aria-modal=true] button,[role=dialog][aria-modal=true] input,[role=dialog][aria-modal=true] a')].filter(e=>e.getBoundingClientRect().width>0).filter(e=>{const r=e.getBoundingClientRect();return r.left<0||r.right>innerWidth+1||e.scrollWidth>e.clientWidth+2}).map(e=>e.textContent||e.getAttribute('aria-label'))")
   assert not overflow,overflow
   for _ in range(4):
    key('Tab');assert js('!!document.activeElement.closest("[role=dialog][aria-modal=true]")')
   return box
  for lang in ['en','zh','ja']:
   js(f'groupWorkProbe.language("{lang}")')
   for dark in [False,True]:
    js(f'groupWorkProbe.setDark({str(dark).lower()})');time.sleep(.15)
    for w in [1440,390,320]:
     size(w,900)
     # Message image and Mermaid share the same static-graphic controls.
     for kind,selector in [('image','[data-reading-examples] button[aria-label*="Architecture drawing"]'),('mermaid','[data-cccc-mermaid-expand]')]:
      click(selector);wait('!!document.querySelector("[role=dialog][aria-modal=true] [data-graphic-viewer]")')
      wait('!document.querySelector("[data-graphic-viewer] button").disabled')
      check_dialog()
      buttons=js("[...document.querySelectorAll('[data-graphic-viewer] button')].map(e=>{const r=e.getBoundingClientRect();return [r.width,r.height]})")
      assert all(a>=35 and b>=35 for a,b in buttons),(kind,buttons)
      if w<640:assert all(a>=43 and b>=43 for a,b in buttons),(kind,buttons)
      if lang=='en' and dark and w in [1440,390]:shot(kind+'-'+str(w))
      key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
      assert js(f'document.activeElement.matches({json.dumps(selector)})'),(kind,'focus return')
      assert js('document.body.style.position')!='fixed'
     matrix.append([lang,dark,w])
  js('groupWorkProbe.setReadingExamples(false);groupWorkProbe.language("en")');size(1440,1000)
  # The quoted snapshot remains a separate modal within the reader; Escape closes only the top layer.
  cdp('Fetch.enable',{'patterns':[{'urlPattern':'*/blobs/reading-example.svg'}]})
  js('groupWorkProbe.modals.getState().setPresentationViewer({groupId:"g1",slotId:"slot-1",surface:"modal",focusRef:{slot_id:"slot-1",card_type:"markdown",title:"Release checklist",snapshot:{path:"state/blobs/reading-example.svg",captured_at:"2026-09-18T00:00:00Z",width:1600,height:900}}})')
  wait('!!document.querySelector(\'button[aria-label="Compare with snapshot"]\')')
  click('button[aria-label="Compare with snapshot"]');click('button[aria-label="Open snapshot"]')
  wait('!!document.querySelector("[data-graphic-viewer] img") && document.querySelector("[data-graphic-viewer] img").naturalWidth>0')
  for w in [1440,390,320]:
   size(w,900)
   assert js("[...document.querySelectorAll('[role=dialog]')].every(e=>e.scrollWidth<=e.clientWidth+1)")
   assert js("(()=>{const buttons=[...document.querySelectorAll('[data-graphic-viewer] button')];return buttons.at(-1).getBoundingClientRect().top===buttons.at(-2).getBoundingClientRect().top})()"), 'zoom controls split across rows'
   key('Tab');assert js('!!document.activeElement.closest("[role=dialog]")?.querySelector("[data-graphic-viewer]")')
  shot('snapshot-mobile');key('Escape');wait('document.querySelectorAll("[role=dialog][aria-modal=true]").length===1')
  key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
  cdp('Fetch.disable')
  # Pin and edit use the same field surfaces, with actual native input controls.
  js('groupWorkProbe.modals.getState().setPresentationPin({groupId:"g1",slotId:"slot-3"})')
  wait('!!document.querySelector("[role=dialog] input[type=url]")')
  for lang,w in [('en',1440),('ja',390),('zh',320)]:
   js(f'groupWorkProbe.language("{lang}")');size(w,900);check_dialog()
  shot('pin-mobile');key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
  # Refresh transport status without discarding an unsaved access configuration.
  js("""window.accessFetch=window.fetch;window.fixturePort=8848;window.accessSaves=[];
    window.fixtureAccess={provider:'off',mode:'tailnet_only',require_access_token:true,enabled:false,status:'stopped',config:{web_host:'127.0.0.1',web_port:8848,web_public_url:''}};
    window.fetch=async(...args)=>{
      const path=new URL(String(args[0]),location.href).pathname;
      if(path.endsWith('/access-tokens'))return Response.json({ok:true,result:{access_tokens:[{token_id:'fixture-admin',user_id:'owner',is_admin:true,allowed_groups:[],created_at:'2026-09-18T00:00:00Z'}]}});
      if(path.endsWith('/remote_access')){
        if(args[1]?.method==='PUT'){
          const draft=JSON.parse(args[1].body);accessSaves.push(draft);fixturePort=draft.web_port;
          fixtureAccess={...fixtureAccess,provider:draft.provider,mode:draft.mode,enabled:draft.enabled,config:{web_host:draft.web_host,web_port:draft.web_port,web_public_url:draft.web_public_url}};
        }
        fixtureAccess.config.web_port=fixturePort;
        return Response.json({ok:true,result:{remote_access:fixtureAccess}});
      }
      return accessFetch(...args);
    }""")
  js('groupWorkProbe.language("en");groupWorkProbe.openSettings("global","webAccess")');size(1440,1000)
  wait('!!document.querySelector(`input[placeholder="8848"]`)')
  typein('input[placeholder="8848"]','8855')
  js("[...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Refresh').click()");time.sleep(.4)
  assert js('document.querySelector(`input[placeholder="8848"]`).value')=='8855'
  typein('input[placeholder="8848"]','8848');js("window.fixturePort=8849;[...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Refresh').click()");time.sleep(.4)
  assert js('document.querySelector(`input[placeholder="8848"]`).value')=='8849'
  # Goal, provider and binding must remain consistent through refresh and Save.
  access_cases=[]
  for label,provider,host,url in [('Private network','manual','0.0.0.0',''),('Local only','off','127.0.0.1',''),('Public URL / tunnel','manual','127.0.0.1','https://workspace.example/ui/')]:
   js(f"[...document.querySelectorAll('button')].find(e=>e.textContent.includes({json.dumps(label)})).click()")
   if url:typein('input[placeholder="https://example.com/ui/"]',url)
   js("[...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Refresh').click()")
   wait("!![...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Refresh'&&!e.disabled)")
   before=js('accessSaves.length')
   js("[...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Save changes').click()")
   wait(f'accessSaves.length==={before+1}')
   sent=js('accessSaves.at(-1)')
   assert (sent['provider'],sent['web_host'],sent['web_public_url'])==(provider,host,url),sent
   assert sent['require_access_token'] is True,sent
   wait("!![...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Saved')")
   assert not js("document.querySelector('[role=alert]')?.textContent")
   access_cases.append(label)
  key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")');js('window.fetch=window.accessFetch')
  # A read-only notebook refresh must preserve a pending selection on both lanes.
  js('groupWorkProbe.language("en");groupWorkProbe.openSettings("group","space")');size(1440,1000)
  wait('document.querySelectorAll("button[role=combobox]").length===2')
  click('button[role=combobox]');wait('!!document.querySelector("[role=option]")')
  js("[...document.querySelectorAll('[role=option]')].find(e=>e.textContent.includes('Next project')).click()")
  wait('document.querySelector("button[role=combobox]").textContent.includes("Next project")')
  js("[...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Refresh').click()")
  time.sleep(.5);assert "Next project" in js('document.querySelector("button[role=combobox]").textContent')
  assert "CCCC" in js('document.querySelectorAll("button[role=combobox]")[1].textContent')
  js("window.notebookFetch=window.fetch;window.notebookBound='notebook-next';window.fetch=async(...args)=>{const r=await notebookFetch(...args);if(new URL(String(args[0]),location.href).pathname.endsWith('/space/status')){const data=await r.json();data.result.bindings.work.remote_space_id=notebookBound;return Response.json(data)}return r}")
  js("[...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Refresh').click()");time.sleep(.4)
  js("window.notebookBound='notebook-fixture';[...document.querySelectorAll('button')].find(e=>e.textContent.trim()==='Refresh').click()");time.sleep(.4)
  assert "CCCC" in js('document.querySelector("button[role=combobox]").textContent')
  js('window.fetch=window.notebookFetch')
  key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
  assert js('groupWorkProbe.errors')==[],js('groupWorkProbe.errors')
  out.joinpath('results.json').write_text(json.dumps({'matrix':matrix,'font':[original,scaled],'access_cases':access_cases,'errors':js('groupWorkProbe.errors')},indent=2))
  print(json.dumps({'passed':len(matrix),'font':[original,scaled],'output':str(out)}),flush=True)
 finally:
  if sock:sock.close()
  browser.terminate()
  try:browser.wait(timeout=5)
  except subprocess.TimeoutExpired:browser.kill();browser.wait()
