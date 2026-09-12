#!/usr/bin/env bash
set -euo pipefail
ego-browser nodejs <<'JS'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC 外观下拉验收');
const base=process.env.CCCC_APPEARANCE_TEST_URL || 'http://127.0.0.1:5190/ui/tests/browser/appearance.html';
async function tap(selector) {
 const point=await js(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
 await cdp('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[point]});
 await cdp('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await wait(.35);
}
for(const host of ['desktop','sheet'])for(const theme of ['light','dark']) {
 await openOrReuseTab(`${base}?host=${host}&theme=${theme}`,{wait:true});
 await cdp('Page.reload');
 await cdp('Emulation.setDeviceMetricsOverride',{width:host==='sheet'?390:1280,height:844,deviceScaleFactor:1,mobile:host==='sheet'});
 await cdp('Emulation.setTouchEmulationEnabled',{enabled:true});await wait(.5);
 if(host==='desktop')await tap('[data-app-settings-trigger]');
 const labels=await js(`Array.from(document.querySelectorAll('[data-appearance-preferences] button')).map(e=>e.getAttribute('aria-label'))`);
 assert.equal(labels.length,3);
 for(let i=0;i<3;i++) {
   const selector=`[data-appearance-preferences] > div:nth-of-type(${i+1}) button[aria-haspopup="menu"]`;
   await tap(selector);await cdp('Page.captureScreenshot',{format:'png'});await wait(.4);
   const state=await js(`(()=>{const menu=document.querySelector('[role="menu"]');const r=menu.getBoundingClientRect();return {rect:r.toJSON(),bg:getComputedStyle(menu).backgroundColor,z:getComputedStyle(menu).zIndex,checked:menu.querySelectorAll('[aria-checked="true"]').length,items:[...menu.querySelectorAll('[role="menuitemradio"]')].map(e=>({text:e.textContent,h:e.getBoundingClientRect().height})),hit:menu.contains(document.elementFromPoint(r.x+r.width/2,r.y+22)),width:innerWidth,height:innerHeight};})()`);
   assert.equal(state.items.length,[3,4,3][i]);assert.equal(state.checked,1);
   assert.ok(state.items.every(x=>x.h>=44));assert.ok(state.hit,'menu must be above its host');
   assert.ok(state.rect.x>=0&&state.rect.right<=state.width&&state.rect.top>=0&&state.rect.bottom<=state.height,'no clipping');
   assert.notEqual(state.bg,'rgba(0, 0, 0, 0)');
   // Pick the current theme to keep both color cases stable; change text size and language.
   await tap(i===0?'[role="menu"] [aria-checked="true"]':'[role="menu"] [role="menuitemradio"]:last-child');
   const after=await js(`({inner:!!document.querySelector('[role="menuitemradio"]'),outer:!!document.querySelector(${JSON.stringify(host==='desktop'?'[data-app-settings-menu]':'.mobile-menu-panel')}),focus:document.activeElement?.getAttribute('aria-haspopup'),calls:window.appearanceCalls})`);
   assert.equal(after.inner,false);assert.equal(after.outer,true);assert.equal(after.focus,'menu');assert.ok(!after.calls.includes('close'));
   // Escape must not escape into the sheet/document listener.
   await tap(selector);await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await wait(.35);
   const esc=await js(`({inner:!!document.querySelector('[role="menuitemradio"]'),outer:!!document.querySelector(${JSON.stringify(host==='desktop'?'[data-app-settings-menu]':'.mobile-menu-panel')}),focus:document.activeElement?.getAttribute('aria-haspopup')})`);
   assert.equal(esc.inner,false);assert.equal(esc.outer,true);assert.equal(esc.focus,'menu');
   cliLog({host,theme,index:i,result:'PASS',state});
 }
}
JS
