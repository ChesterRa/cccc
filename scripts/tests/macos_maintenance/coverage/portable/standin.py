#!/usr/bin/env python3
"""Stand-in agent for the split-supervisor acceptance harness.

Never contacts a provider, never runs a tool, never reads credentials.

The program name picks the mode: a name containing "term" runs as a plain
terminal program (Custom runtime, PTY); any other name (kimi, fake-dsh) speaks
ACP over stdio (local_headless structured session, DeepSeek).

Every process start draws a random incarnation. Each record goes to
<STANDIN_LOG_DIR>/<actor>.<incarnation>.jsonl as one JSON line:
  start  pid, ppid, /proc starttime, session and group ids
  input  one per delivered message that carries a TOKEN-<hex> marker
  chunk  numbered output pieces of the turn started by that input
  end    the turn's last piece was written (or it was cancelled)
  rpc    ACP mode: every method received, unknown ones marked
A maintenance swap that holds the process keeps the incarnation and the chunk
numbers keep rising; a stop and restart shows a new incarnation.

Environment: STANDIN_LOG_DIR (default <program dir>/../logs),
STANDIN_CHUNKS (default 60), STANDIN_INTERVAL seconds (default 0.5).
"""
import json
import os
import re
import signal
import sys
import threading
import time
import uuid
from pathlib import Path

INCARNATION = uuid.uuid4().hex[:12]
PROGRAM = Path(sys.argv[0])
MODE = "terminal" if "term" in PROGRAM.name else "acp"
NAME = os.environ.get("CCCC_ACTOR_ID") or PROGRAM.stem
LOG_DIR = Path(os.environ.get("STANDIN_LOG_DIR") or PROGRAM.absolute().parent.parent / "logs")
CHUNKS = int(os.environ.get("STANDIN_CHUNKS", "60"))
INTERVAL = float(os.environ.get("STANDIN_INTERVAL", "0.5"))
TOKEN = re.compile(r"TOKEN-[0-9a-f]{8,32}")

LOG_DIR.mkdir(parents=True, exist_ok=True)
_log_lock = threading.Lock()
_log = open(LOG_DIR / f"{NAME}.{INCARNATION}.jsonl", "a", buffering=1, encoding="utf-8")
_out_lock = threading.Lock()


def record(event, **fields):
    line = json.dumps({"ev": event, "inc": INCARNATION, "mono": round(time.monotonic(), 3), **fields},
                      ensure_ascii=False)
    with _log_lock:
        _log.write(line + "\n")


def starttime():
    if sys.platform == "darwin":
        # The acceptance driver verifies birth and boot identity via the native inspector.
        return None
    stat = Path("/proc/self/stat").read_text()
    return int(stat[stat.rindex(")") + 2:].split()[19])


def on_signal(number, _frame):
    record("signal", sig=number)
    _log.flush()
    os._exit(128 + number)


for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
    signal.signal(sig, on_signal)

record("start", mode=MODE, pid=os.getpid(), ppid=os.getppid(), starttime=starttime(),
       sid=os.getsid(0), pgid=os.getpgid(0), argv=sys.argv, chunks=CHUNKS, interval=INTERVAL)

seen_tokens = {}
inputs = 0


def note_input(text):
    """Record one delivered message; returns its token, or None when it has none."""
    global inputs
    match = TOKEN.search(text)
    if not match:
        return None
    token = match.group(0)
    inputs += 1
    seen_tokens[token] = seen_tokens.get(token, 0) + 1
    record("input", n=inputs, token=token, copies=seen_tokens[token], text=text[:2000])
    return token


def run_turn(token, emit, cancelled, finish):
    for k in range(1, CHUNKS + 1):
        release = os.environ.get("STANDIN_RELEASE_DIR")
        if k == 2 and release and TOKEN.fullmatch(token):
            while not (Path(release) / token).exists() and not cancelled.is_set():
                time.sleep(0.03)
        if cancelled.is_set():
            record("end", token=token, reason="cancelled", last=k - 1)
            finish("cancelled")
            return
        emit(f"chunk {INCARNATION} {token} {k}/{CHUNKS}\n")
        record("chunk", token=token, k=k, n=CHUNKS)
        time.sleep(INTERVAL)
    record("end", token=token, reason="end_turn", last=CHUNKS)
    finish("end_turn")


