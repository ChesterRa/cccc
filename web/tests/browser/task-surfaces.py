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
  tabs=[('global','account'),('global','actorProfiles'),('global','webAccess'),('global','webModels'),('global','developer'),('global','capabilities'),('group','delivery'),('group','messaging'),('group','transcript'),('group','im'),('group','copyGroups'),('group','connections'),('group','assistants'),('group','space'),('group','guidance'),('group','automation')]
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
  # Shared contrast requirements, not snapshots of one palette. Decorative dividers
  # deliberately have no 3:1 requirement; input boundaries do.
  js("""window.surfaceContrast=(foreground,background)=>{
    const canvas=document.createElement('canvas');canvas.width=canvas.height=1;
    const context=canvas.getContext('2d');
    const rgb=color=>{context.clearRect(0,0,1,1);context.fillStyle=color;context.fillRect(0,0,1,1);return [...context.getImageData(0,0,1,1).data];};
    const luminance=color=>rgb(color).slice(0,3).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4).reduce((sum,v,i)=>sum+v*[.2126,.7152,.0722][i],0);
    const a=luminance(foreground),b=luminance(background);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);
  };window.surfaceToken=name=>getComputedStyle(document.documentElement).getPropertyValue(name).trim();""")
  surfaces=[]
  for dark in [False,True]:
   size(1440,1000);js(f'groupWorkProbe.setDark({str(dark).lower()});groupWorkProbe.language("en");groupWorkProbe.setTextScale(125)');time.sleep(.2)
   ratios=js("""(()=>{const ratios=[];for(const bg of ['--color-bg-primary','--color-bg-secondary','--glass-bg','--glass-panel-bg']) for(const fg of ['--color-text-primary','--color-text-secondary','--color-text-tertiary','--color-text-muted']) ratios.push({fg,bg,ratio:surfaceContrast(surfaceToken(fg),surfaceToken(bg))});return ratios;})()""")
   assert all(r['ratio']>=4.5 for r in ratios),(dark,ratios)
   js('groupWorkProbe.openSettings("global","branding")');wait('!!document.querySelector("#branding-product-name")')
   click('#branding-product-name');key('Tab')
   assert js('document.activeElement.matches("button")&&getComputedStyle(document.activeElement).boxShadow!=="none"'),'keyboard focus must remain visible'
   time.sleep(.2)  # Measure the resting border after its focus transition.
   field=js("""(()=>{const s=getComputedStyle(document.querySelector('#branding-product-name'));return {border:surfaceContrast(s.borderTopColor,s.backgroundColor),text:surfaceContrast(s.color,s.backgroundColor)};})()""")
   assert field['border']>=3 and field['text']>=4.5,(dark,field)
   shot(('dark' if dark else 'light')+'-branding-125')
   js('groupWorkProbe.setNotebookWarning("Notebook refresh is temporarily unavailable.");groupWorkProbe.openSettings("group","space")')
   wait('document.querySelector("[aria-labelledby=settings-modal-title] [role=status]")?.textContent.includes("Notebook refresh is temporarily unavailable")')
   before=js('groupWorkProbe.requests.filter(r=>r.method!=="GET").length')
   wait('document.querySelector("[aria-labelledby=settings-modal-title]").textContent.includes("Saved Google session is verified.")')
   shot(('dark' if dark else 'light')+'-notebook-125')
   for lang,w in [('en',1440),('zh',390),('ja',320)]:
    size(w,1000 if w>640 else 844);js(f'groupWorkProbe.language("{lang}")');time.sleep(.2)
    box=dialog_ok();assert box['scroll']<=box['client']+1 and box['right']<=w+.5,(dark,lang,box)
    if w<640:
     assert js("[...document.querySelectorAll('[aria-labelledby=settings-modal-title] button[aria-pressed]')].filter(e=>e.getBoundingClientRect().width>0).every(e=>e.getBoundingClientRect().width>=innerWidth*.35)"),(dark,lang,'scope buttons squeeze each other')
    click('[aria-labelledby=settings-modal-title] button[role=combobox]')
    wait('!!document.querySelector("[role=listbox]")')
    assert js("(()=>{const e=document.querySelector('[role=listbox]');const r=e.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth+.5})()"),(dark,lang,'notebook menu overflow')
    assert js("(()=>{const e=document.querySelector('[role=listbox]').closest('[data-radix-popper-content-wrapper]');return getComputedStyle(e.firstElementChild).backgroundColor.startsWith('rgb(')})()"),'floating menu must be opaque'
    key('Escape');wait('!document.querySelector("[role=listbox]")')
    assert js('document.activeElement.matches("button[role=combobox]")'),'menu restores trigger focus'
    if w==320:shot(('dark' if dark else 'light')+'-notebook-mobile-125')
   assert js('groupWorkProbe.requests.filter(r=>r.method!=="GET").length')==before,'opening settings and menus must not mutate state'
   key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
   size(1440,1000);js('groupWorkProbe.language("en")')
   click('[data-workspace-files-toggle]');wait('!!document.querySelector("#group-side-panel")&&document.querySelector("#group-side-panel").textContent.includes("README.md")')
   assert js("parseFloat(getComputedStyle(document.querySelector('[data-side-panel-header] h2')).fontSize)>=17.5&&parseFloat(getComputedStyle(document.querySelector('[data-side-panel-header] p')).fontSize)>=15")
   shot(('dark' if dark else 'light')+'-files-125')
   click('[data-workspace-files-toggle]');wait('!document.querySelector("#group-side-panel")')
   click('[data-group-presentation-trigger]');wait('!!document.querySelector("#group-side-panel")')
   js('groupWorkProbe.ui.getState().setChatSidePanelLayout("g1",{compact:false})');time.sleep(.2)
   shot(('dark' if dark else 'light')+'-presentation-125')
   click('[data-group-presentation-trigger]');wait('!document.querySelector("#group-side-panel")')
   surfaces.append({'dark':dark,'contrast':ratios,'input':field})
  # Apply the same layout and text-scale checks across every settings section.
  settings_matrix=[]
  js('groupWorkProbe.setTextScale(125)')
  for lang in ['en','zh','ja']:
   js(f'groupWorkProbe.language("{lang}")')
   for scope,tab in tabs+[('global','branding')]:
    js(f'groupWorkProbe.openSettings("{scope}","{tab}")');time.sleep(.2)
    for dark in [False,True]:
     js(f'groupWorkProbe.setDark({str(dark).lower()})')
     for width in [1440,320]:
      size(width,1000 if width>640 else 844);time.sleep(.04)
      box=dialog_ok();assert box['scroll']<=box['client']+1,(lang,tab,dark,width,box)
      overflow=js("[...document.querySelectorAll('[aria-labelledby=settings-modal-title] button,[aria-labelledby=settings-modal-title] input,[aria-labelledby=settings-modal-title] textarea')].filter(e=>e.getBoundingClientRect().width>2&&!e.closest('.scrollbar-hide')).filter(e=>{const r=e.getBoundingClientRect();return r.left<0||r.right>innerWidth+1||(e.tagName==='BUTTON'&&e.scrollWidth>e.clientWidth+2)}).map(e=>e.textContent||e.getAttribute('aria-label')||e.type)")
      assert not overflow,(lang,tab,dark,width,overflow)
      settings_matrix.append({'lang':lang,'tab':tab,'dark':dark,'width':width})
      if lang=='en' and dark and width==1440:shot('settings-'+tab+'-125')
  # An independent Profile keeps its object boundary; its edit fields stay usable
  # in the nested dialog in either theme and all supported UI languages.
  js('groupWorkProbe.language("en");groupWorkProbe.openSettings("global","actorProfiles")')
  wait('document.querySelector("[aria-labelledby=settings-modal-title]").textContent.includes("Review and implementation")')
  js("[...document.querySelectorAll('[aria-labelledby=settings-modal-title] button')].find(e=>e.textContent==='Edit').click()")
  wait('document.querySelectorAll("[role=dialog][aria-modal=true]").length===2')
  for lang in ['en','zh','ja']:
   js(f'groupWorkProbe.language("{lang}")')
   for dark in [False,True]:
    js(f'groupWorkProbe.setDark({str(dark).lower()})')
    for width in [1440,320]:
     size(width,1000 if width>640 else 844)
     overflow=js("[...document.querySelector('[role=dialog][aria-modal=true]:not([aria-labelledby])').querySelectorAll('button,input,textarea')].filter(e=>e.getBoundingClientRect().width>2).filter(e=>{const r=e.getBoundingClientRect();return r.left<0||r.right>innerWidth+1}).map(e=>e.textContent||e.type)")
     assert not overflow,(lang,dark,width,'profile editor',overflow)
     assert js("[...document.querySelectorAll('[role=dialog][aria-modal=true]:not([aria-labelledby]) .scrollbar-subtle')].every(e=>e.scrollWidth<=e.clientWidth+1)"),(lang,dark,width,'profile editor content overflow')
     if lang=='ja' and dark and width==320:shot('profile-editor-ja-mobile-125')
  click('[role=dialog][aria-modal=true]:not([aria-labelledby]) button')
  wait('document.querySelectorAll("[role=dialog][aria-modal=true]").length===1')
  # All shared settings switches retain a full hit target, visible keyboard focus,
  # and native Space activation; opening/navigating controls does not write settings.
  size(1440,1000);js('groupWorkProbe.language("en")')
  before=js('groupWorkProbe.requests.filter(r=>r.method!=="GET").length')
  for scope,tab in [('global','developer'),('group','assistants')]:
   js(f'groupWorkProbe.openSettings("{scope}","{tab}")');wait('!!document.querySelector("[aria-labelledby=settings-modal-title] input[role=switch]")')
   switches=js("[...document.querySelectorAll('[aria-labelledby=settings-modal-title] input[role=switch]')].map(e=>({width:e.getBoundingClientRect().width,height:e.getBoundingClientRect().height,disabled:e.disabled}))")
   assert all(e['width']>=44 and e['height']>=44 for e in switches),(tab,switches)
   js("document.querySelector('[aria-labelledby=settings-modal-title] input[role=switch]').focus()")
   key('Tab');key('Tab',8)
   assert js("document.activeElement.matches('input[role=switch]')"),tab
   assert js("getComputedStyle(document.activeElement.nextElementSibling).boxShadow!=='none'"),(tab,'missing focus')
   shot('settings-'+tab+'-keyboard-focus')
  assert js('groupWorkProbe.requests.filter(r=>r.method!=="GET").length')==before
  js('groupWorkProbe.openSettings("global","developer")');wait('!!document.querySelector("input[role=switch]")')
  js('document.querySelector("input[role=switch]").focus()');checked=js('document.activeElement.checked');key(' ')
  assert js('document.activeElement.checked')!=checked,'Space must toggle the native switch'
  key(' ');assert js('document.activeElement.checked')==checked
  key('Escape');wait('!document.querySelector("[role=dialog][aria-modal=true]")')
  out.joinpath('settings-matrix.json').write_text(json.dumps(settings_matrix,indent=2))
  js('groupWorkProbe.setTextScale(100);groupWorkProbe.setNotebookWarning("")')
  assert js('groupWorkProbe.errors')==[],js('groupWorkProbe.errors')
  out.joinpath('surfaces.json').write_text(json.dumps(surfaces,indent=2))
  out.joinpath('proof.json').write_text(json.dumps({'matrix':matrix,'errors':js('groupWorkProbe.errors'),'requests':js('groupWorkProbe.requests')},ensure_ascii=False,indent=2))
  print('PASS search lifecycle, settings matrix, all settings tabs, compact/read/switch/collapse Presentation, theme contrast and scaled notebook/files surfaces')
 except Exception:
  try:shot('failure');print(js('groupWorkProbe.errors'));print(js("[...document.querySelectorAll('[role=dialog]')].map(e=>({label:e.getAttribute('aria-labelledby'),modal:e.getAttribute('aria-modal'),text:e.textContent.slice(-200)}))"))
  except:pass
  raise
 finally:
  if sock:sock.close()
  browser.terminate();browser.wait(timeout=10)
