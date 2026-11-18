#!/usr/bin/env python3
"""Official ACP shape fixture; no provider, network or real tool execution."""
import json
import os
import sys
import time
import uuid
from pathlib import Path

root = Path.cwd()
if "--tui" in sys.argv[1:]:
    # Hermes's own terminal viewing the member's session. It reads no keys, so
    # a test can fill its input; raw mode keeps Ctrl+C an ordinary key.
    import tty
    tty.setraw(sys.stdin.fileno())
    (root / "v3_viewer_ready").touch()
    while True:
        time.sleep(60)
if "--setup" in sys.argv[1:]:
    # Hermes's own provider setup, run in the member's terminal. It ends when
    # the test says so; with v3_setup_quit the user left without a provider.
    with (root / "v3_setup_runs.jsonl").open("a") as log:
        log.write(json.dumps(sys.argv[1:]) + "\n")
    finish = root / "v3_setup_finish"
    while not finish.exists():
        time.sleep(0.02)
    finish.unlink()
    if not (root / "v3_setup_quit").exists():
        (root / "v3_setup_needed").unlink(missing_ok=True)
    sys.exit(0)
# A provider that stops reading after session/new: a small stdin pipe makes the
# client's next write block, so its protocol loop answers nothing.
stall_stdin = (root / "v3_stall_stdin").exists()
if stall_stdin:
    import fcntl
    fcntl.fcntl(sys.stdin.fileno(), getattr(fcntl, "F_SETPIPE_SZ", 1031), 4096)
home = Path(os.environ["HOME"])
home.mkdir(parents=True, exist_ok=True)
saved = home / "v3_sessions.json"
sessions = json.loads(saved.read_text()) if saved.exists() else {}
vendor = Path(sys.argv[0]).name
sid = ""
active = None
active_text = ""
permission = None


def emit(**frame):
    print(json.dumps({"jsonrpc": "2.0", **frame}), flush=True)


