#!/usr/bin/env bash
# Real UI with injected Web Speech failures and silent synthetic media.
# The recording lease/storage are isolated; no real microphone or transcription is used.
set -euo pipefail
ego-browser nodejs <<'EOF'
import assert from 'node:assert/strict';
await useOrCreateTaskSpace('CCCC 语音重复启停排查');
await openOrReuseTab('http://127.0.0.1:5555/ui/',{wait:true,timeout:20});
await waitForElement('button[aria-label="开始语音听写"]',{timeout:30});
if(await js(`!!document.querySelector('[role="dialog"] button[aria-label="关闭语音小秘书"]')`)) await click('[role="dialog"] button[aria-label="关闭语音小秘书"]');
await js(String.raw`(() => {
  if(window.__voiceRecoveryTest) return;
  const state=window.__voiceRecoveryTest={starts:0,aborts:0,mode:'network',tracks:[],instances:[]};
  const storage=new Map();
  for(const method of ['getItem','setItem','removeItem']) {
    const original=Storage.prototype[method];
    Storage.prototype[method]=function(key,value){
      if(key!=='cccc.voiceSecretary.activeCapture')return original.call(this,key,value);
      if(method==='getItem')return storage.get(key)||null;
      if(method==='setItem')storage.set(key,value);else storage.delete(key);
    };
  }
  const OriginalChannel=window.BroadcastChannel;
  window.BroadcastChannel=class extends OriginalChannel {
    constructor(name){super(name==='cccc.voiceSecretary.capture'?name+'.recovery-test':name)}
  };
  const originalFetch=window.fetch.bind(window);
  window.fetch=async (input,options)=>{
    const url=typeof input==='string'?input:input.url;
    if(url.includes('/assistants/voice_secretary/recording_lease')) {
      const body=JSON.parse(options.body);state.leaseAction=body.action;
      return new Response(JSON.stringify({ok:true,result:{lease_id:'recovery-test',lost:false,lease:{owner_id:body.owner_id,group_id:url.split('/')[4],group_title:'Recovery test'}}}),{headers:{'Content-Type':'application/json'}});
    }
    if(url.includes('/assistants/voice_secretary/') && options?.method && options.method!=='GET')throw new Error('Unexpected voice mutation during test');
    return originalFetch(input,options);
  };
  navigator.mediaDevices.getUserMedia=async ()=>{
    const context=new AudioContext();state.audioContext=context;
    const stream=context.createMediaStreamDestination().stream;
    state.tracks.push(...stream.getTracks());return stream;
  };
  navigator.mediaDevices.enumerateDevices=async ()=>[];
  class Recognition {
    constructor(){state.instances.push(this)}
    start(){state.starts++;this.timer=setTimeout(()=>{if(state.mode!=='empty')this.onerror?.({error:state.mode});this.onend?.()},20)}
    abort(){state.aborts++;clearTimeout(this.timer)}
    stop(){clearTimeout(this.timer);setTimeout(()=>this.onend?.(),0)}
  }
  window.SpeechRecognition=Recognition;window.webkitSpeechRecognition=Recognition;
})()`);
for(const mode of ['network','audio-capture','empty']) {
  await js(`Object.assign(window.__voiceRecoveryTest,{mode:${JSON.stringify(mode)},starts:0,aborts:0})`);
  await click('button[aria-label="开始语音听写"]');
  const deadline=Date.now()+30_000;
  let state;
  do {
    await new Promise(r=>setTimeout(r,300));
    state=await js(`({starts:window.__voiceRecoveryTest.starts,stopped:!!document.querySelector('button[aria-label="开始语音听写"]'),lease:window.__voiceRecoveryTest.leaseAction,tracksEnded:window.__voiceRecoveryTest.tracks.every(t=>t.readyState==='ended')})`);
    if(state.starts>=8&&state.stopped)break;
  } while(Date.now()<deadline);
  assert.equal(state.starts,8,`${mode}: retry budget ${JSON.stringify(state)}`);
  assert.equal(state.stopped,true,`${mode}: UI did not stop`);
  assert.equal(state.lease,'release',`${mode}: recording lease not released`);
  assert.equal(state.tracksEnded,true,'synthetic microphone probe not cleaned up');
  await new Promise(r=>setTimeout(r,1200));
  assert.equal(await js(`window.__voiceRecoveryTest.starts`),8,'recognition restarted after stopping');
  cliLog({mode,result:'PASS',state});
}
await js(`window.__voiceRecoveryTest.audioContext?.close()`);
EOF

ego-browser nodejs <<'EOF'
cliLog(await completeTaskSpace('CCCC 语音重复启停排查',{keep:false}));
EOF
