#!/usr/bin/env python3
"""Opt-in synthetic Codex Voice burst probe; never opens a microphone or Analyst.

Requires requests, websocket-client, and Chrome/Chromium. Example:
CCCC_CODEX_VOICE_RESULTS_LIVE=1 PROBE_TIMING=mid_turn python3 scripts/tests/probe_codex_voice_results.py
This uses the host Codex ChatGPT account and consumes a short Realtime call.
"""
import json
import sys
import shutil
import os
import re
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

if os.environ.get("CCCC_CODEX_VOICE_RESULTS_LIVE") != "1":
    print("Skipped: set CCCC_CODEX_VOICE_RESULTS_LIVE=1 to use the host account.")
    sys.exit(0)
try:
    import requests
    import websocket
except ImportError as error:
    raise SystemExit("This optional probe requires requests and websocket-client.") from error
chrome = os.environ.get("CCCC_VOICE_CHROME_EXECUTABLE") or shutil.which("google-chrome") or shutil.which("chromium")
if not chrome:
    raise SystemExit("Chrome/Chromium is required for this optional WebRTC probe.")

# Isolated synthetic call: no microphone, repository data, or Analyst process.
profile = tempfile.TemporaryDirectory(prefix="cccc-voice-probe-browser-")
browser = subprocess.Popen([
    chrome, "--headless=new", "--no-sandbox",
    "--remote-debugging-port=0", "--remote-allow-origins=*",
    "--autoplay-policy=no-user-gesture-required",
    "--user-data-dir=" + profile.name, "about:blank",
], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
sock = None
try:
    port_file = Path(profile.name) / "DevToolsActivePort"
    deadline = time.monotonic() + 15
    while not port_file.exists():
        if time.monotonic() > deadline:
            raise RuntimeError("isolated Chrome did not start")
        time.sleep(0.1)
    port = port_file.read_text().splitlines()[0]
    tab = requests.put(f"http://127.0.0.1:{port}/json/new?about:blank", timeout=5).json()
    sock = websocket.create_connection(tab["webSocketDebuggerUrl"], timeout=35)
    sequence = 0

    def evaluate(expression):
        global sequence
        sequence += 1
        sock.send(json.dumps({"id": sequence, "method": "Runtime.evaluate", "params": {
            "expression": expression, "awaitPromise": True, "returnByValue": True,
        }}))
        while True:
            packet = json.loads(sock.recv())
            if packet.get("id") == sequence:
                if "error" in packet:
                    raise RuntimeError(str(packet["error"]))
                if "exceptionDetails" in packet.get("result", {}):
                    raise RuntimeError("browser evaluation failed: " + str(packet["result"]["exceptionDetails"]))
                return packet["result"]["result"].get("value")

    offer = evaluate("""(async () => {
      window.events = [];
      window.pc = new RTCPeerConnection();
      const ac = window.ac = new AudioContext();
      const oscillator = ac.createOscillator();
      const gain = ac.createGain(); gain.gain.value = 0;
      const destination = ac.createMediaStreamDestination();
      oscillator.connect(gain); gain.connect(destination); oscillator.start();
      for (const track of destination.stream.getTracks()) pc.addTrack(track, destination.stream);
      window.dc = pc.createDataChannel('oai-events');
      dc.onmessage = e => { try { events.push(JSON.parse(e.data)); } catch {} };
      await pc.setLocalDescription(await pc.createOffer());
      return pc.localDescription.sdp;
    })()""")
    auth_path = Path(os.environ.get("CCCC_CODEX_AUTH_PATH", str(Path(os.environ.get("CODEX_HOME", str(Path.home()/".codex"))) / "auth.json")))
    auth = json.loads(auth_path.read_text())["tokens"]
    result = requests.post("https://chatgpt.com/backend-api/codex/realtime/calls?intent=quicksilver&architecture=avas", headers={
        "Authorization": "Bearer " + auth["access_token"],
        "chatgpt-account-id": auth["account_id"], "originator": "cccc",
        "x-session-id": str(uuid.uuid4()), "user-agent": "cccc/0.4.37",
        "openai-alpha": "quicksilver=v2",
    }, json={"sdp": offer, "session": {
        "model": "gpt-live-1-codex", "audio": {"output": {"voice": "cove"}},
        "instructions": "This is a synthetic delivery test. Briefly acknowledge each new speakable fact, including its exact number. Do not use tools.",
        "delegation": {"type": "client", "ack_filler": True},
    }}, timeout=30)
    print("provider_http_status", result.status_code, flush=True)
    if not 200 <= result.status_code < 300:
        raise RuntimeError("provider rejected isolated test call")
    answer = result.text
    if answer.startswith('{'):
        payload = result.json()
        answer = payload.get('sdp', payload.get('answer', ''))
    evaluate("pc.setRemoteDescription(" + json.dumps({"type": "answer", "sdp": answer}) + ")")
    deadline = time.monotonic() + 20
    while evaluate("dc.readyState") != "open":
        if time.monotonic() > deadline:
            raise RuntimeError("WebRTC data channel did not open")
        time.sleep(0.2)
    commands = [{"type": "session.context.append", "event_id": f"probe-{n}", "channel": "speakable", "content": [{"type": "input_text", "text": f"New independent Analyst result: checkpoint {n} is verified."}]} for n in range(1, 7)]
    mid_turn = os.environ.get("PROBE_TIMING") == "mid_turn"
    evaluate("for (const c of " + json.dumps(commands[:1] if mid_turn else commands) + ") dc.send(JSON.stringify(c)); true")
    if mid_turn:
        deadline = time.monotonic() + 15
        while not evaluate("events.some(e => e.type === 'turn.created' && e.turn?.role === 'assistant')"):
            if time.monotonic() > deadline:
                raise RuntimeError("no first assistant turn")
            time.sleep(0.1)
        evaluate("for (const c of " + json.dumps(commands[1:]) + ") dc.send(JSON.stringify(c)); true")
        print("five_results_sent_during_first_speech", flush=True)
    print("sent_six_results", flush=True)
    observed = []
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        for event in evaluate("events.splice(0)"):
            observed.append(event)
            if event.get("type") in ["session.context.appended", "delegation.context.appended", "turn.created", "turn.done", "error"]:
                print(json.dumps(event, ensure_ascii=False), flush=True)
        time.sleep(0.5)
    acks = sum(e.get("type") == "session.context.appended" for e in observed)
    transcripts = " ".join(e.get("turn", {}).get("transcript", "") for e in observed if e.get("type") == "turn.done")
    assert acks == 6, f"expected 6 context receipts, got {acks}"
    words = ["one", "two", "three", "four", "five", "six"]
    normalized = transcripts.lower()
    for number, word in enumerate(words, 1):
        normalized = re.sub(r"\b" + word + r"\b", str(number), normalized)
    complete_range = re.search(r"\b1\s*(?:through|to|[-–—])\s*6\b", normalized)
    missing = [] if complete_range else [str(n) for n in range(1, 7)
               if not re.search(r"\b" + str(n) + r"\b", normalized)]
    assert not missing, f"synthetic checkpoints absent from transcript: {missing}"
    print("PASS: six receipts and all six synthetic checkpoints in spoken transcript", flush=True)
finally:
    if sock:
        try:
            evaluate("pc.close(); ac.close(); true")
        except Exception:
            pass
        try:
            sock.send(json.dumps({"id": 99999, "method": "Browser.close"}))
        except Exception:
            pass
        sock.close()
    if browser.poll() is None:
        browser.terminate()
    try:
        browser.wait(timeout=5)
    except subprocess.TimeoutExpired:
        browser.kill()
        browser.wait(timeout=5)
    for attempt in range(5):
        try:
            profile.cleanup()
            break
        except OSError:
            if attempt == 4:
                raise
            time.sleep(0.2)
