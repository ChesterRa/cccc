#!/usr/bin/env bash
# Real voice sheet with local fixture transport: checks layout and controls, not ASR quality.
set -euo pipefail
ego-browser nodejs <<'JS'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC 语音面板空间优化');
const base=process.env.CCCC_VOICE_WORKSPACE_URL || 'http://127.0.0.1:5190/ui/tests/browser/voice-workspace-mobile.html';
const out=process.env.CCCC_VOICE_WORKSPACE_OUTPUT || "/tmp/cccc-voice-workspace-mobile";
const fs=await import('node:fs');fs.mkdirSync(out,{recursive:true});
async function until(expression){for(let i=0;i<50;i++){if(await js(expression))return;await wait(.1);}throw Error(`Timed out: ${expression}`);}
async function tap(selector){const p=await js(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);await cdp('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[p]});await cdp('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await wait(.15);}
async function ready(theme,scale,extra=0){
 await gotoAndWait(`${base}?theme=${theme}&scale=${scale}&extra=${extra}`,{timeout:20});
 await cdp("Page.reload");
 await cdp('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
 await cdp('Emulation.setTouchEmulationEnabled',{enabled:true});
 await until(`document.querySelectorAll('[data-voice-mobile-sheet]').length===1 && !!document.querySelector('[data-voice-activity-item="last"]')`);
 await cdp('Page.captureScreenshot',{format:'png'});await wait(.6);
}
for(const theme of ['dark','light'])for(const scale of [100,125]){
 await ready(theme,scale);
 const state=await js(`(()=>{const r=s=>document.querySelector(s).getBoundingClientRect().toJSON();const last=r('[data-voice-activity-item="last"]'),scroll=r('[data-voice-activity-scroll]');return {last,scroll,sheet:r('[data-voice-mobile-sheet]'),body:r('[data-voice-workspace-body]'),device:['language','microphone'].map(k=>r('[data-voice-setting="'+k+'"] button')),refresh:r('[data-voice-refresh]'),closed:document.querySelector('.voice-mobile-prompt-toggle').getAttribute('aria-expanded'),description:getComputedStyle(document.querySelector('[data-voice-prompt-description]')).display,targets:[...document.querySelectorAll('[data-voice-mobile-sheet] button')].map(e=>e.getBoundingClientRect().toJSON()).filter(r=>r.width>0&&r.height>0)};})()`);
 assert.ok(state.last.top>=state.scroll.top&&state.last.bottom<=state.scroll.bottom-4,'last reply must be entirely visible');
 assert.ok(state.scroll.bottom<=state.sheet.bottom&&state.sheet.bottom<=844);
 assert.equal(state.device[0].top,state.device[1].top);assert.equal(state.device[0].top,state.refresh.top);
 for(const r of [...state.device,state.refresh])assert.ok(r.height>=44&&r.width>=44&&r.right<=390);
 assert.equal(state.closed,'false');assert.equal(state.description,'none');
 assert.ok(state.targets.every(r=>r.width>=44&&r.height>=44),'all visible controls need 44px targets');
 const shot=await cdp('Page.captureScreenshot',{format:'png'});fs.writeFileSync(`${out}/after-${theme}-${scale}.png`,Buffer.from(shot.data,'base64'));
 if(theme==='dark'&&scale===100)fs.writeFileSync(`${out}/after-390x844.png`,Buffer.from(shot.data,'base64'));
 await tap('.voice-mobile-prompt-toggle');assert.equal(await js(`getComputedStyle(document.querySelector('[data-voice-prompt-description]')).display!=='none'`),true);
 await tap('.voice-mobile-prompt-toggle');
 cliLog({theme,scale,result:'PASS',state});
}
// Exercise pending state transitions repeatedly through real controls.
await ready('dark',100);
for(let round=1;round<=10;round++){
 await tap('[data-voice-setting="language"] button');
 const value=round%2?'en-US':'mixed';
 await until(`!!document.querySelector('[role="option"][data-value="${value}"]')`);
 await tap(`[role="option"][data-value="${value}"]`);
 await until(`window.voiceWorkspaceProbe.languages.length===${round}`);
 const enumerations=await js('voiceWorkspaceProbe.enumerations');await tap('[data-voice-refresh]');
 await until(`window.voiceWorkspaceProbe.enumerations>${enumerations}`);
 if(round===1){
  await tap('[data-voice-setting="microphone"] button');
  await until(`!!document.querySelector('[role="option"][data-value="fixture-mic"]')`);
  await tap('[role="option"][data-value="fixture-mic"]');
 }
 await tap('[data-voice-record]');await until(`window.voiceWorkspaceProbe.starts===${round} && !document.querySelector('[data-voice-record]').disabled`);
 assert.equal(await js(`document.querySelector('[data-voice-setting="language"] button').disabled`),true);
 await tap('[data-voice-record]');await until(`window.voiceWorkspaceProbe.stops===${round} && !document.querySelector('[data-voice-setting="language"] button').disabled`);
 assert.ok(await js(`window.voiceWorkspaceProbe.devices.at(-1).includes('fixture-mic')`));
 cliLog({round,controls:'PASS'});
}
await ready('light',125,10);
const p=await js(`(()=>{const e=document.querySelector('[data-voice-activity-scroll]');const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2,scroll:e.scrollHeight,client:e.clientHeight}})()`);
assert.ok(p.scroll>p.client);
await cdp('Input.synthesizeScrollGesture',{x:p.x,y:p.y,yDistance:-180,gestureSourceType:'touch',speed:400});await wait(.3);
assert.ok(await js(`document.querySelector('[data-voice-activity-scroll]').scrollTop>0`));
assert.equal(await js(`document.querySelector('[data-voice-workspace-body]').scrollTop`),0);
await tap('[data-voice-sheet-close]');await until(`!document.querySelector('[data-voice-mobile-sheet]')`);
await cdp('Input.synthesizeScrollGesture',{x:195,y:400,yDistance:-180,gestureSourceType:'touch',speed:400});await wait(.3);
assert.ok(await js(`document.querySelector('[aria-label="Background messages"]').scrollTop>0`));
cliLog({independentActivityScroll:'PASS',backgroundChatScroll:'PASS'});
JS
