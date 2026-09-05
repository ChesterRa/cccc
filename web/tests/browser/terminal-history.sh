#!/usr/bin/env bash
# Real browser/layout + stubbed HTTP page responses; ANSI rendering is tested in Rust.
set -euo pipefail
ego-browser nodejs <<'EOF'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC 审查修复验收');
for (const [width,height] of [[1280,900],[375,667],[320,568]]) {
  await openOrReuseTab('http://localhost:5555/ui/tests/browser/terminal-history.html', {wait:true});
  await gotoAndWait('http://localhost:5555/ui/tests/browser/terminal-history.html');
  await cdp('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:width<600});
  await click('button');
  await wait(0.5);
  assert.equal(await js('window.historyRequests.length'),1,'short pages must not automatically scan the archive');
  assert.equal(await js('document.querySelector("pre").textContent'),'new frame');
  await click('xpath=//button[normalize-space()="Load older history"]');
  await wait(0.3);
  const result = await js(`(() => {
    const e=document.querySelector('[aria-labelledby="terminal-history-title"]');const r=e.getBoundingClientRect();
    return {text:e.querySelector('pre').textContent, requests:window.historyRequests, top:r.top,bottom:r.bottom,width:document.documentElement.scrollWidth};
  })()`);
  assert.equal(result.text,'old frame\n\nnew frame');
  assert.equal(result.requests.length,2);
  assert.ok(result.requests[1].includes('before=15') && result.requests[1].includes('render_before=40'));
  assert.ok(result.top>=0 && result.bottom<=height+1 && result.width<=width,JSON.stringify(result));
  await pressKey('Escape');
  await wait(0.1);
  assert.equal(await js('!!document.querySelector("[aria-labelledby=terminal-history-title]")'),false);
  assert.equal(await js('document.activeElement.textContent'),'Open history');
  cliLog({width,height,result:'PASS'});
}
EOF

# Close the task space only after the preceding assertions pass.
ego-browser nodejs <<'EOF'
cliLog(await completeTaskSpace('CCCC 审查修复验收', { keep: false }));
EOF
