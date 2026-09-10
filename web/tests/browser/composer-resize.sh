#!/usr/bin/env bash
# Real AppShell/ChatComposer, existing isolated group-work transport fixture; no backend writes.
set -euo pipefail
ego-browser nodejs <<'JS'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC 输入区拖拽验收');
await openOrReuseTab(process.env.CCCC_COMPOSER_RESIZE_URL || 'http://127.0.0.1:5190/ui/tests/browser/composer-resize.html',{wait:true});
await cdp('Page.reload');await waitForElement('textarea:not(.xterm-helper-textarea)',{timeout:20});await wait(.6);
const textarea='textarea:not(.xterm-helper-textarea)';
const handle='footer [role="separator"][aria-orientation="horizontal"]';
const pause=async()=>{await cdp('Page.captureScreenshot',{format:'png'});await wait(.3);};
async function draft(text){
 await js(`(()=>{const e=document.querySelector(${JSON.stringify(textarea)});e.focus();e.select();})()`);
 await cdp('Input.insertText',{text});await pause();
}
async function dimensions(width,height){await cdp('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:width<768});await pause();}
async function key(name){await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:name,code:name});await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:name,code:name});await pause();}
await js(`(()=>{window.__heightWrites=0;const f=Storage.prototype.setItem;Storage.prototype.setItem=function(k,v){if(k==='cccc-composer-height')window.__heightWrites++;return f.call(this,k,v);};})()`);
for(const dark of [false,true])for(const scale of [100,125]) {
 await dimensions(1280,900);
 await js(`groupWorkProbe.setDark(${dark});groupWorkProbe.setTextScale(${scale});groupWorkProbe.ui.getState().setComposerHeight(64);`);await pause();
 await draft(Array(35).fill('Multiline input height regression').join('\n'));
 const start=await js(`(()=>{const e=document.querySelector(${JSON.stringify(handle)}),r=e.getBoundingClientRect(),t=document.querySelector(${JSON.stringify(textarea)});window.__heightWrites=0;return {x:r.x+r.width/2,y:r.y+r.height/2,min:+e.getAttribute('aria-valuemin'),height:t.getBoundingClientRect().height};})()`);
 assert.ok(Math.abs(start.height-64*scale/100)<1);
 await cdp('Input.dispatchMouseEvent',{type:'mousePressed',x:start.x,y:start.y,button:'left',buttons:1,clickCount:1});
 for(let i=1;i<=5;i++)await cdp('Input.dispatchMouseEvent',{type:'mouseMoved',x:start.x,y:start.y-i*40,button:'left',buttons:1});
 const preview=await js(`({writes:window.__heightWrites,stored:groupWorkProbe.ui.getState().composerHeight,height:document.querySelector(${JSON.stringify(textarea)}).getBoundingClientRect().height})`);
 assert.equal(preview.writes,0,'no per-frame persistence');assert.equal(preview.stored,64,'no per-frame store writes');assert.ok(preview.height>start.height+100,'drag grows a long draft');
 await cdp('Input.dispatchMouseEvent',{type:'mouseReleased',x:start.x,y:start.y-200,button:'left',clickCount:1});await pause();
 const committed=await js(`(()=>{const t=document.querySelector(${JSON.stringify(textarea)}),footer=t.closest('footer'),panel=footer.parentElement;return {stored:groupWorkProbe.ui.getState().composerHeight,persisted:localStorage.getItem('cccc-composer-height'),writes:window.__heightWrites,messageHeight:panel.querySelector(':scope > main').getBoundingClientRect().height,cap:+document.querySelector(${JSON.stringify(handle)}).getAttribute('aria-valuenow'),height:t.getBoundingClientRect().height};})()`);
 assert.equal(committed.writes,1);assert.equal(+committed.persisted,committed.stored);assert.ok(committed.messageHeight>=80*scale/100);assert.ok(committed.height<=committed.cap+1);
 await draft('');
 assert.ok(Math.abs(await js(`document.querySelector(${JSON.stringify(textarea)}).getBoundingClientRect().height`)-64*scale/100)<1,'empty draft uses the original minimum');
 await draft('a\nb\nc\nd\ne\nf');
 const grown=await js(`document.querySelector(${JSON.stringify(textarea)}).getBoundingClientRect().height`);
 assert.ok(grown>64*scale/100&&grown<committed.cap,'content still grows naturally below the chosen cap');
 await js(`document.querySelector(${JSON.stringify(handle)}).focus()`);await key('End');
 await dimensions(1280,450);
 const smaller=await js(`(()=>{const h=document.querySelector(${JSON.stringify(handle)}),t=document.querySelector(${JSON.stringify(textarea)});return {max:+h.getAttribute('aria-valuemax'),now:+h.getAttribute('aria-valuenow'),height:t.getBoundingClientRect().height};})()`);
 assert.ok(smaller.max<committed.cap);assert.ok(smaller.now<=smaller.max&&smaller.height<=smaller.max+1);
 await js(`document.querySelector(${JSON.stringify(handle)}).focus()`);await key('Home');assert.equal(await js('groupWorkProbe.ui.getState().composerHeight'),64);
 await key('ArrowUp');assert.equal(await js('groupWorkProbe.ui.getState().composerHeight'),80);
 await key('ArrowDown');assert.equal(await js('groupWorkProbe.ui.getState().composerHeight'),64);
 await dimensions(1280,900);await js(`document.querySelector(${JSON.stringify(handle)}).focus()`);await key('End');
 const p=await js(`(()=>{const r=document.querySelector(${JSON.stringify(handle)}).getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
 for(let clickCount=1;clickCount<=2;clickCount++){await cdp('Input.dispatchMouseEvent',{type:'mousePressed',...p,button:'left',buttons:1,clickCount});await cdp('Input.dispatchMouseEvent',{type:'mouseReleased',...p,button:'left',clickCount});}await pause();
 assert.equal(await js('groupWorkProbe.ui.getState().composerHeight'),64,'double click resets');
 cliLog({dark,scale,result:'PASS',preview,committed,smaller});
}
await draft('');await dimensions(390,844);await cdp('Emulation.setTouchEmulationEnabled',{enabled:true});await pause();
assert.equal(await js(`document.querySelectorAll(${JSON.stringify(handle)}).length`),0,'no mobile handle in the DOM');
const point=await js(`(()=>{let e=document.querySelector('[data-message-row]');while(e&&!(e.scrollHeight>e.clientHeight&&/auto|scroll/.test(getComputedStyle(e).overflowY)))e=e.parentElement;if(!e)throw Error('missing message scroller');window.__messages=e;const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+60,before:e.scrollTop,height:e.clientHeight};})()`);
await cdp('Input.synthesizeScrollGesture',{x:point.x,y:point.y,yDistance:150,gestureSourceType:'touch',speed:400});await pause();
const after=await js(`window.__messages.scrollTop`);assert.ok(after<point.before,'mobile touch must still scroll to earlier messages: '+JSON.stringify({before:point.before,after}));
cliLog({mobileScroll:'PASS',before:point.before,after,handleCount:0});
JS
