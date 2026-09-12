#!/usr/bin/env bash
# Same real components and deterministic fixture before/after; transport remains mocked.
set -euo pipefail
phase=${1:-after}
case "$phase" in before|after) ;; *) exit 2 ;; esac
ego-browser nodejs <<JS
const phase = "$phase";
const assert = (await import('node:assert/strict')).default;
await useOrCreateTaskSpace('CCCC 语音面板三模式');
const fs = await import('node:fs');
const out = '/tmp/cccc-voice-workspace-modes';
fs.mkdirSync(out, {recursive:true});
const records = [];
async function ready(mode, theme, scale, width) {
  await cdp('Emulation.setDeviceMetricsOverride', {width,height:844,deviceScaleFactor:1,mobile:width<640});
  await cdp('Emulation.setTouchEmulationEnabled', {enabled:width<640});
  await gotoAndWait('http://127.0.0.1:5555/ui/tests/browser/voice-workspace-mobile.html?mode='+mode+'&theme='+theme+'&scale='+scale, {timeout:20});
  await cdp('Page.reload');
  for(let i=0;i<100;i++) {
    if(await js("document.querySelectorAll('[data-voice-mobile-sheet]').length===1 && !!document.querySelector('[data-voice-document-path], [data-voice-activity-item]') && (document.querySelector('[data-voice-document-panel]') ? !!document.querySelector('h1') : true)")) break;
    if(i===99) throw Error('Fixture did not render: '+mode);
    await wait(.1);
  }
  await js("(()=>{const s=document.createElement('style');s.textContent='*,*::before,*::after{animation:none!important;transition:none!important;caret-color:transparent!important}';document.head.append(s)})()");
  await cdp('Page.captureScreenshot',{format:'png'});
  await wait(.5);
}
for(const width of [390,640,1024,1440]) for(const mode of ['document','instruction','prompt']) for(const theme of ['dark','light']) for(const scale of width===390?[100,125]:[100]) {
  await ready(mode,theme,scale,width);
  const state = await js("(()=>{const q=s=>document.querySelector(s),r=s=>{const e=q(s);return e&&{width:e.clientWidth,scrollWidth:e.scrollWidth,height:e.clientHeight,scrollHeight:e.scrollHeight,...e.getBoundingClientRect().toJSON()}};return {heading:r('[data-voice-document-heading]'),path:r('[data-voice-document-path]'),surface:r('[data-voice-document-panel]>div:last-child'),activity:r('[data-voice-activity-scroll]'),body:r('[data-voice-workspace-body]'),sheet:r('[data-voice-mobile-sheet]'),h1:q('h1')&&getComputedStyle(q('h1')).fontSize,targets:[...q('[data-voice-mobile-sheet]').querySelectorAll('button')].map(e=>e.getBoundingClientRect().toJSON()).filter(r=>r.width>0&&r.height>0)}})()");
  const key=[mode,width,theme,scale].join('-');
  const cssSelector = 'style[data-vite-dev-id$="/voiceWorkspaceMobile.css"]';
  const currentCss = await js('document.querySelector('+JSON.stringify(cssSelector)+').textContent');
  if(phase==='before') fs.writeFileSync(out+'/before.css',currentCss);
  const shot = await cdp('Page.captureScreenshot',{format:'png'});
  fs.writeFileSync(out+'/'+phase+'-'+key+'.png',Buffer.from(shot.data,'base64'));
  const failures = [];
  if(width===390) {
    if(!state.targets.every(r=>r.width>=44&&r.height>=44)) failures.push('44px touch targets');
    if(mode==='document') {
      if(state.heading.scrollWidth>Math.ceil(state.heading.width)) failures.push('heading overflow');
      if(state.path.width<190||state.path.scrollWidth>Math.ceil(state.path.width)) failures.push('readable path');
      if(parseFloat(state.h1)>25) failures.push('mobile heading font');
      if(state.surface.bottom>state.sheet.bottom-8) failures.push('document bottom safe space');
    } else {
      if(state.activity.height<300) failures.push('activity needs 300px');
      if(state.activity.bottom>state.sheet.bottom-8) failures.push('activity bottom safe space');
    }
  }
  if(phase==='after'&&width>=640) {
    // Isolate compositor edge noise by comparing old/new CSS in the same render.
    // TSX changes in this task are inert data attributes only; keep original screenshots too.
    const beforeCss=fs.readFileSync(out+'/before.css','utf8');
    for(const [label,css] of [['stable-before',beforeCss],['stable-after',currentCss]]) {
      await js('document.querySelector('+JSON.stringify(cssSelector)+').textContent='+JSON.stringify(css));
      await cdp('Page.captureScreenshot',{format:'png'});await wait(.3);
      const stable=await cdp('Page.captureScreenshot',{format:'png'});
      fs.writeFileSync(out+'/'+label+'-'+key+'.png',Buffer.from(stable.data,'base64'));
    }
  }
  records.push({key,state,failures});
  if(phase==='after') assert.deepEqual(failures,[],key);
  cliLog({phase,key,state});
}
fs.writeFileSync(out+'/'+phase+'.json',JSON.stringify(records,null,2));
JS
