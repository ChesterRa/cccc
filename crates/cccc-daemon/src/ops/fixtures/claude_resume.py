#!/usr/bin/env python3
"""Offline Agent View fixture: exact resume, copied IDs, missing history, busy jobs and trust."""
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
LIVE_SHORT = "fedcba98"


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value))
    temporary.replace(path)


def confirmation_stage(root, mode):
    if mode == "workspace_then_disclaimer":
        record = root / ".claude.json"
        trusted = record.exists() and json.loads(record.read_text()).get("projects", {}).get(os.getcwd(), {}).get("hasTrustDialogAccepted") is True
        if not trusted:
            return "workspace"
    if mode in {"disclaimer", "workspace_then_disclaimer", "confirmation_failure"}:
        settings = root / "settings.json"
        accepted = settings.exists() and json.loads(settings.read_text()).get("skipDangerousModePermissionPrompt") is True
        if not accepted:
            return "disclaimer"
    return None


def show_confirmation(root, stage):
    (root / "prompt_stage").write_text(stage)
    os.write(1, f"\r\nOffline {stage} confirmation: type yes or no\r\n>".encode())


def launch_context(root):
    settings_path = next((sys.argv[index + 1] for index, arg in enumerate(sys.argv[:-1]) if arg == "--settings"), None)
    private = {"settings_arg": False, "private_settings": False, "auth_digest": None, "routing_digest": None}
    if settings_path is not None:
        path = pathlib.Path(settings_path).resolve()
        # This fixture must never inspect a real provider configuration.
        path.relative_to(pathlib.Path(sys.argv[0]).resolve().parent)
        environment = json.loads(path.read_text()).get("env", {})
        private = {"settings_arg": True, "private_settings": path.stat().st_mode & 0o077 == 0,
                   "auth_digest": hashlib.sha256(str(environment.get("ANTHROPIC_AUTH_TOKEN", "")).encode()).hexdigest(),
                   "routing_digest": hashlib.sha256(str(environment.get("ANTHROPIC_BASE_URL", "")).encode()).hexdigest()}
    return {**private, "path": os.environ.get("PATH"), "executable": sys.argv[0], "cwd": os.getcwd(), "config_dir": str(root.resolve()),
            "config_dir_env": os.environ.get("CLAUDE_CONFIG_DIR"), "home": os.environ.get("HOME"),
            "userprofile": os.environ.get("USERPROFILE"),
            "host_oauth_url": os.environ.get("CLAUDE_CODE_CUSTOM_OAUTH_URL")}


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
            with (root / "control_requests.jsonl").open("a") as log:
                log.write(json.dumps({"op": op, "short": request.get("short")}) + "\n")
            jobs = []
            if (root / "active").exists():
                state = json.loads((root / "jobs" / SHORT / "state.json").read_text())
                jobs = [{"short": SHORT, "sessionId": state["sessionId"], "cwd": state["cwd"], "cliVersion": "2.1.286"}]
            if (root / "saved_live").exists():
                state = json.loads((root / "jobs" / LIVE_SHORT / "state.json").read_text())
                jobs.append({"short": LIVE_SHORT, "sessionId": SAVED, "cwd": state["cwd"], "cliVersion": "2.1.286"})
            if op == "list" and (root / "deny_control").exists():
                result = {"ok": False, "op": op, "code": "EAUTH", "error": "fixture control refused"}
            elif op == "list":
                result = {"ok": True, "op": op, "jobs": jobs}
            elif op == "kill":
                if request.get("short") == LIVE_SHORT:
                    (root / "saved_live").unlink(missing_ok=True)
                else:
                    (root / "active").unlink(missing_ok=True)
                result = {"ok": True, "op": op}
            else:
                result = {"ok": False, "op": op, "code": "EPROTO", "error": "unexpected fixture operation"}
            connection.sendall(json.dumps(result).encode() + b"\n")
