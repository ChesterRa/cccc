#!/usr/bin/env bash
# Read-only UI navigation; requires a Chinese CCCC UI, an existing group and ego lite.
set -euo pipefail
ego-browser nodejs <<'EOF'
import assert from 'node:assert/strict';
const pause=()=>new Promise(r=>setTimeout(r,400));
const task=await useOrCreateTaskSpace('CCCC 侧栏与设置手机检查'); cliLog({taskSpace:task.id});
await openOrReuseTab(process.env.CCCC_MOBILE_TEST_URL || 'http://localhost:5555/ui/',{wait:true,timeout:20});
await cdp('Emulation.setDeviceMetricsOverride',{width:375,height:667,deviceScaleFactor:1,mobile:true});
await cdp('Emulation.setTouchEmulationEnabled',{enabled:true});
await cdp('Page.captureScreenshot',{format:'png'});
await pause();
await waitForElement('button[aria-label="菜单"]',{timeout:20}); await pause();
if(await js(`!!document.querySelector('button[aria-label="关闭设置"]')`)){ await click('button[aria-label="关闭设置"]'); await pause(); }
if(await js(`(()=>{const e=document.querySelector('button[aria-label="关闭侧边栏"]');return e&&e.getBoundingClientRect().x>=0;})()`)) {
  await click('button[aria-label="关闭侧边栏"]'); await pause();
}
async function openSettings(){
  if (!(await js(`!!document.querySelector('[role="dialog"][aria-label="菜单"]')`))) await click('button[aria-label="菜单"]');
  await waitForElement('[role="dialog"][aria-label="菜单"]',{timeout:30});
  await pause();
  await cdp('Page.captureScreenshot',{format:'png'});
  await click('xpath=//div[@role="dialog" and @aria-label="菜单"]//button[normalize-space()="设置"]');
  await waitForElement('button[aria-label="关闭设置"]',{timeout:30});
  await pause();
  await js(`document.getAnimations().forEach(a=>{if(a.effect.getComputedTiming().iterations!==Infinity)a.finish()})`);
}
await openSettings();
const tabSelector='xpath=//div[@role="dialog"]//div[contains(@class,"touch-pan-x")]//button[normalize-space()="复制工作组"]';
const strip=await js(`document.querySelector('[role="dialog"] .touch-pan-x').getBoundingClientRect().toJSON()`);
await cdp('Input.synthesizeScrollGesture',{x:strip.right-30,y:strip.y+strip.height/2,xDistance:-850,gestureSourceType:'touch',speed:1800});
await click(tabSelector); await pause();
await click('button[aria-label="关闭设置"]'); await openSettings();
async function verify(label){
  const result=await js(`(()=>{
    const strip=document.querySelector('[role="dialog"] .touch-pan-x');
    const tab=[...strip.querySelectorAll('button')].find(e=>e.textContent.trim()==='复制工作组');
    const r=tab.getBoundingClientRect(), s=strip.getBoundingClientRect();
    return {left:r.left,right:r.right,stripLeft:s.left,stripRight:s.right,hit:tab.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2))};
  })()`);
  assert.ok(result.left>=result.stripLeft-1 && result.right<=result.stripRight+1 && result.hit, `${label}: active tab hidden ${JSON.stringify(result)}`);
  cliLog({label,result:'PASS',bounds:result});
}
await verify('reopen');
await cdp('Emulation.setDeviceMetricsOverride',{width:320,height:568,deviceScaleFactor:1,mobile:true});
await cdp('Page.captureScreenshot',{format:'png'}); await pause(); await verify('resize');
await click('xpath=//div[@role="dialog"]//button[normalize-space()="全局" and not(ancestor::aside)]'); await pause();
await click('xpath=//div[@role="dialog"]//button[normalize-space()="当前工作组" and not(ancestor::aside)]'); await pause();
await verify('scope switch');
await click('button[aria-label="关闭设置"]');
EOF

ego-browser nodejs <<'EOF'
cliLog(await completeTaskSpace('CCCC 侧栏与设置手机检查',{keep:false}));
EOF
