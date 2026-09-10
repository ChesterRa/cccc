#!/usr/bin/env python3
"""Opt-in synthetic Codex Voice burst probe; never opens a microphone or Analyst.

Requires requests, websocket-client, and Chrome/Chromium. Example:
CCCC_CODEX_VOICE_RESULTS_LIVE=1 PROBE_TIMING=mid_turn python3 scripts/tests/probe_codex_voice_results.py
Use PROBE_TIMING=summary for one complete summary, or after_turn for two
summaries separated by the first completed assistant turn.
Set PROBE_SCHEDULER_URL=http://127.0.0.1:5557 with an isolated Web Vite server
and PROBE_TIMING=after_turn to exercise the actual browser output scheduler.
Optionally supply PROBE_INTERRUPT_WAV with a synthetic spoken WAV fixture to
test interruption. No microphone is used; all received audio is played muted.
PROBE_INCLUDE_SYNTHETIC_TRANSCRIPT=1 prints only this synthetic call's transcript.
Each run uses the host Codex ChatGPT account and consumes a short Realtime call.
Reports separate receipts, transcript facts, and received audio, not human hearing.
Imports and runs without the opt-in flag never use credentials or start a browser.
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
import base64
from urllib.parse import urlparse


FACTS = (
    ("amber", 17),
    ("birch", 29),
    ("coral", 43),
    ("delta", 58),
    ("elm", 61),
    ("frost", 76),
)
NUMBER_WORDS = (
    "seventeen",
    "twenty nine",
    "forty three",
    "fifty eight",
    "sixty one",
    "seventy six",
)


def probe_commands(timing):
    if timing not in ("burst", "mid_turn", "summary", "after_turn"):
        raise ValueError("PROBE_TIMING must be burst, mid_turn, summary, or after_turn")
    facts = [f"The {name} result is {number}." for name, number in FACTS]
    texts = [" ".join(facts)] if timing == "summary" else facts
    if timing == "after_turn":
        texts = [facts[0], " ".join(facts[1:])]
    return [
        {
            "type": "session.context.append",
            "event_id": f"probe-{n}",
            "channel": "speakable",
            "content": [{"type": "input_text", "text": text}],
        }
        for n, text in enumerate(texts, 1)
    ]


def protocol_observation(event):
    """Inspect correlation fields, never print provider content or error text."""
    observation = {"type": event.get("type"), "fields": sorted(event)}
    for field in ("event_id", "client_event_id", "delegation_item_id"):
        value = event.get(field)
        if isinstance(value, str) and re.fullmatch(r"[a-zA-Z0-9_.:/-]{1,128}", value):
            observation[field] = value
    turn = event.get("turn")
    if isinstance(turn, dict):
        observation["turn_role"] = turn.get("role")
        observation["turn_fields"] = sorted(turn)
    error = event.get("error")
    if isinstance(error, dict):
        for field in ("code", "type", "event_id", "param"):
            value = error.get(field)
            if isinstance(value, str) and re.fullmatch(
                r"[a-zA-Z0-9_.:/-]{1,128}", value
            ):
                observation[f"error_{field}"] = value
    return observation


def summarize_probe(events, commands, audio):
    receipts = [e for e in events if e.get("type") == "session.context.appended"]
    done = [
        e["turn"]
        for e in events
        if e.get("type") == "turn.done"
        and isinstance(e.get("turn"), dict)
        and e["turn"].get("role") == "assistant"
    ]
    texts = [
        t["transcript"].lower().replace("-", " ")
        for t in done
        if isinstance(t.get("transcript"), str)
    ]
    for word, (_, number) in zip(NUMBER_WORDS, FACTS):
        texts = [
            re.sub(r"\b" + word.replace(" ", r"\s+") + r"\b", str(number), text)
            for text in texts
        ]
    missing = [
        name
        for name, number in FACTS
        if not any(
            re.search(rf"\b{name}\b[^.!?;\n]{{0,64}}\b{number}\b", text)
            for text in texts
        )
    ]
    sent_ids = {command["event_id"] for command in commands}
    echoed = sorted(
        {
            e[field]
            for e in receipts
            for field in ("event_id", "client_event_id")
            if isinstance(e.get(field), str) and e[field] in sent_ids
        }
    )
    errors = [protocol_observation(e) for e in events if e.get("type") == "error"]
    return {
        "commands_sent": len(commands),
        "context_receipts": len(receipts),
        "echoed_client_ids": echoed,
        "assistant_turns_completed": len(done),
        "synthetic_facts_absent_from_completed_transcripts": missing,
        "audio_bytes_received": audio.get("bytes", 0),
        "audio_energy_received": audio.get("energy", 0),
        "decoded_audio_power_sum": audio.get("decoded_power", 0),
        "audio_context_state": audio.get("context_state"),
        "provider_errors": errors,
        "observed_delivery": (
            not missing
            and not errors
            and len(receipts) == len(commands)
            and audio.get("bytes", 0) > 0
            and (audio.get("energy", 0) > 0 or audio.get("decoded_power", 0) > 0)
        ),
        "per_source_speech_confirmed": False,
        "human_heard_confirmed": False,
        "user_interruption_tested": False,
    }


def main():
    if os.environ.get("CCCC_CODEX_VOICE_RESULTS_LIVE") != "1":
        print("Skipped: set CCCC_CODEX_VOICE_RESULTS_LIVE=1 to use the host account.")
        return 0
    timing = os.environ.get("PROBE_TIMING", "burst")
    commands = probe_commands(
        timing
    )  # Reject invalid scenarios before opening a paid call.
    scheduler_url = os.environ.get("PROBE_SCHEDULER_URL", "")
    interruption_path = os.environ.get("PROBE_INTERRUPT_WAV", "")
    if scheduler_url:
        parsed = urlparse(scheduler_url)
        if (
            parsed.scheme != "http"
            or parsed.hostname not in ("127.0.0.1", "localhost")
            or timing != "after_turn"
        ):
            raise ValueError(
                "PROBE_SCHEDULER_URL requires a loopback Vite server and PROBE_TIMING=after_turn"
            )
    if interruption_path and not scheduler_url:
        raise ValueError("PROBE_INTERRUPT_WAV requires the actual browser scheduler")
    if interruption_path:
        commands[0]["content"][0]["text"] = (
            "Before the six facts arrive, please say these words aloud slowly: This is an isolated notification test. We are checking that new updates wait for the conversation and that the listener can interrupt an answer without losing later messages."
        )
        commands[1]["content"][0]["text"] = probe_commands("summary")[0]["content"][0][
            "text"
        ]
    try:
        import requests
        import websocket
    except ImportError as error:
        raise SystemExit(
            "This optional probe requires requests and websocket-client."
        ) from error
    chrome = (
        os.environ.get("CCCC_VOICE_CHROME_EXECUTABLE")
        or shutil.which("google-chrome")
        or shutil.which("chromium")
    )
    if not chrome:
        raise SystemExit("Chrome/Chromium is required for this optional WebRTC probe.")

    # Isolated synthetic call: no microphone, repository data, or Analyst process.
    profile = tempfile.TemporaryDirectory(prefix="cccc-voice-probe-browser-")
    browser = subprocess.Popen(
        [
            chrome,
            "--headless=new",
            "--no-sandbox",
            "--remote-debugging-port=0",
            "--remote-allow-origins=*",
            "--autoplay-policy=no-user-gesture-required",
            "--user-data-dir=" + profile.name,
            "about:blank",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    sock = None
    try:
        port_file = Path(profile.name) / "DevToolsActivePort"
        deadline = time.monotonic() + 15
        while not port_file.exists():
            if time.monotonic() > deadline:
                raise RuntimeError("isolated Chrome did not start")
            time.sleep(0.1)
        port = port_file.read_text().splitlines()[0]
        tab = requests.put(
            f"http://127.0.0.1:{port}/json/new?about:blank", timeout=5
        ).json()
        sock = websocket.create_connection(tab["webSocketDebuggerUrl"], timeout=35)
        sequence = 0

        def evaluate(expression):
            nonlocal sequence
            sequence += 1
            sock.send(
                json.dumps(
                    {
                        "id": sequence,
                        "method": "Runtime.evaluate",
                        "params": {
                            "expression": expression,
                            "awaitPromise": True,
                            "returnByValue": True,
                        },
                    }
                )
            )
            while True:
                packet = json.loads(sock.recv())
                if packet.get("id") == sequence:
                    if "error" in packet:
                        raise RuntimeError("CDP request failed")
                    if "exceptionDetails" in packet.get("result", {}):
                        raise RuntimeError("browser evaluation failed")
                    return packet["result"]["result"].get("value")

        if scheduler_url:
            evaluate(
                "location.href="
                + json.dumps(
                    scheduler_url.rstrip("/")
                    + "/ui/tests/browser/codex-voice-protocol.html"
                )
            )
            time.sleep(0.5)
        offer = evaluate("""(async () => {
          window.events = [];
          window.decodedAudioPower = 0;
          window.audioMeters = [];
          window.pc = new RTCPeerConnection();
          const ac = window.ac = new AudioContext();
          const oscillator = ac.createOscillator();
          const gain = ac.createGain(); gain.gain.value = 0;
          const destination = window.inputDestination = ac.createMediaStreamDestination();
          oscillator.connect(gain); gain.connect(destination); oscillator.start();
          for (const track of destination.stream.getTracks()) pc.addTrack(track, destination.stream);
          pc.ontrack = event => {
            const stream = new MediaStream([event.track]);
            // Chromium must activate its media renderer before the WebAudio
            // consumer receives decoded remote samples. Muting prevents sound
            // on the host; this does not record or use a microphone.
            const player = window.remotePlayer = new Audio();
            player.srcObject = stream; player.muted = true;
            player.play().catch(() => { window.audioPlaybackFailed = true; });
            const source = ac.createMediaStreamSource(stream);
            const silent = ac.createGain(); silent.gain.value = 0;
            const meter = ac.createAnalyser(); meter.fftSize = 512;
            source.connect(meter); meter.connect(silent); silent.connect(ac.destination);
            const samples = new Float32Array(meter.fftSize);
            audioMeters.push(setInterval(() => {
              meter.getFloatTimeDomainData(samples);
              for (const sample of samples) decodedAudioPower += sample * sample;
            }, 20));
          };
          window.dc = pc.createDataChannel('oai-events');
          window.overflow = false;
          window.observeProvider = data => {
            try {
              if (events.length >= 4096) { overflow = true; return; }
              const event = JSON.parse(data);
              events.push(event);
              window.scheduler?.observe(event);
            } catch { overflow = true; }
          };
          dc.onmessage = e => observeProvider(e.data);
          await ac.resume();
          await pc.setLocalDescription(await pc.createOffer());
          return pc.localDescription.sdp;
        })()""")
        if scheduler_url:
            evaluate("""(async () => {
              const { CodexVoiceProviderChannel } = await import('/ui/src/features/codexVoice/codexVoiceProviderChannel.ts');
              window.scheduler = new CodexVoiceProviderChannel(observeProvider,
                code => events.push({type:'error',error:{code}}), () => false,
                () => events.push({type:'probe.unconfirmed'}));
              scheduler.bind(dc);
              return true;
            })()""")
        if interruption_path:
            wav = Path(interruption_path).read_bytes()
            if len(wav) > 2 * 1024 * 1024:
                raise ValueError("synthetic interruption fixture exceeds 2 MiB")
            evaluate(
                """(async () => {
              const bytes = Uint8Array.from(atob("""
                + json.dumps(base64.b64encode(wav).decode())
                + """), c => c.charCodeAt(0));
              window.interruption = ac.createBufferSource();
              interruption.buffer = await ac.decodeAudioData(bytes.buffer);
              interruption.connect(inputDestination);
              return true;
            })()"""
            )
        auth_path = Path(
            os.environ.get(
                "CCCC_CODEX_AUTH_PATH",
                str(
                    Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex")))
                    / "auth.json"
                ),
            )
        )
        auth = json.loads(auth_path.read_text())["tokens"]
        result = requests.post(
            "https://chatgpt.com/backend-api/codex/realtime/calls?intent=quicksilver&architecture=avas",
            headers={
                "Authorization": "Bearer " + auth["access_token"],
                "chatgpt-account-id": auth["account_id"],
                "originator": "cccc",
                "x-session-id": str(uuid.uuid4()),
                "user-agent": "cccc/0.4.39",
                "openai-alpha": "quicksilver=v2",
            },
            json={
                "sdp": offer,
                "session": {
                    "model": "gpt-live-1-codex",
                    "audio": {"output": {"voice": "cove"}},
                    "instructions": "This is a synthetic protocol test, not a real task. Speak English. Briefly acknowledge each new speakable fact with its name and exact number. Do not use tools."
                    + (
                        " For the first context beginning 'Before the six facts', read the entire supplied paragraph aloud verbatim and slowly, instead of confirming that you will read it."
                        if interruption_path
                        else ""
                    ),
                    "delegation": {"type": "client", "ack_filler": True},
                },
            },
            timeout=30,
        )
        print("provider_http_status", result.status_code, flush=True)
        if not 200 <= result.status_code < 300:
            raise RuntimeError("provider rejected isolated test call")
        answer = result.text
        if answer.startswith("{"):
            payload = result.json()
            answer = payload.get("sdp", payload.get("answer", ""))
        evaluate(
            "pc.setRemoteDescription("
            + json.dumps({"type": "answer", "sdp": answer})
            + ")"
        )
        deadline = time.monotonic() + 20
        while evaluate("dc.readyState") != "open":
            if time.monotonic() > deadline:
                raise RuntimeError("WebRTC data channel did not open")
            time.sleep(0.2)
        split_delivery = timing in ("mid_turn", "after_turn")
        evaluate(
            "for (const c of "
            + json.dumps(commands[:1] if split_delivery else commands)
            + ") { if (window.scheduler) scheduler.send(c); else dc.send(JSON.stringify(c)); } true"
        )
        if split_delivery:
            deadline = time.monotonic() + 15
            boundary = (
                "turn.done"
                if timing == "after_turn" and not scheduler_url
                else "turn.created"
            )
            while not evaluate(
                "events.some(e => e.type === "
                + json.dumps(boundary)
                + " && e.turn?.role === 'assistant')"
            ):
                if time.monotonic() > deadline:
                    raise RuntimeError("no first assistant turn")
                time.sleep(0.1)
            if interruption_path:
                time.sleep(0.5)
                interruption_during_speech = evaluate(
                    "events.some(e => e.type === 'turn.created' && e.turn?.role === 'assistant' && !events.some(done => done.type === 'turn.done' && done.turn?.id === e.turn.id))"
                )
                evaluate("interruption.start(); true")
                deadline = time.monotonic() + 15
                while not evaluate(
                    "events.some(e => e.type === 'turn.created' && e.turn?.role === 'user')"
                ):
                    if time.monotonic() > deadline:
                        raise RuntimeError(
                            "synthetic speech did not create a user turn"
                        )
                    time.sleep(0.1)
            evaluate(
                "for (const c of "
                + json.dumps(commands[1:])
                + ") { if (window.scheduler) scheduler.send(c); else dc.send(JSON.stringify(c)); } true"
            )
            print(f"remaining_results_sent_after={boundary}", flush=True)
        print(f"sent_contexts={len(commands)} timing={timing}", flush=True)
        observed = []
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            batch = evaluate("({overflow, events: events.splice(0)})")
            if batch["overflow"] or len(observed) + len(batch["events"]) > 4096:
                raise RuntimeError("provider events exceeded probe limit")
            for event in batch["events"]:
                observed.append(event)
                if event.get("type") in [
                    "session.context.appended",
                    "delegation.context.appended",
                    "turn.created",
                    "turn.done",
                    "error",
                ]:
                    print(
                        json.dumps(protocol_observation(event), ensure_ascii=False),
                        flush=True,
                    )
            time.sleep(0.5)
        audio = evaluate("""(async () => {
          let bytes = 0, energy = 0;
          for (const s of (await pc.getStats()).values()) {
            if (s.type === 'inbound-rtp' && s.kind === 'audio') {
              bytes += s.bytesReceived || 0; energy += s.totalAudioEnergy || 0;
            }
          }
          return {bytes, energy, decoded_power: decodedAudioPower, context_state: ac.state};
        })()""")
        report = summarize_probe(observed, commands, audio)
        if scheduler_url:
            report["actual_browser_scheduler"] = True
            report["scheduler_receipts"] = evaluate("scheduler.receipt()")
            report["scheduler_unconfirmed"] = sum(
                e.get("type") == "probe.unconfirmed" for e in observed
            )
        if interruption_path:
            report["synthetic_user_turn_tested"] = any(
                e.get("type") == "turn.done" and e.get("turn", {}).get("role") == "user"
                for e in observed
            )
            report["user_interruption_tested"] = (
                report["synthetic_user_turn_tested"] and interruption_during_speech
            )
            report["synthetic_user_transcripts"] = [
                e["turn"].get("transcript", "")
                for e in observed
                if e.get("type") == "turn.done"
                and e.get("turn", {}).get("role") == "user"
            ]
            report["observed_delivery"] = (
                report["observed_delivery"] and report["user_interruption_tested"]
            )
        if os.environ.get("PROBE_INCLUDE_SYNTHETIC_TRANSCRIPT") == "1":
            report["synthetic_transcripts"] = [
                e["turn"].get("transcript", "")[:4000]
                for e in observed
                if e.get("type") == "turn.done"
                and isinstance(e.get("turn"), dict)
                and e["turn"].get("role") == "assistant"
            ]
        print(json.dumps(report, ensure_ascii=False), flush=True)
        return 0 if report["observed_delivery"] else 1
    finally:
        if sock:
            try:
                evaluate(
                    "audioMeters.forEach(clearInterval); window.remotePlayer?.pause(); pc.close(); ac.close(); true"
                )
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


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(json.dumps({"probe_failed": True, "error_type": type(error).__name__}))
        sys.exit(2)