elif "--version" in sys.argv:
    configured = os.environ.get("CLAUDE_CONFIG_DIR")
    if configured:
        root = pathlib.Path(configured).resolve()
        # Publish a job after receipt preparation, at the real launch's
        # version check. The fixture must never read a user's configuration.
        root.relative_to(pathlib.Path(sys.argv[0]).resolve().parent)
        pending = root / "publish_saved_live.json"
        if pending.exists():
            publish = json.loads(pending.read_text())
            receipt_path = pathlib.Path(publish["receipt_path"]).resolve()
            receipt_path.relative_to(pathlib.Path(sys.argv[0]).resolve().parent)
            receipt = json.loads(receipt_path.read_text())
            # Earlier CLI checks must not publish it before prepare has
            # recorded a new attempt. This is the prepare/launch race case.
            if receipt.get("last_resume_attempt_id"):
                write_json(root / "jobs" / LIVE_SHORT / "state.json", publish["state"])
                (root / "saved_live").touch()
                pending.unlink()
    print("2.1.286 (Claude Code)")
else:
    configured = os.environ.get("CLAUDE_CONFIG_DIR")
    root = pathlib.Path(configured) if configured is not None else pathlib.Path(os.environ.get("HOME") or os.environ["USERPROFILE"]) / ".claude"
    mode = (root / "mode").read_text()
    if "--bg" in sys.argv:
        # Read consent before this launch is counted: a test may answer the
        # next prompt as soon as it sees the count.
        stage = confirmation_stage(root, mode)
        with (root / "launches").open("a") as log:
            log.write("resume\n" if "--resume" in sys.argv else "fresh\n")
        with (root / "argv.jsonl").open("a") as log:
            log.write(json.dumps(sys.argv[1:]) + "\n")
        with (root / "launch_contexts").open("a") as log:
            log.write(json.dumps(launch_context(root)) + "\n")
        if mode == "untrusted" or stage == "workspace":
            print("Workspace not trusted", file=sys.stderr)
            sys.exit(1)
        if stage == "disclaimer":
            print("--bg with bypassPermissions requires accepting the disclaimer first. Run `claude --dangerously-skip-permissions` once interactively.", file=sys.stderr)
            sys.exit(1)
        if mode in {"confirmation_failure", "startup_failure"}:
            print("fixture provider startup failed after confirmation", file=sys.stderr)
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
            records = [
                {"type": "user", "sessionId": session, "promptId": "fixture-history", "message": {"content": "fixture history"}},
                {"type": "system", "sessionId": session, "subtype": "turn_duration"},
            ]
            path.write_text("".join(json.dumps(record) + "\n" for record in records))
        write_json(root / "jobs" / SHORT / "state.json", {
            "sessionId": session, "daemonShort": SHORT, "cwd": os.getcwd(),
            "tempo": "working" if mode == "busy" else "idle", "inFlight": {"tasks": 0, "queued": 0},
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
        stage = confirmation_stage(root, mode)
        confirmation_terminal = stage is not None
        if stage:
            write_json(root / "prompt_launch.json", {**launch_context(root), "argv": sys.argv[1:]})
            if "--dangerously-skip-permissions" not in sys.argv:
                os.write(1, b"\r\nFixture confirmation requires the interactive bypass flag\r\n")
                sys.exit(2)
            show_confirmation(root, stage)
        else:
            (root / "managed_terminal_ready").touch()
        pending = b""
        while True:
            data = os.read(0, 65536)
            if not data:
                break
            if stage and (b"\x03" in data or b"\x04" in data):
                (root / "prompt_closed").touch()
                sys.exit(0)
            pending += data
            while b"\r" in pending:
                message, pending = pending.split(b"\r", 1)
                if stage:
                    answer = message.decode().strip()
                    with (root / "confirmation_inputs").open("a") as inputs:
                        inputs.write(json.dumps(answer) + "\n")
                    if answer == "no":
                        (root / "prompt_declined").touch()
                        sys.exit(1)
                    if answer != "yes":
                        show_confirmation(root, stage)
                        continue
                    if stage == "workspace":
                        write_json(root / ".claude.json", {"projects": {os.getcwd(): {"hasTrustDialogAccepted": True}}})
                    else:
                        # Only this fake provider writes acceptance, after explicit PTY input.
                        write_json(root / "settings.json", {"skipDangerousModePermissionPrompt": True})
                    stage = confirmation_stage(root, mode)
                    if stage:
                        show_confirmation(root, stage)
                    else:
                        (root / "prompt_stage").write_text("accepted")
                    continue
                target = "unexpected_confirmation_tasks" if confirmation_terminal else "received"
                with (root / target).open("a") as received:
                    received.write(json.dumps(message.decode()) + "\n")