def terminal_mode():
    def emit(text):
        with _out_lock:
            sys.stdout.write(text)
            sys.stdout.flush()

    emit(f"standin terminal ready inc={INCARNATION}\n")
    for line in sys.stdin:
        token = note_input(line)
        if token and seen_tokens[token] == 1:
            threading.Thread(target=run_turn, daemon=True,
                             args=(token, emit, threading.Event(), lambda _reason: None)).start()
    record("eof")


def acp_mode():
    sessions_file = Path(os.environ.get("HOME", str(LOG_DIR))) / f"standin_sessions_{NAME}.json"
    sessions_file.parent.mkdir(parents=True, exist_ok=True)
    try:
        sessions = json.loads(sessions_file.read_text())
    except (OSError, ValueError):
        sessions = []
    state = {"sid": "", "cancel": None}

    def send(**frame):
        with _out_lock:
            sys.stdout.write(json.dumps({"jsonrpc": "2.0", **frame}) + "\n")
            sys.stdout.flush()

    def session_result(sid):
        return {"sessionId": sid,
                "models": {"availableModels": [{"modelId": "model-kimi", "name": "Stand-in model"}]},
                "configOptions": [{"id": "model", "options": [{"value": "model-kimi", "name": "Stand-in model"}]}]}

    for line in sys.stdin:
        try:
            message = json.loads(line)
        except ValueError:
            record("rpc", method=None, bad_json=line[:200])
            continue
        method = message.get("method")
        params = message.get("params") or {}
        has_id = "id" in message
        known = method in ("initialize", "session/new", "session/load", "session/prompt", "session/cancel",
                           "session/set_config_option", "session/set_model", "session/set_mode")
        record("rpc", method=method, id=message.get("id"), unknown=bool(method) and not known)
        if method is None:
            continue
        if method == "initialize":
            send(id=message["id"], result={"protocolVersion": 1,
                                           "agentCapabilities": {"loadSession": True},
                                           "agentInfo": {"name": "standin", "version": "0.0.0"}})
        elif method == "session/new":
            state["sid"] = "standin-" + uuid.uuid4().hex
            sessions.append(state["sid"])
            sessions_file.write_text(json.dumps(sessions))
            send(id=message["id"], result=session_result(state["sid"]))
        elif method == "session/load":
            if params.get("sessionId") in sessions:
                state["sid"] = params["sessionId"]
                send(id=message["id"], result=session_result(state["sid"]))
            else:
                send(id=message["id"], error={"code": -32000, "message": "Session unavailable"})
        elif method in ("session/set_config_option", "session/set_model", "session/set_mode"):
            send(id=message["id"], result={})
        elif method == "session/prompt":
            text = "".join(block.get("text", "") for block in params.get("prompt", [])
                           if isinstance(block, dict))
            token = note_input(text) or f"untokened-{inputs}"
            sid = params.get("sessionId", state["sid"])
            request_id = message["id"]
            cancelled = threading.Event()
            state["cancel"] = cancelled

            def emit(piece, sid=sid):
                send(method="session/update", params={"sessionId": sid, "update": {
                    "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": piece}}})

            def finish(reason, request_id=request_id):
                send(id=request_id, result={"stopReason": reason})

            threading.Thread(target=run_turn, daemon=True, args=(token, emit, cancelled, finish)).start()
        elif method == "session/cancel":
            if state["cancel"] is not None:
                state["cancel"].set()
        elif has_id:
            send(id=message["id"], error={"code": -32601, "message": f"Method not found: {method}"})
    record("eof")


try:
    terminal_mode() if MODE == "terminal" else acp_mode()
except Exception:
    import traceback
    record("crash", traceback=traceback.format_exc()[-2000:])
    raise
