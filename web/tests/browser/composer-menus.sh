#!/usr/bin/env bash
# Requires ego lite, a Chinese CCCC UI and an existing group. Only edits a temporary draft; never sends it.
set -euo pipefail
ego-browser nodejs <<'EOF'
import assert from 'node:assert/strict';
const pause = () => new Promise(resolve => setTimeout(resolve, 350));
const task = await useOrCreateTaskSpace('CCCC 输入候选回归');
cliLog({taskSpace:task.id});
await openOrReuseTab(process.env.CCCC_MOBILE_TEST_URL || 'http://localhost:5555/ui/', {wait:true,timeout:20});
await cdp('Page.reload');
await pause();
await cdp('Emulation.setDeviceMetricsOverride',{width:390,height:400,deviceScaleFactor:1,mobile:true});
await cdp('Emulation.setTouchEmulationEnabled',{enabled:true});
await waitForElement('textarea[aria-label="消息输入框"]',{timeout:20});
await pause();
if (await js(`(()=>{const e=document.querySelector('button[aria-label="关闭侧边栏"]'); return e && e.getBoundingClientRect().x>=0;})()`)) {
  await click('button[aria-label="关闭侧边栏"]'); await pause();
}
const original = await js(`document.querySelector('textarea[aria-label="消息输入框"]').value`);
async function replaceDraft(text) {
  await click('textarea[aria-label="消息输入框"]');
  await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',modifiers:4,commands:['selectAll']});
  await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',modifiers:4});
  if (text) await cdp('Input.insertText',{text});
  else {
    await cdp('Input.dispatchKeyEvent',{type:'keyDown',key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8});
    await cdp('Input.dispatchKeyEvent',{type:'keyUp',key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8});
  }
  await pause();
}
try {
  for (const trigger of ['#','/']) {
    await replaceDraft(trigger);
    for (const height of [400,300,400]) {
      await cdp('Emulation.setDeviceMetricsOverride',{width:390,height,deviceScaleFactor:1,mobile:true});
      // Force a rendered frame: background Chromium tabs can defer viewport events.
      await cdp('Page.captureScreenshot',{format:'png'});
      for (let retry=0; retry<20; retry++) {
        const ready=await js(`visualViewport.height === ${height} && parseFloat(document.documentElement.style.getPropertyValue('--app-viewport-height')) === ${height}`);
        if (ready) break;
        await pause();
      }
      assert.equal(await js(`parseFloat(document.documentElement.style.getPropertyValue('--app-viewport-height'))`), height, 'app must finish resizing before layout assertions');
      await pause();
      const bounds = await js(`(()=>{const e=document.querySelector('[role="listbox"]'); const r=e.getBoundingClientRect(); const first=e.querySelector('button').getBoundingClientRect(); return {top:r.top,bottom:r.bottom,firstTop:first.top,height:r.height};})()`);
      assert.ok(bounds.top>=7 && bounds.bottom<=height, `${trigger} menu clipped: ${JSON.stringify(bounds)}`);
      assert.ok(bounds.height>40 && bounds.firstTop>=7, 'first suggestion must remain visible');
      cliLog({trigger,height,result:'PASS',bounds});
    }
    const point = await js(`(()=>{const r=document.querySelector('[role="listbox"] button').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
    await cdp('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[point]});
    await cdp('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    await pause();
    const value=await js(`document.querySelector('textarea[aria-label="消息输入框"]').value`);
    assert.ok(value.startsWith(trigger) && value.length>1, 'touch must insert the chosen suggestion');
    cliLog({trigger,touchSelection:'PASS'});
  }
} finally {
  await replaceDraft(original);
}
EOF

ego-browser nodejs <<'EOF'
cliLog(await completeTaskSpace('CCCC 输入候选回归',{keep:false}));
EOF