def type_in_terminal():
    # A turn the user typed in the provider's own terminal, once the test says
    # CCCC is listening.
    signal = root / "v3_type_in_terminal"
    while not signal.exists():
        time.sleep(0.02)
    emit(method="session/update", params={"sessionId": sid, "update": {
        "sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "typed in the terminal"}}})


if (root / "v3_terminal_turns").exists():
    import threading
    threading.Thread(target=type_in_terminal, daemon=True).start()


def finish(reason="end_turn"):
    global active, permission
    if active is not None:
        emit(id=active, result={"stopReason": reason})
    active = permission = None


def session_result(new=False):
    model = "model-" + {"kiro-cli": "kiro", "hermes": "hermes", "droid": "droid"}[vendor]
    result = {"models": {"availableModels": [{"modelId": model, "name": "Fixture model"}]},
              "configOptions": [{"id": "model", "options": [{"value": model, "name": "Fixture model"}]}],
              "modes": {"availableModes": [{"id": mode, "name": mode} for mode in ["fixture-agent", "default", "auto-high"]]}}
    if new:
        result["sessionId"] = sid
    elif vendor == "hermes":
        result["_meta"] = {"hermes": {"sessionProvenance": {"acpSessionId": sid}}}
    if not new and (root / "v3_wrong_identity").exists():
        result["sessionId"] = "unrelated-session"
    return result


for line in sys.stdin:
    request = json.loads(line)
    with (root / "v3_requests.jsonl").open("a") as log:
        log.write(json.dumps(request) + "\n")
    method = request.get("method")
    params = request.get("params", {})
    rejection_file = root / "v3_rpc_rejection.json"
    if rejection_file.exists():
        rejection = json.loads(rejection_file.read_text())
        if method == rejection["method"]:
            emit(id=request["id"], error=rejection["error"])
            continue
    if method is None:
        if request.get("id") == permission:
            emit(method="session/update", params={"sessionId": sid, "update": {
                "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "consent:" + json.dumps(request["result"])}}})
            finish()
        continue
    setup_needed = vendor == "hermes" and (root / "v3_setup_needed").exists()
    if method == "initialize":
        result = {"protocolVersion": 99 if (root / "v3_bad_protocol").exists() else 1,
                  "agentCapabilities": {"loadSession": True, "sessionCapabilities": {"list": {}}}}
        if vendor == "hermes":
            # Hermes always offers its setup, and its provider once one resolves.
            setup = {"id": "hermes-setup", "name": "Configure Hermes provider", "type": "terminal", "args": ["--setup"]}
            provider = {"id": "fixture-provider", "name": "fixture-provider runtime credentials"}
            result["authMethods"] = [setup] if setup_needed else [provider, setup]
    elif method == "session/list":
        if (root / "v3_list_endless").exists():
            result = {"sessions": [], "nextCursor": "new-cursor-" + str(request["id"])}
        elif (root / "v3_list_cycle").exists():
            result = {"sessions": [], "nextCursor": "repeated-cursor"}
        elif (root / "v3_list_unavailable").exists():
            result = {"sessions": []}
        elif (root / "v3_list_paginated").exists() and not params.get("cursor"):
            result = {"sessions": [{"sessionId": "other", "cwd": str(root)}], "nextCursor": "page-two"}
        else:
            result = {"sessions": [{"sessionId": key, "cwd": str(root / "wrong") if (root / "v3_list_wrong_cwd").exists() else cwd}
                                   for key, cwd in sessions.items()]}
    elif method == "session/new":
        if setup_needed:
            emit(id=request["id"], error={"code": -32603, "message": "storage_unavailable"})
            continue
        sid = "v3-" + str(uuid.uuid4())
        result = session_result(new=True)
    elif method == "session/load":
        sid = params["sessionId"]
        if sid not in sessions:
            if vendor == "hermes":
                result = {}
            else:
                emit(id=request["id"], error={"code": -32602, "message": "Unknown session identifier"})
                continue
        else:
            result = session_result()
    elif method in ("session/set_config_option", "session/set_model", "session/set_mode"):
        if params.get("value", params.get("modelId", params.get("modeId"))) == "unavailable":
            emit(id=request["id"], error={"code": -32602, "message": "Unavailable selection"})
            continue
        result = {}
    elif method == "session/prompt":
        active = request["id"]
        sessions[sid] = str(root)
        saved.write_text(json.dumps(sessions))
        text = active_text = params["prompt"][0]["text"]
        if "REJECT_BEFORE_RECEIPT" in text:
            emit(id=active, error={"code": -32001,
                "message": "Synthetic admission failure: " + os.environ.get("CCCC_TEST_ADMISSION_SECRET", "") + "界" * 1700,
                "data": {"private_detail": "DO_NOT_PUBLISH_PROVIDER_DATA"}})
            active = None
            continue
        if "DELAY_RECEIPT" in text:
            continue
        if text == "PERMISSION":
            permission = "v3-approval"
            emit(id=permission, method="session/request_permission", params={"sessionId": sid,
                "toolCall": {"title": "Fixture operation", "kind": "other"},
                "options": [{"optionId": "yes", "kind": "allow_once"}, {"optionId": "no", "kind": "reject_once"}]})
        else:
            emit(method="session/update", params={"sessionId": sid, "update": {
                "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "live:" + text}}})
            if text != "HOLD" and "KEEP_OPEN" not in text:
                finish()
        continue
    elif method == "session/cancel":
        if "UNKNOWN_AFTER_CANCEL" in active_text and active is not None:
            # Hermes's gateway lost the turn: its outcome is unknown.
            emit(id=active, error={"code": -32603,
                "message": "unknown_execution: fixture turn outcome is unknown"})
            active = permission = None
        elif "IGNORE_CANCEL" not in active_text:
            finish("cancelled")
        continue
    else:
        result = {}
    if "id" in request:
        emit(id=request["id"], result=result)
    if stall_stdin and method == "session/new":
        while True:
            time.sleep(60)
