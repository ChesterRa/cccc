#!/usr/bin/env bash
# Requires Vite :5555 + ego-browser. Fake credentials/HTTP only; no provider API or microphone calls.
set -euo pipefail
ego-browser nodejs <<'JS'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC 外部ASR配置验收');
await openOrReuseTab('http://localhost:5555/ui/tests/browser/external-asr-settings.html',{wait:true});
await cdp('Emulation.setDeviceMetricsOverride',{width:1280,height:900,deviceScaleFactor:1,mobile:false});
await waitForElement('input[type="password"]');
await fillInput('input[type="password"]','test-bailian-key');
await click('xpath=//button[normalize-space()="保存供应商凭证"]');
await wait(0.3);
assert.equal(await js(`document.querySelector('input[type="password"]').value`),'');
let saved=await js(`window.externalAsrFixture.requests.filter(r=>r.method==='PUT').at(-1)`);
assert.ok(saved.url.endsWith('/bailian'));assert.equal(saved.body.api_key,'test-bailian-key');
await click('xpath=//button[normalize-space()="测试连接"]');
await wait(0.2);
assert.ok(await js(`document.querySelector('[role="status"]').textContent.includes('连接成功')`));
await click('button[aria-label="识别供应商"]');
await click('xpath=//*[@role="option" and normalize-space()="火山引擎豆包"]');
await wait(0.3);
assert.equal(await js(`document.querySelector('input[type="password"]').value`),'');
await click('button[aria-label="鉴权方式"]');
await click('xpath=//*[@role="option" and contains(normalize-space(),"App ID + Access Token")]');
await fillInput('xpath=//label[span[normalize-space()="App ID"]]/input','test-app');
await fillInput('xpath=//label[span[normalize-space()="Access Token"]]/input','test-access');
await click('xpath=//button[normalize-space()="保存供应商凭证"]');
await wait(0.3);
saved=await js(`window.externalAsrFixture.requests.filter(r=>r.method==='PUT').at(-1)`);
assert.ok(saved.url.endsWith('/volcengine'));assert.equal(saved.body.auth_mode,'app_token');assert.equal(saved.body.access_token,'test-access');
assert.ok(await js(`Array.from(document.querySelectorAll('input[type="password"]')).every(input=>input.value==='')`));
for (const width of [390,320]) {
  await cdp('Emulation.setDeviceMetricsOverride',{width,height:844,deviceScaleFactor:1,mobile:true});
  await cdp('Page.captureScreenshot',{format:'png'});
  const bounds=await js(`({width:document.documentElement.scrollWidth,viewport:innerWidth,inputs:[...document.querySelectorAll('input')].map(i=>i.getBoundingClientRect().height)})`);
  assert.ok(bounds.width<=bounds.viewport,JSON.stringify(bounds));assert.ok(bounds.inputs.every(height=>height>=44));
  cliLog({width,result:'PASS',bounds});
}
await js(`document.querySelector('main').lastElementChild.scrollIntoView({block:'end'})`);
await click('xpath=//button[normalize-space()="清除该供应商凭证"]');
await wait(0.3);
const providers=await js('window.externalAsrFixture.providers');
assert.equal(providers.find(p=>p.provider==='volcengine').configured,false);
assert.equal(providers.find(p=>p.provider==='bailian').configured,true);
cliLog({result:'PASS',checks:['provider switch','masked credentials','save','probe','clear','mobile layout'],boundary:'fixed HTTP responses; provider protocol and auth covered by Rust tests'});
JS

ego-browser nodejs <<'JS'
cliLog(await completeTaskSpace('CCCC 外部ASR配置验收',{keep:false}));
JS
