#!/usr/bin/env bash
# Run against the Vite dev UI. Uses real components/CSS; only the prompt submission is mocked.
set -euo pipefail
ego-browser nodejs <<'JS'
import assert from 'node:assert/strict';
const pause = () => new Promise(resolve => setTimeout(resolve, 400));
await useOrCreateTaskSpace('CCCC 390px 语音工具栏');
await openOrReuseTab(process.env.CCCC_MOBILE_TEST_URL || 'http://127.0.0.1:5190/ui/', {wait:true});
await cdp('Page.reload');
await waitForElement('textarea', {timeout:20});
await pause();
await js(`(()=>{document.querySelector('button[aria-label="Minimize panel — keep voice running"]')?.click(); const b=document.querySelector('button[aria-label="Group actions · cccc"],button[aria-label="工作组操作 · cccc"]');b?.parentElement.click();})()`);
await pause();
await waitForElement('.voice-mobile-only button', {timeout:20});
await js(`(()=>{const original=window.fetch.bind(window);window.__voiceMobileSubmissions=0;window.fetch=async(input,opts)=>{const u=typeof input==='string'?input:input.url;if(u.includes('/assistants/voice_secretary/inputs')){window.__voiceMobileSubmissions++;return new Response(JSON.stringify({ok:true,result:{}}),{headers:{'Content-Type':'application/json'}});}return original(input,opts);};})()`);
async function tap(selector) {
  const point = await js(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect();const p={x:r.x+r.width/2,y:r.y+r.height/2};if(!e.contains(document.elementFromPoint(p.x,p.y)))throw Error('Target is obscured: '+${JSON.stringify(selector)});return p;})()`);
  await cdp('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[point]});
  await cdp('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
  await pause();
}
await cdp('Emulation.setTouchEmulationEnabled',{enabled:true});
for(const lang of ['en','zh']) {
  await js(`import('/ui/src/i18n/index.ts').then(({default:i})=>i.changeLanguage(${JSON.stringify(lang)}))`);
  for(const [width,height] of [[390,844],[844,390],[1280,900]]) {
    await cdp('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:true});
    await cdp('Page.captureScreenshot',{format:'png'});await pause();
    await js(`document.querySelector('button[aria-label="Close sidebar"],button[aria-label="关闭侧边栏"]')?.click()`);await pause();
    const state=await js(`(()=>{const group=document.querySelector('.voice-mobile-only').parentElement;const r=group.getBoundingClientRect();const visible=[...group.querySelectorAll('button')].map(e=>({label:e.getAttribute('aria-label'),r:e.getBoundingClientRect().toJSON()})).filter(e=>e.r.width>0);return {r:r.toJSON(),visible,desktop:getComputedStyle(document.querySelector('.voice-desktop-controls')).display,mobile:getComputedStyle(document.querySelector('.voice-mobile-only')).display,bodyWidth:document.documentElement.scrollWidth};})()`);
    assert.ok(state.bodyWidth<=width,`body overflow ${lang} ${width}`);
    for(const b of state.visible)assert.ok(b.r.left>=state.r.left && b.r.right<=state.r.right+1,`control overflow: ${JSON.stringify(b)}`);
    if(width<640){
      assert.equal(state.visible.length,2,'mobile keeps microphone and options only');
      assert.equal(state.desktop,'none');
      for(const b of state.visible)assert.ok(b.r.width>=44&&b.r.height>=44,'44px touch target');
      await tap('.voice-mobile-only button');
      const menu=await js(`(()=>{const e=document.querySelector('.voice-mobile-menu');return {r:e.getBoundingClientRect().toJSON(),overflow:getComputedStyle(e).overflowY,buttons:[...e.querySelectorAll('button')].map(b=>({h:b.getBoundingClientRect().height,w:b.getBoundingClientRect().width})),background:getComputedStyle(e).backgroundColor};})()`);
      assert.ok(menu.r.left>=0&&menu.r.right<=width&&menu.r.top>=0&&menu.r.bottom<=height);
      assert.equal(menu.overflow,'auto');assert.ok(menu.buttons.length>4,'mode and language options available');
      for(const b of menu.buttons)assert.ok(b.h>=44&&b.w>=44);
      await tap('.voice-mobile-only button');
    } else {
      assert.equal(state.mobile,'none');assert.ok(state.visible.length>=4,'desktop actions retained');
    }
    cliLog({lang,width,height,result:'PASS',controls:state.visible});
  }
}
// Long English status is driven by the actual prompt action, not injected DOM.
await cdp('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
await cdp('Page.captureScreenshot',{format:'png'});await pause();
await js(`import('/ui/src/i18n/index.ts').then(async({default:i})=>{await i.changeLanguage('en');i.addResource('en','chat','voiceSecretaryPromptDraftWaitingShort','Polishing prompt while waiting for the assistant to finish processing this longer English instruction…');})`);
await js(`(()=>{const e=document.querySelector('textarea[aria-label]');window.__voiceOriginalDraft=e.value;Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(e,'Mobile layout regression');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);await pause();
await tap('.voice-mobile-only button');
await tap('.voice-mobile-menu button:has(svg.lucide-sparkles)');
const status=await js(`(()=>{const slot=document.querySelector('[data-voice-mobile-status-slot]');const s=slot.getBoundingClientRect();const t=document.querySelector('textarea[aria-label]').getBoundingClientRect();const b=document.querySelector('.voice-mobile-only').getBoundingClientRect();return {text:slot.textContent,slot:s.toJSON(),textarea:t.toJSON(),toolbar:b.toJSON(),submissions:window.__voiceMobileSubmissions,overflow:slot.scrollWidth>slot.clientWidth};})()`);
assert.equal(status.submissions,1,'prompt submission was intercepted');
assert.match(status.text,/Polishing prompt/);assert.equal(status.overflow,false);
assert.ok(status.slot.bottom<=status.textarea.top+1,'status is above the input');
assert.ok(status.slot.bottom<=status.toolbar.top,'status must not overlap controls');
cliLog({longStatus:'PASS',status});
for(const [width,height] of [[844,390],[1280,900]]) {
  await cdp('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:true});
  await cdp('Page.captureScreenshot',{format:'png'});await pause();
  const bounds=await js(`(()=>{const row=document.querySelector('[data-composer-action-bar]');const r=row.getBoundingClientRect();return {width:row.clientWidth,scroll:row.scrollWidth,body:document.documentElement.scrollWidth,right:r.right,controls:[...row.querySelectorAll('button')].map(b=>b.getBoundingClientRect().toJSON()).filter(r=>r.width>0)};})()`);
  assert.ok(bounds.scroll<=bounds.width+1 && bounds.body<=width && bounds.right<=width && bounds.controls.every(r=>r.right<=bounds.right && r.left>=0),'long pending status overflows '+JSON.stringify(bounds));
  cliLog({longStatus:'PASS',width,height,bounds});
}

await js(`(()=>{const e=document.querySelector('textarea[aria-label]');Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(e,window.__voiceOriginalDraft);e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
await cdp('Page.reload');
cliLog('PASS: mobile, landscape, desktop, Chinese/English, long pending status; no backend prompt sent');
JS
