#!/usr/bin/env bash
# Requires Vite :5555 and ego-browser. Only HTTP is mocked; production UI and API client run.
set -euo pipefail
ego-browser nodejs <<'JS'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('语音设置自动保存验收');
await openOrReuseTab('http://localhost:5555/ui/tests/browser/voice-settings-autosave.html',{wait:true});
await cdp('Page.reload');
await waitForElement('button[aria-label="识别位置"]');
assert.equal(await js(`window.voiceSettingsFixture.writes.length`),0);
assert.equal(await js(`document.body.innerText.includes('保存识别设置')`),false);
await click('button[aria-label="识别位置"]');
await click('xpath=//*[@role="option" and contains(normalize-space(),"外部服务 ASR")]');
await waitForElement('button[aria-label="识别供应商"]');
assert.equal(await js(`window.voiceSettingsFixture.assistant.config.recognition_backend`),'external_provider_asr');
await click('button[aria-label="识别供应商"]');
await click('xpath=//*[@role="option" and normalize-space()="火山引擎豆包"]');
await wait(0.3);
assert.equal(await js(`window.voiceSettingsFixture.assistant.config.external_asr_provider`),'volcengine');
await fillInput('input[type="number"]','45');
await pressKey('Enter');
await wait(0.3);
assert.equal(await js(`window.voiceSettingsFixture.assistant.config.auto_document_max_window_seconds`),45);
await click('xpath=//label[.//input[@role="switch"] and contains(normalize-space(),"录音中自动更新文档")]');
await wait(0.3);
assert.equal(await js(`window.voiceSettingsFixture.assistant.config.auto_document_max_window_seconds`),null);
assert.ok(await js(`window.voiceSettingsFixture.writes.every(write=>!('enabled' in write))`));
await js(`window.voiceSettingsFixture.failNext=true; document.querySelector('button[aria-label="识别位置"]').scrollIntoView({block:'center'})`);
await wait(0.3);
await click('button[aria-label="识别位置"]');
await waitForElement('[role="option"]');
await click('xpath=//*[@role="option" and contains(normalize-space(),"浏览器 ASR")]');
await wait(0.3);
assert.ok(await js(`document.body.innerText.includes('测试保存失败')`));
assert.ok(await js(`document.querySelector('button[aria-label="识别位置"]').textContent.includes('外部服务 ASR')`));
assert.equal(await js(`window.voiceSettingsFixture.assistant.config.recognition_backend`),'external_provider_asr');
cliLog({result:'PASS',checks:['no save button','no save on load','backend/provider autosave','interval Enter save','switch autosave','no actor enable in config requests','failed save rollback']});
JS

ego-browser nodejs <<'JS'
cliLog(await completeTaskSpace('语音设置自动保存验收',{keep:false}));
JS
