#!/usr/bin/env python3
"""Official ACP shape fixture; no provider, network or real tool execution."""
import json
import os
import sys
import uuid
from pathlib import Path

root = Path.cwd()
home = Path(os.environ["HOME"])
home.mkdir(parents=True, exist_ok=True)
saved = home / "v3_sessions.json"
sessions = json.loads(saved.read_text()) if saved.exists() else {}
vendor = Path(sys.argv[0]).name
sid = ""
active = None
permission = None


def emit(**frame):
    print(json.dumps({"jsonrpc": "2.0", **frame}), flush=True)


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
    if method == "initialize":
        result = {"protocolVersion": 99 if (root / "v3_bad_protocol").exists() else 1,
                  "agentCapabilities": {"loadSession": True, "sessionCapabilities": {"list": {}}}}
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
        sid = "v3-" + str(uuid.uuid4())
        result = session_result(new=True)
        with (root / "v3_bindings.jsonl").open("a") as log:
            log.write(json.dumps({"vendor": vendor, "cwd": params["cwd"], "mcp": params["mcpServers"],
                                 "skip_configured_mcp": os.environ.get("HERMES_ACP_SKIP_CONFIGURED_MCP")}) + "\n")
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
        with (root / "v3_bindings.jsonl").open("a") as log:
            log.write(json.dumps({"vendor": vendor, "cwd": params["cwd"], "mcp": params["mcpServers"],
                                 "skip_configured_mcp": os.environ.get("HERMES_ACP_SKIP_CONFIGURED_MCP")}) + "\n")
    elif method in ("session/set_config_option", "session/set_model", "session/set_mode"):
        if params.get("value", params.get("modelId", params.get("modeId"))) == "unavailable":
            emit(id=request["id"], error={"code": -32602, "message": "Unavailable selection"})
            continue
        result = {}
    elif method == "session/prompt":
        active = request["id"]
        sessions[sid] = str(root)
        saved.write_text(json.dumps(sessions))
        text = params["prompt"][0]["text"]
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
            if text != "HOLD":
                finish()
        continue
    elif method == "session/cancel":
        finish("cancelled")
        continue
    else:
        result = {}
    if "id" in request:
        emit(id=request["id"], result=result)
