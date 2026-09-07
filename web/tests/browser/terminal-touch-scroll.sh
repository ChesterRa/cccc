#!/usr/bin/env bash
# Requires the Vite dev server and ego-browser. Uses real xterm, no Actor/backend writes.
set -euo pipefail
ego-browser nodejs <<'JS'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC X10 触摸滚动回归');
await openOrReuseTab('http://localhost:5555/ui/tests/browser/terminal-touch-scroll.html',{wait:true});
await cdp('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
await cdp('Emulation.setTouchEmulationEnabled',{enabled:true});
await waitForElement('#terminal[data-ready="true"]');
for (const [mode, alternate] of [['none',false],['x10',false],['vt200',false],['drag',false],['any',false],['none',true],['x10',true]]) {
  await js(`window.touchScrollFixture.configure(${JSON.stringify(mode)},${alternate})`);
  const before=await js('window.touchScrollFixture.snapshot()');
  assert.equal(before.mode,mode);
  assert.equal(before.buffer,alternate?'alternate':'normal');
  const start=before.point;
  const end={x:start.x,y:start.y+before.cellHeight*3};
  await cdp('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[start]});
  await cdp('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[end]});
  await cdp('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
  await wait(0.1);
  const after=await js('window.touchScrollFixture.snapshot()');
  if (!alternate && (mode==='none' || mode==='x10')) {
    assert.ok(before.base>3,'fixture must have real scrollback');
    assert.equal(after.viewport,before.viewport-3,'touch must scroll older local history');
    assert.deepEqual(after.wheels,[],'local scroll must not synthesize wheel events');
    assert.deepEqual(after.input,[],'local history scrolling must not send input');
  } else {
    assert.equal(after.viewport,before.viewport,'application scrolling must leave local history unchanged');
    assert.deepEqual(after.wheels,[-1,-1,-1]);
    if (alternate) {
      assert.equal(after.input.join(''),'\x1bOA'.repeat(3),'alternate buffer must receive arrow-key scrolling');
    } else {
      assert.equal(after.input.length,3);
      assert.ok(after.input.every(data=>/^\x1b\[<64;\d+;\d+M$/.test(data)),'mouse protocols must receive real SGR wheel reports');
    }
  }
  cliLog({mode,buffer:after.buffer,result:'PASS',viewportBefore:before.viewport,viewportAfter:after.viewport,reports:after.input.length});
}
JS

ego-browser nodejs <<'JS'
cliLog(await completeTaskSpace('CCCC X10 触摸滚动回归',{keep:false}));
JS
