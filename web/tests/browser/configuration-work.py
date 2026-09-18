#!/usr/bin/env python3
"""Configuration dialogs, synthetic APIs. Use the isolated Vite server from group-work.py.
Set CCCC_GROUP_WORK_BASE_URL and optionally CCCC_CONFIGURATION_WORK_OUTPUT_DIR.
Requires Chrome, requests and websocket-client; never attaches to user browsers.
"""
import base64,json,os,subprocess,tempfile,time
from pathlib import Path
import requests,websocket
phase='verified'
out=Path(os.environ.get('CCCC_CONFIGURATION_WORK_OUTPUT_DIR','/tmp/cccc-configuration-work'));out.mkdir(parents=True,exist_ok=True)
base=os.environ.get('CCCC_GROUP_WORK_BASE_URL','http://127.0.0.1:15559').rstrip('/')
with tempfile.TemporaryDirectory(prefix='cccc-configuration-work-browser-') as profile:
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
    cdp('Input.dispatchKeyEvent',{'type':kind,'key':key,'code':key,'windowsVirtualKeyCode':{'Enter':13,'Tab':9,'Escape':27,'End':35}[key],**({'text':'\r'} if key=='Enter' and kind=='keyDown' else {})})
   time.sleep(.1)
  results=[]
  for theme in ['light','dark']:
   for width,lang in [(1440,'en'),(390,'zh'),(320,'ja')]:
    for surface in ['create-group','edit-group','create-actor','edit-actor']:
     cdp('Emulation.setDeviceMetricsOverride',{'width':width,'height':1000,'deviceScaleFactor':1,'mobile':width<640})
     cdp('Page.navigate',{'url':base+'/ui/tests/browser/configuration-work.html?surface='+surface+'&theme='+theme+'&lang='+lang+'&scale='+('125' if width==320 else '100')})
     wait("!!document.querySelector('#open-configuration')")
     js("document.querySelector('#open-configuration').focus()")
     press('Enter')
     wait("!!document.querySelector('[role=dialog][aria-modal=true]')")
     time.sleep(.4)
     if surface=='create-group':
      assert js("document.activeElement.tagName")=='INPUT'
      assert js("document.activeElement.value")=='/workspace/project'
     box=js("(()=>{const e=document.querySelector('[role=dialog][aria-modal=true]');const r=e.getBoundingClientRect();const b=e.querySelector(':scope > .overflow-y-auto');return {left:r.left,right:r.right,scroll:e.scrollWidth,client:e.clientWidth,background:getComputedStyle(b).backgroundImage}})()")
     assert box['left']>=-.5 and box['right']<=width+.5 and box['scroll']<=box['client']+1,(theme,width,surface,box)
     assert box['background']=='none',box
     overflow=js("[...document.querySelectorAll('[role=dialog] button,[role=dialog] input,[role=dialog] select')].filter(e=>e.getBoundingClientRect().width>1&&!e.closest('[role=tablist]')).filter(e=>{const r=e.getBoundingClientRect();return r.left<-.5||r.right>innerWidth+.5}).map(e=>e.textContent||e.getAttribute('placeholder'))")
     if overflow: shot('overflow-'+theme+'-'+surface+'-'+str(width))
     assert not overflow,(theme,width,surface,overflow)
     if 'actor' in surface:
      js("document.querySelector('[role=tab][aria-selected=true]').focus()")
      press('End')
      wait("document.activeElement.matches('[role=tab][aria-selected=true]:last-child')")
      time.sleep(.25)
      assert js("(()=>{const r=document.activeElement.getBoundingClientRect();return r.left>=-.5&&r.right<=innerWidth+.5})()"),(theme,width,surface,js("({active:document.activeElement.outerHTML,box:document.activeElement.getBoundingClientRect().toJSON(),scroll:document.activeElement.parentElement.parentElement.scrollLeft})"))
     for _ in range(5):
      press('Tab')
      assert js("!!document.activeElement.closest('[role=dialog]')")
     shot(theme+'-'+surface+'-'+str(width))
     press('Escape')
     wait("!document.querySelector('[role=dialog][aria-modal=true]')")
     assert js("document.activeElement.id")=='open-configuration',(theme,width,surface,js("document.activeElement.outerHTML.slice(0,250)"))
     assert js('configurationWorkProbe.actions')==[]
     assert js('configurationWorkProbe.errors')==[],js('configurationWorkProbe.errors')
     results.append({'theme':theme,'surface':surface,'width':width,'lang':lang})
  out.joinpath('results.json').write_text(json.dumps(results,indent=2))
  print(json.dumps({'passed':len(results),'output':str(out)}))
  sock.close()
 finally:
  p.terminate();p.wait(timeout=10)
