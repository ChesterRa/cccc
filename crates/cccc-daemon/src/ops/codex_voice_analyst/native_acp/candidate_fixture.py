#!/usr/bin/env python3
"""Isolated ACP agent fixture; never starts a provider or executes a tool."""
import json
import os
import sys
import uuid
from pathlib import Path

root = Path.cwd()
home = Path(os.environ["HOME"])
home.mkdir(parents=True, exist_ok=True)
saved = home / "candidate_sessions.json"
sessions = json.loads(saved.read_text()) if saved.exists() else []
sid = ""
active = None
permission = None
mcp = []


def emit(**message):
    print(json.dumps({"jsonrpc": "2.0", **message}), flush=True)


def update(text):
    emit(method="session/update", params={"sessionId": sid, "update": {
        "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}})


def finish(reason="end_turn"):
    global active, permission
    if active is not None:
        emit(id=active, result={"stopReason": reason})
    active = permission = None


for line in sys.stdin:
    request = json.loads(line)
    with (root / "candidate_requests.jsonl").open("a") as record:
        record.write(json.dumps(request) + "\n")
    method = request.get("method")
    params = request.get("params", {})
    if method is None:
        if request.get("id") == permission:
            update("permission:" + json.dumps(request["result"]))
            finish()
        continue
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}
    elif method == "session/load" and (
            (root / "candidate_reject_load").exists() or params["sessionId"] not in sessions):
        emit(id=request["id"], error={"code": -32000, "message": "Session unavailable"})
        continue
    elif method in ("session/new", "session/load"):
        sid = params.get("sessionId") or "candidate-" + str(uuid.uuid4())
        mcp = params["mcpServers"]
        result = {"sessionId": sid, "models": {"availableModels": [
            {"modelId": "model-auggie", "name": "Fixture model"}]}, "configOptions": [
            {"id": "model", "options": [{"value": "model-kimi", "name": "Fixture model"}]}]}
        # Auggie's startup index consent must not authorize workspace upload.
        if Path(sys.argv[0]).name == "auggie" and not (root / "candidate_late_index").exists():
            emit(id="index-startup", method="session/request_permission", params={
                "sessionId": sid, "toolCall": {"title": "Index workspace", "kind": "other"},
                "options": [{"optionId": "yes", "kind": "allow_once"},
                            {"optionId": "no", "kind": "reject_once"}]})
    elif method in ("session/set_config_option", "session/set_model"):
        if params.get("value", params.get("modelId")) == "unavailable":
            emit(id=request["id"], error={"code": -32602, "message": "Unavailable model"})
            continue
        result = {}
    elif method == "session/prompt":
        active = request["id"]
        text = params["prompt"][0]["text"]
        if sid not in sessions:
            sessions.append(sid)
            saved.write_text(json.dumps(sessions))
        if "PERMISSION" in text or "LATE_INDEX" in text:
            permission = "candidate-late-index" if "LATE_INDEX" in text else "candidate-permission"
            emit(id=permission, method="session/request_permission", params={
                "sessionId": sid, "toolCall": {"title": "Index workspace" if "LATE_INDEX" in text else "Fixture operation", "kind": "other"},
                "options": [{"optionId": "yes", "kind": "allow_once"},
                            {"optionId": "no", "kind": "reject_once"}]})
        else:
            update("live:" + text)
            if "HOLD" not in text:
                finish()
        continue
    elif method == "session/cancel":
        finish("cancelled")
        continue
    else:
        result = {}
    if "id" in request:
        emit(id=request["id"], result=result)
