#!/usr/bin/env python3
"""Offline managed-provider fixture. Uses isolated CCCC IPC, never provider APIs."""
import hashlib
import json
import os
import re
import signal
import socket
import subprocess
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

runtime = os.environ.get("SECRETARY_FIXTURE_RUNTIME", Path(sys.argv[0]).name)
cwd = Path.cwd()
session_id = str(uuid.uuid4())
args = sys.argv[1:]

if runtime == "copilot" and "mcp" in args and "list" in args:
    print(json.dumps({"mcpServers": {"fixture-extra": {"type": "local"}}}))
    sys.exit(0)


def trace(stage):
    if os.environ.get("FIXTURE_EVENTS"):
        with open(Path(os.environ["FIXTURE_EVENTS"]) / "startup-trace.jsonl", "a") as stream:
            stream.write(json.dumps({"runtime": runtime, "stage": stage}) + "\n")


trace("process-start")

def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(value))
    tmp.replace(path)


current_task_id = None

def task_call(arguments):
    home = Path(os.environ["FIXTURE_HOME"])
    address = json.loads((home / "daemon/ccccd.addr.json").read_text())
    channel = socket.socket(socket.AF_UNIX if address["transport"] == "unix" else socket.AF_INET)
    channel.settimeout(5)
    channel.connect(address["path"] if address["transport"] == "unix" else (address["host"], address["port"]))
    arguments = dict(arguments, task_id=current_task_id, _cccc_secretary_token=os.environ["CCCC_SECRETARY_TASK_TOKEN"])
    channel.sendall((json.dumps({"v": 1, "op": "voice_secretary_task", "args": arguments}) + "\n").encode())
    response = json.loads(channel.makefile("rb").readline())
    channel.close()
    assert response["ok"], response.get("error", {}).get("code")
    return response["result"]


def packet(prompt):
    global current_task_id
    current_task_id = re.search(r"TASK_ID: ([a-f0-9]{32})", prompt).group(1)
    assert os.environ["CCCC_MCP_TOOL_PROFILE"] == "secretary-task"
    grant = os.environ["CCCC_SECRETARY_TASK_TOKEN"]
    assert not any(grant in arg for arg in sys.argv)
    context = task_call({"action": "context"})
    write_json(Path(os.environ["FIXTURE_EVENTS"]) / context["task_id"], {
        "task_id": context["task_id"], "group_id": context["target"]["group_id"],
        "cwd": str(cwd), "session": session_id, "runtime": runtime, "pid": os.getpid(),
    })
    return context


def submit(context):
    text = context["inputs"][0]["text"]
    kind = context["target"]["kind"]
    if kind == "document":
        document = cwd / context["working_document"]
        document.write_text(document.read_text() + "\n" + text)
        return task_call({"action": "commit", "base_version": context["base_version"]})
    if kind == "prompt":
        action = re.search(r'cccc_voice_secretary_task\(action="([^"]+)"', text)
        assert action, "Prompt input must name its terminal task-tool action"
        return task_call({"action": action.group(1), "draft_text": "Polished " + text})
    return task_call({"action": "report", "status": "done", "reply_text": "Answer " + text})


if "--version" in args:
    print("2.1.291" if runtime == "claude" else "1.18.33")
    sys.exit(0)

