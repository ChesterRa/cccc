#!/usr/bin/env bash
# Requires the dev server and ego-browser. Cancels row drags without reordering.
set -euo pipefail
ego-browser nodejs <<'EOF'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC 审查修复验收');
await openOrReuseTab(process.env.CCCC_MENU_TEST_URL || 'http://localhost:5555/ui/', { wait: true, timeout: 20 });
await cdp('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
await wait(1);
const selector = '[role="button"] button[aria-haspopup="menu"]';
await hover(selector);
const pos = await js(`(() => {const e=document.querySelector('${selector}'); const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
await js(`(() => {
  window.__dragAnnouncements = [];
  window.__dragObserver = new MutationObserver(records => {
    for (const r of records) {
      const e = r.target.nodeType === Node.TEXT_NODE ? r.target.parentElement : r.target;
      if (e.closest?.('[aria-live]')) window.__dragAnnouncements.push(e.textContent);
    }
  });
  window.__dragObserver.observe(document.body, {subtree:true, childList:true, characterData:true});
})()`);
const move = async (type, x, y, pressed) => cdp('Input.dispatchMouseEvent', {type,x,y,button:type==='mouseMoved'?'none':'left',buttons:pressed?1:0,clickCount:1});
await move('mousePressed',pos.x,pos.y,true);
await move('mouseMoved',pos.x+12,pos.y+6,true);
await wait(0.2);
const announcements = await js('window.__dragAnnouncements');
// Cancel before releasing even on regression, so a failed check cannot reorder groups.
await pressKey('Escape');
await move('mouseReleased',pos.x+12,pos.y+6,false);
assert.equal(announcements.some(x=>/Draggable item|picked up|dragging|已抓取|拖动|拖拽/i.test(x)),false,JSON.stringify(announcements));
await click(selector);
assert.equal(await js(`document.querySelector('${selector}').getAttribute('aria-expanded')`),'true');
await pressKey('Escape');
// Positive control: the row itself must still activate the real mouse sensor.
await js('window.__dragAnnouncements = []');
await move('mousePressed',pos.x-90,pos.y,true);
await move('mouseMoved',pos.x-78,pos.y+6,true);
await wait(0.2);
const rowAnnouncements = await js('window.__dragAnnouncements');
await pressKey('Escape');
await move('mouseReleased',pos.x-78,pos.y+6,false);
assert.ok(rowAnnouncements.some(x=>/Draggable item|picked up/i.test(x)),JSON.stringify(rowAnnouncements));
await js('window.__dragObserver.disconnect()');
cliLog({result:'PASS',menuAnnouncements:announcements,rowAnnouncements});
EOF

# Close the task space only after the preceding assertions pass.
ego-browser nodejs <<'EOF'
cliLog(await completeTaskSpace('CCCC 审查修复验收', { keep: false }));
EOF
