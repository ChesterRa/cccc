#!/usr/bin/env python3
"""Offline Agent View fixture: exact resume, copied IDs, missing history and trust."""
import hashlib
import json
import os
import pathlib
import socket
import sys
import time
import tty

SAVED = "52b41c61-e23c-4b7c-8b60-809c347451b5"
FRESH = "ca52e69a-6596-4abd-a0ec-3e8690dc70e1"
SHORT = "abcdef12"


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value))
    temporary.replace(path)


if len(sys.argv) > 1 and sys.argv[1] == "server":
    root = pathlib.Path(sys.argv[2]).resolve()
    directory = pathlib.Path("/tmp") / f"cc-daemon-{os.getuid()}" / hashlib.sha256(str(root).encode()).hexdigest()[:8]
    directory.mkdir(parents=True, exist_ok=True)
    directory.chmod(0o700)
    daemon = root / "daemon"
    daemon.mkdir(exist_ok=True)
    key = daemon / "control.key"
    key.write_text("0123456789abcdef0123456789abcdef")
    key.chmod(0o600)
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(str(directory / "control.sock"))
    listener.listen()
    (root / "ready").touch()
    while True:
        connection, _ = listener.accept()
        with connection:
            line = connection.makefile("rb").readline()
            request = json.loads(line)
            op = request["op"]
            jobs = []
            if (root / "active").exists():
                state = json.loads((root / "jobs" / SHORT / "state.json").read_text())
                jobs = [{"short": SHORT, "sessionId": state["sessionId"], "cwd": state["cwd"], "cliVersion": "2.1.286"}]
            if op == "list":
                result = {"ok": True, "op": op, "jobs": jobs}
            elif op == "kill":
                (root / "active").unlink(missing_ok=True)
                result = {"ok": True, "op": op}
            else:
                result = {"ok": False, "op": op, "code": "EPROTO", "error": "unexpected fixture operation"}
            connection.sendall(json.dumps(result).encode() + b"\n")
elif "--version" in sys.argv:
    print("2.1.286 (Claude Code)")
else:
    root = pathlib.Path(os.environ["CLAUDE_CONFIG_DIR"])
    mode = (root / "mode").read_text()
    if "--bg" in sys.argv:
        with (root / "launches").open("a") as log:
            log.write("resume\n" if "--resume" in sys.argv else "fresh\n")
        if mode == "untrusted":
            print("Workspace not trusted", file=sys.stderr)
            sys.exit(1)
        if mode == "delayed_resume" and "--resume" in sys.argv:
            (root / "resume_pending").touch()
            deadline = time.monotonic() + 10
            while not (root / "release_resume").exists() and time.monotonic() < deadline:
                time.sleep(0.01)
        session = SAVED if "--resume" in sys.argv and mode != "copied" else FRESH
        path = root / "projects" / "workspace" / f"{session}.jsonl"
        if mode != "missing_history" and not path.exists():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps({"type": "user", "sessionId": session, "message": {"content": "fixture history"}}) + "\n")
        write_json(root / "jobs" / SHORT / "state.json", {
            "sessionId": session, "daemonShort": SHORT, "cwd": os.getcwd(),
            "tempo": "idle", "inFlight": {"tasks": 0, "queued": 0},
            "intent": "history-bearing", "linkScanOffset": 0,
            "linkScanPath": str(path) if path.exists() else None, "outcome": None,
        })
        (root / "active").touch()
        print(f"started · {SHORT}")
    else:
        # Native attach or the temporary trust prompt. No model task is executed.
        tty.setraw(0)
        os.write(1, b"\x1b[?2004hOffline Claude fixture\r\n>")
        (root / "terminal_ready").touch()
        pending = b""
        while True:
            data = os.read(0, 65536)
            if not data:
                break
            pending += data
            while b"\r" in pending:
                message, pending = pending.split(b"\r", 1)
                with (root / "received").open("a") as received:
                    received.write(json.dumps(message.decode()) + "\n")
