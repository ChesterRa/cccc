#!/usr/bin/env bash
# Requires the CCCC dev server (default :5555) and ego lite. Never sends messages or changes runtime state.
set -euo pipefail
ego-browser nodejs <<'EOF'
import assert from 'node:assert/strict';
const task = await useOrCreateTaskSpace('CCCC 手机菜单回归');
cliLog({ taskSpace: task.id });
await openOrReuseTab(process.env.CCCC_MOBILE_TEST_URL || 'http://localhost:5555/ui/', { wait: true, timeout: 20 });
await wait(1);
if (await js(`!!document.querySelector('[role="dialog"][aria-label="菜单"]')`)) { await click('button[aria-label="关闭菜单"]'); await wait(0.3); }
await cdp('Emulation.setTouchEmulationEnabled', { enabled: true });
const originalFontSize = await js(`document.documentElement.style.fontSize`);
for (const [width, height, largeText] of [[375,667,false], [320,568,false], [667,375,false], [390,844,false], [320,568,true]]) {
  await cdp('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: true });
  await wait(1);
  const sidebar = await js(`(() => { const e=document.querySelector('button[aria-label="关闭侧边栏"]'); return e && e.getBoundingClientRect().x >= 0; })()`);
  if (sidebar) { await click('button[aria-label="关闭侧边栏"]'); await wait(0.4); }
  await click('button[aria-label="菜单"]');
  await wait(1);
  if (largeText) await js(`(() => { document.documentElement.style.fontSize='125%'; document.querySelector('[role="dialog"][aria-label="菜单"] .text-lg').textContent='超长工作组名称：手机端交互与跨项目协作检查'.repeat(4); })()`);
  await js(`document.getAnimations().forEach(a => { if (a.effect.getComputedTiming().iterations !== Infinity) a.finish(); })`);
  const bounds = await js(`(() => {
    const panel = document.querySelector('[role="dialog"][aria-label="菜单"]');
    const close = panel.querySelector('button[aria-label="关闭菜单"]');
    const rect = panel.getBoundingClientRect();
    const cr = close.getBoundingClientRect();
    return { top: rect.top, bottom: rect.bottom, closeTop: cr.top, closeBottom: cr.bottom, width: document.documentElement.scrollWidth, viewport: innerWidth };
  })()`);
  assert.ok(bounds.top >= 0 && bounds.bottom <= height + 1, `${width}x${height}: menu outside viewport ${JSON.stringify(bounds)}`);
  assert.ok(bounds.closeTop >= 0 && bounds.closeBottom <= height, 'close button must stay reachable');
  assert.ok(bounds.width <= bounds.viewport, 'no horizontal page overflow');
  // A real touch gesture must reveal the final runtime controls without moving the close button.
  for (let swipe = 0; swipe < 3; swipe++) {
    await cdp('Input.synthesizeScrollGesture', { x: width / 2, y: height - 60, yDistance: -Math.min(400, height - 160), gestureSourceType: 'touch', speed: 1800 });
  }
  await wait(0.3);
  const last = await js(`(() => {
    const panel = document.querySelector('[role="dialog"][aria-label="菜单"]');
    const buttons = panel.querySelectorAll('button');
    const e = buttons[buttons.length-1]; const r = e.getBoundingClientRect();
    return { top:r.top, bottom:r.bottom, hit:e.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)) };
  })()`);
  assert.ok(last.top >= 0 && last.bottom <= height && last.hit, `runtime controls unreachable: ${JSON.stringify(last)}`);
  await click('button[aria-label="关闭菜单"]');
  await wait(0.2);
  assert.equal(await js(`!!document.querySelector('[role="dialog"][aria-label="菜单"]')`), false);
  cliLog({ width, height, largeText, result: 'PASS', bounds, last });
}
await js(`document.documentElement.style.fontSize = ${JSON.stringify(originalFontSize)}`);
EOF

# Cleanup only after the preceding run confirms all assertions passed.
ego-browser nodejs <<'EOF'
cliLog(await completeTaskSpace('CCCC 手机菜单回归', { keep: false }));
EOF
