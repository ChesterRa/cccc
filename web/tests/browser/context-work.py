#!/usr/bin/env python3
"""Real Context modal, synthetic APIs. Use the isolated Vite server from group-work.py.
Set CCCC_GROUP_WORK_BASE_URL and optionally CCCC_CONTEXT_WORK_OUTPUT_DIR.
Requires Chrome, requests and websocket-client; never attaches to user browsers.
"""
import base64,json,os,subprocess,tempfile,time
from pathlib import Path
import requests,websocket
phase='verified'
out=Path(os.environ.get('CCCC_CONTEXT_WORK_OUTPUT_DIR','/tmp/cccc-context-work'));out.mkdir(parents=True,exist_ok=True)
base=os.environ.get('CCCC_GROUP_WORK_BASE_URL','http://127.0.0.1:15559').rstrip('/')
with tempfile.TemporaryDirectory(prefix='cccc-context-browser-') as profile:
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
  def click_key(key):
   js("[...document.querySelectorAll('button')].find(b=>b.textContent.trim()===contextWorkProbe.t("+json.dumps(key)+")).click()")
   time.sleep(.15)
  def value(selector,text):
   js("(()=>{const e=document.querySelector("+json.dumps(selector)+");Object.getOwnPropertyDescriptor(e.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype,'value').set.call(e,"+json.dumps(text)+");e.dispatchEvent(new Event('input',{bubbles:true}));})()")
   time.sleep(.15)
  results=[]
  for theme in ['light','dark']:
   for lang in ['en','zh','ja']:
    for width in [1440,390,320]:
     cdp('Emulation.setDeviceMetricsOverride',{'width':width,'height':1000,'deviceScaleFactor':1,'mobile':width<640})
     cdp('Page.navigate',{'url':base+'/ui/tests/browser/context-work.html?theme='+theme+'&lang='+lang+'&scale='+('125' if width==320 else '100')+'&data='+('full' if width==390 else 'sparse')})
     wait("!!window.contextWorkProbe && document.body.innerText.includes('T001')")
     box=js("(()=>{const e=document.querySelector('[role=dialog]');const r=e.getBoundingClientRect();return {left:r.left,right:r.right,scroll:e.scrollWidth,client:e.clientWidth}})()")
     assert box['left']>=0 and box['right']<=width+.5 and box['scroll']<=box['client']+1,(theme,lang,width,box)
     assert js("document.querySelectorAll('[data-task-column]').length") == 3
     # The original selected filter had white-on-white text. Check real computed contrast.
     ratio=js("""(()=>{const e=[...document.querySelectorAll('button[aria-pressed=true]')].find(b=>b.textContent.includes(' · '));const s=getComputedStyle(e);const lum=c=>c.match(/[0-9.]+/g).slice(0,3).map(Number).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4).reduce((a,v,i)=>a+v*[.2126,.7152,.0722][i],0);const a=lum(s.color),b=lum(s.backgroundColor);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05)})()""")
     assert ratio>=4.5,(theme,ratio)
     # Field edits survive canceled navigation; a deliberate discard restores the original.
     click_key('editButton')
     value('[role=dialog] input','Unsaved objective')
     js('window.confirm=()=>false')
     click_key('agents')
     click_key('coordination')
     assert js("document.querySelector('[role=dialog] input').value")=='Unsaved objective'
     click_key('cancel')
     click_key('agents')
     wait("document.body.innerText.includes('agent-4')")
     assert js("[...document.querySelectorAll('article details')].every(e=>!e.open)")
     js("document.querySelector('article summary').focus()")
     press('Enter')
     assert js("document.querySelector('article details').open")
     assert js("document.querySelector('article details').innerText.includes('ownership')")
     assert js("document.querySelector('article').innerText.includes('Waiting for the review result')")
     press('Enter')
     assert not js("document.querySelector('article details').open")
     if lang=='en':shot(theme+'-'+str(width)+'-agents')
     click_key('coordination')
     click_key('projectMd');wait("document.body.innerText.includes('Repository reference')")
     click_key('brief')
     if lang=='en':shot(theme+'-'+str(width)+'-coordination')
     # Reading, expanding and canceling must never write shared state.
     assert js("contextWorkProbe.requests.every(r=>r.method==='GET')")
     assert js('contextWorkProbe.errors')==[],js('contextWorkProbe.errors')
     results.append({'theme':theme,'lang':lang,'width':width,'contrast':ratio})
  out.joinpath('results.json').write_text(json.dumps(results,indent=2))
  print(json.dumps({'passed':len(results),'output':str(out)}))
  sock.close()
 finally:
  p.terminate();p.wait(timeout=10)