# Claude's existing adapter uses Agent View's control socket and transcript.
if runtime == "claude":
    if "--settings" in args:
        settings = json.loads(Path(args[args.index("--settings") + 1]).read_text())
        assert len(settings["env"]["CCCC_SECRETARY_TASK_TOKEN"]) == 64
        assert Path(args[args.index("--settings") + 1]).stat().st_mode & 0o077 == 0
        os.environ.update(settings.get("env", {}))
    root = Path(os.environ["CLAUDE_CONFIG_DIR"]).resolve()
    if "--bg" in args:
        assert "--strict-mcp-config" in args
        if os.environ.get("SECRETARY_FIXTURE_REJECT_LAUNCH"):
            print("Offline launch rejected", file=sys.stderr)
            sys.exit(2)
        ready = root / "fixture-ready"
        ready.unlink(missing_ok=True)
        old_socket = Path("/tmp") / ("cc-daemon-" + str(os.getuid())) / hashlib.sha256(str(root).encode()).hexdigest()[:8] / "control.sock"
        deadline = time.monotonic() + 4
        while old_socket.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        # Native Agent View filters caller environment; each worker reads its
        # own settings file. A first worker's grant must not reach later jobs.
        worker_env = {key: value for key, value in os.environ.items() if key in ("HOME", "PATH", "CLAUDE_CONFIG_DIR", "LANG")}
        child = subprocess.Popen([sys.argv[0], "--fixture-control", "--settings", args[args.index("--settings") + 1]], env=worker_env,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL, start_new_session=True)
        # Real Agent View launch returns after the managed job appears.
        ready = root / "fixture-ready"
        deadline = time.monotonic() + 4
        while not ready.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        assert ready.exists()
        print("started · abcdef12")
        sys.exit(0)
    short = "abcdef12"
    transcript = root / "projects/fixture" / (session_id + ".jsonl")
    state_path = root / "jobs" / short / "state.json"
    control_dir = Path("/tmp") / ("cc-daemon-" + str(os.getuid())) / hashlib.sha256(str(root).encode()).hexdigest()[:8]
    control_dir.mkdir(parents=True, mode=0o700)
    key = root / "daemon/control.key"
    key.parent.mkdir(parents=True, exist_ok=True)
    key.write_text("f" * 32)
    key.chmod(0o600)
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(str(control_dir / "control.sock"))
    listener.listen()
    listener.settimeout(0.1)
    write_json(state_path, {"sessionId": session_id, "daemonShort": short, "cwd": str(cwd),
                           "inFlight": {"tasks": 0, "queued": 0}, "tempo": "idle", "outcome": None})
    (root / "fixture-ready").write_text(str(os.getpid()))
    deadline = None
    stopped = False
    while deadline is None or time.monotonic() < deadline:
        try:
            client, _ = listener.accept()
        except socket.timeout:
            continue
        request = json.loads(client.makefile("rb").readline())
        operation = request["op"]
        result = {"ok": True, "op": operation}
        if operation == "list":
            result["jobs"] = [] if stopped else [{"short": short, "sessionId": session_id,
                "cwd": str(cwd), "cliVersion": "2.1.291"}]
            if os.environ.get("FIXTURE_UNRELATED_JOB"):
                result["jobs"].append({"short": "11112222", "sessionId": "11111111-2222-4333-8444-555555555555",
                                       "cwd": str(root / "unrelated-actor"), "cliVersion": "2.1.291"})
        elif operation == "reply":
            context = packet(request["text"])
            if "HOLD" not in context["inputs"][0]["text"]:
                submit(context)
                transcript.parent.mkdir(parents=True, exist_ok=True)
                records = [{"type": "user", "sessionId": session_id, "promptId": current_task_id,
                            "message": {"content": request["text"]}},
                           {"type": "assistant", "sessionId": session_id,
                            "message": {"content": [{"type": "text", "text": "submitted"}]}},
                           {"type": "system", "sessionId": session_id, "subtype": "turn_duration"}]
                with transcript.open("a") as stream:
                    stream.write("".join(json.dumps(r) + "\n" for r in records))
                write_json(state_path, {"sessionId": session_id, "daemonShort": short, "cwd": str(cwd),
                    "linkScanPath": str(transcript), "inFlight": {"tasks": 0, "queued": 0},
                    "tempo": "idle", "outcome": None})
        elif operation == "kill":
            assert request["short"] == short, "unrelated job was targeted"
            stopped = True
            deadline = time.monotonic() + 0.6
        client.sendall((json.dumps(result) + "\n").encode())
        client.close()
    listener.close()
    (control_dir / "control.sock").unlink(missing_ok=True)
    control_dir.rmdir()
    sys.exit(0)

# Grok has a native user MCP registry and a separately owned leader process.
if runtime == "grok":
    config = Path(os.environ["GROK_HOME"]) / "config.toml"
    if args[:1] == ["inspect"]:
        entries = [] if not config.exists() else [{"name": "cccc", "transport": "stdio",
            "target": os.environ["CCCC_CLI"], "source": {"type": "configToml", "path": str(config)}}]
        print(json.dumps({"mcpServers": entries}))
        sys.exit(0)
    if args[:2] == ["mcp", "add"]:
        config.parent.mkdir(parents=True, exist_ok=True)
        config.write_text('[mcp_servers.cccc]\ncommand="${CCCC_CLI:-cccc}"\nargs=["mcp"]\n')
        sys.exit(0)
    if args[:2] == ["mcp", "list"]:
        print(json.dumps([{"name": "cccc", "command": os.environ["CCCC_CLI"], "args": ["mcp"], "enabled": True}]))
        sys.exit(0)
    if "leader" in args:
        while True:
            signal.pause()

