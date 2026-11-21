#!/usr/bin/env python3
"""Offline OpenCode HTTP/SSE and TUI fixture; no provider or MCP calls."""
import base64
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

args = sys.argv[1:]
if "--version" in args:
    print("2.2.5")
    sys.exit(0)

root = Path(os.environ["OPENCODE_FIXTURE_DIR"])
root.mkdir(parents=True, exist_ok=True)
session = "ses_chet_fixture"


def record(name, value):
    with (root / name).open("a") as stream:
        stream.write(json.dumps(value) + "\n")


if "--server" in args:
    visible = args[args.index("--session") + 1]
    (root / "terminal-session").write_text(visible)
    print("> ", flush=True)
    for line in sys.stdin:
        text = line.strip()
        if text == "/new":
            visible = "ses_visible_other"
            (root / "terminal-session").write_text(visible)
        elif text:
            record("terminal-inputs.jsonl", {"sessionID": visible, "text": text})
    sys.exit(0)

assert "serve" in args and "--stdio" in args
authorization = "Basic " + base64.b64encode(
    ("opencode:" + os.environ["OPENCODE_PASSWORD"]).encode()
).decode()
events = []
event_lock = threading.Condition()
request_lock = threading.Lock()
# This fixture runs inbox items one at a time, in the order it accepted them.
inbox = []
inbox_turn = threading.Condition()
requests = 0


def emit(kind, data):
    with event_lock:
        events.append({"type": kind, "data": {"sessionID": session, **data},
                       "durable": {"seq": len(events) + 1}})
        event_lock.notify_all()


def wait_for(name):
    deadline = time.monotonic() + 15
    while not (root / name).exists():
        assert time.monotonic() < deadline, name
        time.sleep(0.01)


def finish(text, message, delivery):
    emit("session.inbox.enqueued", {"inboxID": message,
         "item": {"type": "user", "payload": {"text": text}, "delivery": delivery}})
    with inbox_turn:
        while inbox[0] != message:
            inbox_turn.wait()
    run(text, message)
    with inbox_turn:
        inbox.pop(0)
        inbox_turn.notify_all()


def run(text, message):
    emit("session.inbox.delivered", {"inboxID": message})
    assistant = "assistant-" + message
    emit("session.step.started", {"assistantMessageID": assistant})
    if "HOLD_FIRST" in text:
        wait_for("release-completion")
    emit("session.text.ended", {"assistantMessageID": assistant,
         "ordinal": 0, "text": "Answer: " + text})
    emit("session.step.ended", {"assistantMessageID": assistant, "finish": "stop"})
    emit("session.execution.succeeded", {})


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def authorized(self):
        if self.headers.get("Authorization") == authorization:
            return True
        self.send_response(401)
        self.end_headers()
        return False

    def reply(self, value):
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(value).encode())

    def do_GET(self):
        if not self.authorized():
            return
        if self.path == "/api/info":
            self.reply({"pid": os.getpid(), "version": "2.2.5"})
        elif self.path.startswith("/api/mcp?"):
            self.reply({"data": [{"name": "cccc", "status": {"status": "connected"}}]})
        else:
            assert self.path == "/api/event"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            self.wfile.write(b'data: {"type":"server.connected","data":{}}\n\n')
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

    def do_POST(self):
        global requests
        if not self.authorized():
            return
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.path == "/api/session":
            self.reply({"data": {"id": session, "location": {"directory": str(Path.cwd())}}})
            return
        assert self.path == f"/api/session/{session}/prompt", self.path
        # This fixture queues plain prompts as well as explicitly queued ones.
        assert body.get("delivery", "queue") == "queue" and "id" not in body
        delivery = body.get("delivery", "queue")
        with request_lock:
            requests += 1
            message = f"msg_fixture_{requests}"
            record("requests.jsonl", {"sessionID": session, "id": message, **body})
        if "DELAY_ADMISSION" in body["text"]:
            (root / "admission-started").write_text(message)
            wait_for("release-admission")
        if "DISCONNECT_ADMISSION" in body["text"]:
            self.close_connection = True
            return
        with inbox_turn:
            inbox.append(message)
        self.reply({"data": {"id": message, "sessionID": session, "delivery": delivery}})
        threading.Thread(target=finish, args=(body["text"], message, delivery),
                         daemon=True).start()


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
server.daemon_threads = True
threading.Thread(target=server.serve_forever, daemon=True).start()
print(json.dumps({"url": f"http://127.0.0.1:{server.server_port}"}), flush=True)
sys.stdin.read()
