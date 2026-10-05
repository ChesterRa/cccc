#!/usr/bin/env python3
"""Offline official-protocol fixture: no provider network, credentials or commands."""
import json
import os
import sys
import uuid
from pathlib import Path

session_id = ""
active = None
pending_permission = None
home = Path(os.environ["HOME"])
sessions_path = home / "fixture_sessions.json"
sessions = json.loads(sessions_path.read_text()) if sessions_path.exists() else []
with open(Path.cwd() / "fixture_launch.jsonl", "a") as log:
    log.write(json.dumps(sys.argv) + "\n")


def emit(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def update(text):
    emit({"method": "session/update", "params": {
        "sessionId": session_id, "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": text}}}})


def finish(reason="end_turn"):
    global active, pending_permission
    if active is not None:
        emit({"id": active, "result": {"stopReason": reason}})
        active = None
    pending_permission = None


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    params = request.get("params", {})
    with open(Path.cwd() / "fixture_requests.jsonl", "a") as record:
        record.write(json.dumps(request) + "\n")
    if method is None:
        if request.get("id") == pending_permission:
            update("permission:" + json.dumps(request["result"]))
            finish()
        continue
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}
    elif method == "session/load" and ((Path.cwd() / "fixture_reject_load").exists() or params["sessionId"] not in sessions):
        emit({"id": request["id"], "error": {"code": -32000, "message": "Fixture session unavailable"}})
        continue
    elif method in ("session/new", "session/load"):
        session_id = params.get("sessionId") or "opaque-session~" + str(uuid.uuid4())
        result = {"sessionId": session_id,
                  "models": {"availableModels": [{"modelId": "fixture-model[fast=true]", "name": "fixture-model"}]},
                  "configOptions": [{"id": "model", "options": [{"value": "fixture-model-medium", "name": "Fixture model"}]}]}
        home.mkdir(parents=True, exist_ok=True)
    elif method in ("session/set_config_option", "session/set_model") and params.get("value", params.get("modelId")) == "unavailable":
        emit({"id": request["id"], "error": {"code": -32602, "message": "Unavailable model"}})
        continue
    elif method == "session/prompt":
        if active is not None:
            emit({"id": request["id"], "error": {"code": -32000, "message": "Concurrent prompt"}})
            continue
        active = request["id"]
        text = params["prompt"][0]["text"]
        if session_id not in sessions:
            sessions.append(session_id)
            sessions_path.write_text(json.dumps(sessions))
        if "DISCONNECT_BEFORE_RECEIPT" in text:
            sys.exit(0)
        if "REJECT_BEFORE_RECEIPT" in text:
            emit({"id": active, "error": {"code": -32000, "message": "Synthetic admission failure"}})
            active = None
            continue
        if "DELAY_RECEIPT" in text:
            continue
        # A replay and a different session must not admit this request.
        emit({"method": "session/update", "params": {"sessionId": session_id,
            "_meta": {"isReplay": True}, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "REPLAY"}}}})
        emit({"method": "session/update", "params": {"sessionId": "wrong-session",
            "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "FOREIGN"}}}})
        if "QUESTION" in text or "PLAN" in text:
            pending_permission = "interaction-opaque-id"
            if "QUESTION" in text:
                emit({"id": pending_permission, "method": "cursor/ask_question", "params": {
                    "toolCallId": "call-question", "title": "Choose a fixture", "questions": [
                        {"id": "q1", "prompt": "First?", "options": [{"id": "a", "label": "Alpha"}, {"id": "b", "label": "Beta"}]},
                        {"id": "q2", "prompt": "Second?", "allowMultiple": True, "options": [{"id": "c", "label": "Charlie"}, {"id": "d", "label": "Delta"}]}]}})
            else:
                emit({"id": pending_permission, "method": "cursor/create_plan", "params": {"toolCallId": "call-plan", "name": "Fixture plan", "overview": "Offline only", "plan": "1. Inspect fixture\n2. Finish", "todos": []}})
        elif "PERMISSION" in text:
            pending_permission = "permission-opaque-id"
            emit({"id": pending_permission, "method": "session/request_permission", "params": {
                "sessionId": session_id,
                "toolCall": {"title": "Synthetic command", "kind": "execute", "rawInput": {"command": "fixture only"}},
                "options": [{"optionId": "once", "kind": "allow_once"}, {"optionId": "never", "kind": "reject_once"}]}})
        else:
            update("live:" + text)
            if "FAIL_AFTER_RECEIPT" in text:
                emit({"id": active, "error": {"code": -32000, "message": "Synthetic provider failure"}})
                active = None
                continue
            if "HOLD" not in text:
                finish()
        continue
    elif method == "fixture/finish":
        finish()
        result = {}
    elif method == "session/cancel":
        finish("cancelled")
        continue
    else:
        result = {}
    emit({"id": request["id"], "result": result})