# OpenCode/Kilo combine ACP control with an authenticated HTTP lifecycle stream.
events = []
event_lock = threading.Condition()
if runtime in ("opencode", "kilo"):
    prefix = runtime.upper()
    port = int(args[args.index("--port") + 1])
    import base64
    authorization = "Basic " + base64.b64encode((os.environ[prefix + "_SERVER_USERNAME"] + ":" + os.environ[prefix + "_SERVER_PASSWORD"]).encode()).decode()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            trace("http:" + self.path.split("?")[0])
            if self.headers.get("Authorization") != authorization:
                self.send_response(401)
                self.end_headers()
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream" if self.path.startswith("/event") else "application/json")
            self.end_headers()
            if not self.path.startswith("/event"):
                self.wfile.write(b'{"healthy":true}')
                return
            self.wfile.write(b": ready\n\n")
            self.wfile.flush()
            offset = 0
            while True:
                with event_lock:
                    if len(events) == offset:
                        event_lock.wait(timeout=1)
                    batch = events[offset:]
                    offset = len(events)
                for event in batch:
                    self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
                self.wfile.flush()

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()


def emit(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def status(kind):
    with event_lock:
        events.append({"type": "session.status", "properties": {"sessionID": session_id, "status": {"type": kind}}})
        event_lock.notify_all()


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    trace("acp:" + str(method))
    params = request.get("params", {})
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}
    elif method == "session/new":
        servers = params.get("mcpServers", [])
        if servers:
            env = {item["name"]: item["value"] for item in servers[0]["env"]}
            assert env["CCCC_MCP_TOOL_PROFILE"] == "secretary-task"
            assert "CCCC_SECRETARY_TASK_TOKEN" not in env
        result = {"sessionId": session_id, "configOptions": []}
    elif method == "session/load":
        raise AssertionError("A Secretary task must never resume an Actor/Analyst session")
    elif method == "session/prompt":
        assert "Voice Secretary" in params["prompt"][0]["text"]
        context = packet(params["prompt"][0]["text"])
        text = context["inputs"][0]["text"]
        if "HOLD" in text:
            continue
        if "DISCONNECT" in text:
            sys.exit(0)
        if runtime in ("opencode", "kilo"):
            # These providers admit controlled prompts through the ordered user
            # echo, not busy/idle or the ACP response alone.
            user = str(uuid.uuid4())
            with event_lock:
                events.extend([
                    {"type": "message.updated", "properties": {"info": {"id": user, "role": "user", "sessionID": session_id}}},
                    {"type": "message.part.updated", "properties": {"part": {"id": "part-" + user, "messageID": user, "sessionID": session_id, "type": "text", "text": params["prompt"][0]["text"]}}},
                ])
                event_lock.notify_all()
            status("busy")
        else:
            emit({"method": "session/update", "params": {"sessionId": session_id, "update": {
                "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "processing"}}}})
        if "INTERACTION_" in text and not context.get("followup"):
            deadline = time.monotonic() + 10
            while not (Path(os.environ["FIXTURE_EVENTS"]) / "release-interaction").exists():
                assert time.monotonic() < deadline, "Owner fixture did not release interaction"
                time.sleep(0.01)
            if "INTERACTION_QUESTION" in text:
                emit({"id": "fixture-interaction", "method": "cursor/ask_question", "params": {
                    "sessionId": session_id, "toolCallId": "fixture-tool", "questions": [{
                        "id": "date", "prompt": "Which date should I use? " + os.environ["FIXTURE_PRIVATE_KEY"] + " " + os.environ["CCCC_SECRETARY_TASK_TOKEN"],
                        "options": [{"id": "today", "label": "Today"}]}]}})
            elif "INTERACTION_PLAN" in text:
                emit({"id": "fixture-interaction", "method": "cursor/create_plan", "params": {
                    "sessionId": session_id, "toolCallId": "fixture-tool", "name": "Read project notes",
                    "plan": "Read the accepted Group notes, then answer."}})
            else:
                options = [{"optionId": "yes", "kind": "allow_once", "name": "Allow"},
                           {"optionId": "no", "kind": "reject_once", "name": "Reject"}]
                if "INTERACTION_DENIED" in text:
                    options = options[1:]
                emit({"id": "fixture-interaction", "method": "session/request_permission", "params": {
                    "sessionId": session_id, "toolCall": {"toolCallId": "fixture-tool", "title": "Read project notes"},
                    "options": options}})
            continue
        if "HOLD" in text:
            continue
        submit(context)
        if runtime in ("opencode", "kilo"):
            status("idle")
        emit({"id": request["id"], "result": {"stopReason": "end_turn"}})
        continue
    elif method == "session/cancel":
        continue
    elif method is None:
        if request.get("id") == "fixture-interaction":
            outcome = request.get("result", {}).get("outcome", {})
            allowed = outcome.get("outcome") in ("accepted", "answered") or (outcome.get("outcome") == "selected" and outcome.get("optionId") == "yes")
            write_json(Path(os.environ["FIXTURE_EVENTS"]) / "interaction-response", {"allowed": allowed})
        continue
    else:
        result = {}
    emit({"id": request["id"], "result": result})
