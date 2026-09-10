#!/usr/bin/env bash
set -euo pipefail
ego-browser nodejs <<'JS'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC 语音面板三模式');
const fs=await import('node:fs');
const out='/tmp/cccc-voice-workspace-modes';
async function until(expression) {for(let i=0;i<60;i++){if(await js(expression))return;await wait(.1);}throw Error(expression);}
async function tap(selector) {
 const p=await js(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});e.scrollIntoView({block:'nearest'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
 await cdp('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[p]});
 await cdp('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await wait(.1);
}
async function ready(mode,theme='light',scale=125,extra='') {
 await cdp('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
 await cdp('Emulation.setTouchEmulationEnabled',{enabled:true});
 await gotoAndWait('http://127.0.0.1:5555/ui/tests/browser/voice-workspace-mobile.html?mode='+mode+'&theme='+theme+'&scale='+scale+extra,{timeout:20});
 await cdp('Page.reload');
 await until(`document.querySelectorAll('[data-voice-mobile-sheet]').length===1 && !!document.querySelector('[data-voice-document-path], [data-voice-activity-item]')`);
 await wait(.5);
}
async function scroll(selector) {
 const p=await js(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:Math.min(r.bottom-20,780),height:e.clientHeight,scroll:e.scrollHeight}})()`);
 assert.ok(p.scroll>p.height,'fixture must have scrollable content');
 await cdp('Input.synthesizeScrollGesture',{x:p.x,y:p.y,yDistance:-160,gestureSourceType:'touch',speed:400});await wait(.3);
 assert.ok(await js('document.querySelector('+JSON.stringify(selector)+').scrollTop>0'));
}
for(const theme of ['dark','light']) {
 await ready('document',theme,125,'&long=1');await until("!!document.querySelector('h1')");
 const state=await js(`(()=>{const q=s=>document.querySelector(s),r=s=>{const e=q(s);return {width:e.clientWidth,scrollWidth:e.scrollWidth,...e.getBoundingClientRect().toJSON()}};return {heading:r('[data-voice-document-heading]'),path:r('[data-voice-document-path]'),surface:r('[data-voice-document-panel]>div:last-child'),sheet:r('[data-voice-mobile-sheet]')}})()`);
 assert.ok(state.heading.scrollWidth<=Math.ceil(state.heading.width));
 assert.ok(state.path.scrollWidth<=Math.ceil(state.path.width)&&state.path.width>=190);
 assert.ok(state.surface.bottom<=state.sheet.bottom-8,'long title must leave readable document space');
 const shot=await cdp('Page.captureScreenshot',{format:'png'});fs.writeFileSync(out+'/after-document-long-'+theme+'.png',Buffer.from(shot.data,'base64'));
 await scroll('[data-voice-document-panel]>div:last-child');
 assert.equal(await js("document.querySelector('[data-voice-workspace-body]').scrollTop"),0);
 cliLog({theme,longDocument:'PASS',state});
}
await ready('document');await until("!!document.querySelector('h1')");
for(let round=1;round<=10;round++) {
 await tap('[data-voice-document-actions] button:last-child');
 await until("!!document.querySelector('[data-voice-document-panel] textarea')");
 const value=await js("document.querySelector('[data-voice-document-panel] textarea').value");
 assert.ok(value.includes('# 会议记录与工作计划'));
 await tap('[data-voice-document-actions] button:last-child');
 await until("!!document.querySelector('[data-voice-document-panel] h1')");
 await tap('[data-voice-document-views] button:last-child');
 await until("document.querySelector('[data-voice-document-views] button:last-child').getAttribute('aria-pressed')==='true'");
 await tap('[data-voice-document-views] button:first-child');
 await until("!!document.querySelector('[data-voice-document-panel] h1')");
 cliLog({round,editPreviewAndTranscript:'PASS'});
}
await ready('instruction','light',125,'&extra=20');
await scroll('[data-voice-activity-scroll]');
assert.equal(await js("document.querySelector('[data-voice-workspace-body]').scrollTop"),0);
await fillInput('[data-voice-instruction-input]','请整理本次讨论的待办事项。');
assert.equal(await js("document.querySelector('[data-voice-instruction-send]').disabled"),false);
const input=await js("(()=>{const e=document.querySelector('[data-voice-instruction-input]');return {height:e.clientHeight,value:e.value}})()");
assert.ok(input.height>=44&&input.height<=90);assert.ok(input.value.includes('待办事项'));
await tap('[data-voice-instruction-send]');
await until("document.querySelector('[data-voice-instruction-input]').value===''");
await until("document.querySelector('[data-voice-instruction-send]').disabled");
await tap('[data-voice-sheet-close]');await until("!document.querySelector('[data-voice-mobile-sheet]')");
await scroll('[aria-label="Background messages"]');
cliLog({instructionInputAndSend:'PASS',instructionActivityScroll:'PASS',backgroundScroll:'PASS'});
JS
