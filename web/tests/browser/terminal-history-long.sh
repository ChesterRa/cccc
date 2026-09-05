#!/usr/bin/env bash
# Real history panel; fixed HTTP responses. Rust tests cover raw ANSI rendering.
set -euo pipefail
ego-browser nodejs <<'EOF'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC 长历史回归验收');
await openOrReuseTab('http://localhost:5555/ui/tests/browser/terminal-history.html',{wait:true});
await gotoAndWait('http://localhost:5555/ui/tests/browser/terminal-history.html');
await js(`(() => {
  window.historyRequests = [];
  window.fetch = async (input) => {
    const url = String(input);
    if (!url.includes('/history-fixture/terminal/history?')) throw new Error('unexpected request');
    window.historyRequests.push(url);
    const older = new URL(url, location.origin).searchParams.has('before');
    return Response.json({ok:true,result:{text:Array.from({length:older?6000:1500},(_,i)=>'line '+(i+(older?0:4500))).join('\\n'),start_cursor:older?0:4500,end_cursor:6000,has_more:!older,cursor_expired:false,hint:''}});
  };
})()`);
await click('button');
await wait(0.5);
assert.equal(await js(`document.querySelector('pre').textContent.split('\\n').length`),1500);
// Wait for initial scroll anchoring, then use a real wheel gesture in the panel.
for (let attempt=0;attempt<30;attempt++) {
  if (await js(`document.querySelector('.overflow-auto').scrollTop > 160`)) break;
  await wait(0.1);
}
const position=await js(`(() => {const r=document.querySelector('.overflow-auto').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
await cdp('Input.synthesizeScrollGesture',{...position,yDistance:40000,speed:100000,gestureSourceType:'mouse'});
for (let attempt=0;attempt<30;attempt++) {
  if (await js(`window.historyRequests.length === 2`)) break;
  await wait(0.1);
}
await wait(0.2);
const result=await js(`(() => {const pre=document.querySelector('pre');const scroller=document.querySelector('.overflow-auto');return {lines:pre.textContent.split('\\n').length,older:pre.textContent.startsWith('line 0\\n'),middle:pre.textContent.includes('\\nline 5000\\n'),newest:pre.textContent.endsWith('line 5999'),requests:window.historyRequests.length,scrollable:scroller.scrollHeight>scroller.clientHeight};})()`);
assert.deepEqual(result,{lines:6000,older:true,middle:true,newest:true,requests:2,scrollable:true});
cliLog({result:'PASS',...result,httpBoundary:'fixed responses; raw rendering is covered by Rust tests'});
await pressKey('Escape');
EOF

ego-browser nodejs <<'EOF'
cliLog(await completeTaskSpace('CCCC 长历史回归验收',{keep:false}));
EOF
