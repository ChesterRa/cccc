"""Local stand-in model for free real-CLI keep checks.

Speaks OpenAI Chat Completions (streaming and not) and serves a model list.
For a CCCC delivery that asks for a one-word reply, it calls the runtime's
CCCC reply tool with the event and Group taken from the delivered text; after
the tool result it ends the turn with a short text. Requests without tools
(titles, summaries) and anything else get a short text. Every request is
logged as one JSON line: path, sizes, tool names and the decision, never
headers or keys.

usage: python3 -I mock_model.py <port> <log file>
"""
import json
import re
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
LOG = sys.argv[2]
MODEL = "mock-cccc"
INSTRUCTION = re.compile(
    r"\[event_id=([0-9a-f]{32})[^\]]*\]:[^\n]*?"
    r"Reply to this message through CCCC with exactly one word: ([A-Z]+)"
)
GROUP = re.compile(r"group_id: (g_[0-9a-f]+)")
LOCK = threading.Lock()


def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(
            part["text"]
            for part in content
            if isinstance(part, dict) and isinstance(part.get("text"), str)
        )
    return ""


def tool_names(body):
    names = []
    for tool in body.get("tools") or []:
        if isinstance(tool, dict):
            name = (tool.get("function") or {}).get("name") or tool.get("name")
            if isinstance(name, str):
                names.append(name)
    return names


def tool_defs(body):
    defs = {}
    for tool in body.get("tools") or []:
        if isinstance(tool, dict):
            function = tool.get("function") or tool
            name = function.get("name")
            if isinstance(name, str):
                defs[name] = function.get("parameters") or function.get("input_schema") or {}
    return defs


def search_arguments(schema):
    """Arguments for a deferred-tool search, built from its own schema."""
    properties = (schema or {}).get("properties") or {}
    arguments = {}
    for name, spec in properties.items():
        kind = (spec or {}).get("type")
        lowered = name.lower()
        if kind == "string" and ("query" in lowered or lowered in ("q", "name", "pattern", "search")):
            arguments[name] = "cccc_message_reply"
        elif kind == "array" and "quer" in lowered:
            arguments[name] = ["cccc message reply"]
        elif kind in ("integer", "number") and name in ((schema or {}).get("required") or []):
            arguments[name] = 5
    return arguments


def use_arguments(schema, name, arguments):
    """Arguments for a generic tool-invocation tool, built from its own schema:
    the string property naming the tool gets the name, and the property for the
    tool's input gets the arguments (as an object or a JSON string, per its type)."""
    properties = (schema or {}).get("properties") or {}
    result = {}
    for key, spec in properties.items():
        kind = (spec or {}).get("type")
        kinds = kind if isinstance(kind, list) else [kind]
        lowered = key.lower()
        if "string" in kinds and "name" in lowered:
            result[key] = name
        elif any(word in lowered for word in ("arg", "input", "param")):
            result[key] = arguments if "object" in kinds or kind is None else json.dumps(arguments)
    return result


def decide(body):
    messages = [m for m in body.get("messages") or [] if isinstance(m, dict)]
    tools = tool_names(body)
    last = messages[-1] if messages else {}
    if not tools:
        return {"kind": "text", "text": "Done." if last.get("role") == "tool" else "OK."}
    # Runtimes may append their own reminders as later user messages, so the
    # delivery is the newest user message that carries the instruction.
    position, found = None, []
    for i in range(len(messages) - 1, -1, -1):
        if messages[i].get("role") == "user":
            found = INSTRUCTION.findall(text_of(messages[i].get("content")))
            if found:
                position = i
                break
    if position is None:
        return {"kind": "text", "text": "Done." if last.get("role") == "tool" else "OK."}
    event_id, word = found[-1]
    since = messages[position + 1:]
    # A word starting with SLOW holds the first answer of that turn for a while,
    # so a stop can be issued while the turn is still running.
    if word.startswith("SLOW") and not any(m.get("role") in ("assistant", "tool") for m in since):
        time.sleep(25)
    called = [
        (call.get("function") or {}).get("name") or ""
        for m in since if m.get("role") == "assistant"
        for call in m.get("tool_calls") or [] if isinstance(call, dict)
    ]
    calls = [
        call.get("function") or {}
        for m in since if m.get("role") == "assistant"
        for call in m.get("tool_calls") or [] if isinstance(call, dict)
    ]
    tried = [c for c in calls
             if (c.get("name") or "").endswith("cccc_message_reply")
             or ("cccc_message_reply" in str(c.get("arguments") or "") and (c.get("name") or "") in ("tool_call", "use_tool"))]
    last_result = next((text_of(m.get("content")) for m in reversed(since) if m.get("role") == "tool"), "")
    if tried and "not connected" not in last_result and '"error"' not in last_result:
        return {"kind": "text", "text": "Done."}
    tried_names = " ".join(str(c.get("arguments") or "") for c in tried)
    reply = next((name for name in tools if name.endswith("cccc_message_reply")), None)
    if reply is not None:
        groups = GROUP.findall("\n".join(text_of(m.get("content")) for m in messages))
        arguments = {"event_id": event_id, "text": word}
        if groups:
            arguments["group_id"] = groups[-1]
        return {"kind": "tool", "name": reply, "arguments": json.dumps(arguments)}
    # A bridge runtime (Hermes) keeps MCP tools behind tool_search and invokes
    # them through tool_call; take the exact name from the search result.
    if "tool_call" in tools:
        found_names = re.findall(r"[A-Za-z0-9_.:-]*cccc_message_reply",
                                 "\n".join(text_of(m.get("content")) for m in since if m.get("role") == "tool"))
        found_names = [n for n in dict.fromkeys(found_names) if f'"{n}"' not in tried_names]
        if found_names:
            groups = GROUP.findall("\n".join(text_of(m.get("content")) for m in messages))
            arguments = {"event_id": event_id, "text": word}
            if groups:
                arguments["group_id"] = groups[-1]
            return {"kind": "tool", "name": "tool_call",
                    "arguments": json.dumps({"calls": [{"name": found_names[0], "arguments": arguments}]})}
    # Grok keeps MCP tools behind search_tool and invokes them through use_tool;
    # the call is shaped from use_tool's own schema.
    if "use_tool" in tools:
        found_names = re.findall(r"[A-Za-z0-9_.:/-]*cccc_message_reply",
                                 "\n".join(text_of(m.get("content")) for m in since if m.get("role") == "tool"))
        found_names = [n for n in dict.fromkeys(found_names) if f'"{n}"' not in tried_names]
        if found_names:
            groups = GROUP.findall("\n".join(text_of(m.get("content")) for m in messages))
            arguments = {"event_id": event_id, "text": word}
            if groups:
                arguments["group_id"] = groups[-1]
            return {"kind": "tool", "name": "use_tool",
                    "arguments": json.dumps(use_arguments(tool_defs(body).get("use_tool"), found_names[0], arguments))}
    search = next((name for name in tools if name.replace("_", "").lower() in ("toolsearch", "searchtool")), None)
    if search is not None and search not in called:
        arguments = search_arguments(tool_defs(body).get(search))
        return {"kind": "tool", "name": search, "arguments": json.dumps(arguments)}
    return {"kind": "text", "text": "NO_CCCC_REPLY_TOOL"}


def last_user_tail(body):
    """The end of the newest user message: the delivered CCCC text in a test Group."""
    messages = [m for m in body.get("messages") or [] if isinstance(m, dict)]
    users = [m for m in messages if m.get("role") == "user"]
    return text_of(users[-1].get("content"))[-1500:] if users else ""


def last_tool_result(body):
    """The newest tool result, shortened: how a runtime answers a deferred-tool search."""
    messages = [m for m in body.get("messages") or [] if isinstance(m, dict)]
    tools = [m for m in messages if m.get("role") == "tool"]
    return text_of(tools[-1].get("content"))[:2000] if tools else ""


def log(record):
    with LOCK, open(LOG, "a", encoding="utf-8") as handle:
        handle.write(json.dumps(record) + "\n")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):  # keep stderr quiet; requests go to LOG
        pass

    def send_json(self, status, payload):
        data = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = self.path.split("?", 1)[0].rstrip("/")
        log({"t": time.strftime("%H:%M:%S"), "method": "GET", "path": path})
        if path.endswith("/models"):
            self.send_json(200, {"object": "list", "data": [
                {"id": name, "object": "model", "created": 0, "owned_by": "local-mock"}
                for name in (MODEL, MODEL + "-saved", MODEL + "-session")
            ]})
        else:
            self.send_json(404, {"error": {"message": "not found"}})

    def do_POST(self):
        path = self.path.split("?", 1)[0].rstrip("/")
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""
        try:
            body = json.loads(raw or b"{}")
        except ValueError:
            log({"t": time.strftime("%H:%M:%S"), "method": "POST", "path": path, "error": "bad json"})
            self.send_json(400, {"error": {"message": "bad json"}})
            return
        if not path.endswith("/chat/completions"):
            log({"t": time.strftime("%H:%M:%S"), "method": "POST", "path": path, "error": "unsupported"})
            self.send_json(404, {"error": {"message": "unsupported path"}})
            return
        decision = decide(body)
        tools = tool_names(body)
        log({
            "t": time.strftime("%H:%M:%S"),
            "method": "POST",
            "path": path,
            "model": body.get("model"),
            "stream": bool(body.get("stream")),
            "messages": len(body.get("messages") or []),
            "last_role": ((body.get("messages") or [{}])[-1] or {}).get("role"),
            "tools": len(tools),
            "cccc_tools": [name for name in tools if "cccc" in name][:40],
            "other_tools": [name for name in tools if "cccc" not in name][:20],
            "search_defs": {name: schema for name, schema in tool_defs(body).items()
                            if any(word in name.lower() for word in ("search", "tool_call", "describe", "use_tool"))},
            "last_tool_result": last_tool_result(body),
            "last_user_tail": last_user_tail(body),
            "decision": decision,
        })
        if body.get("stream"):
            self.stream(body, decision)
        else:
            self.send_json(200, self.completion(body, decision))

    def completion(self, body, decision):
        message = {"role": "assistant", "content": None}
        finish = "stop"
        if decision["kind"] == "tool":
            message["tool_calls"] = [{
                "id": "call_" + uuid.uuid4().hex[:24],
                "type": "function",
                "function": {"name": decision["name"], "arguments": decision["arguments"]},
            }]
            finish = "tool_calls"
        else:
            message["content"] = decision["text"]
        return {
            "id": "chatcmpl-" + uuid.uuid4().hex[:24],
            "object": "chat.completion",
            "created": int(time.time()),
            "model": body.get("model") or MODEL,
            "choices": [{"index": 0, "message": message, "finish_reason": finish}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
        }

    def stream(self, body, decision):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        ident = "chatcmpl-" + uuid.uuid4().hex[:24]
        created = int(time.time())
        model = body.get("model") or MODEL

        def chunk(delta, finish=None, usage=None):
            payload = {
                "id": ident,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
            }
            if usage is not None:
                payload["usage"] = usage
            self.wfile.write(b"data: " + json.dumps(payload).encode() + b"\n\n")
            self.wfile.flush()

        usage = {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        if decision["kind"] == "tool":
            call = "call_" + uuid.uuid4().hex[:24]
            chunk({"role": "assistant", "content": None, "tool_calls": [{
                "index": 0, "id": call, "type": "function",
                "function": {"name": decision["name"], "arguments": ""},
            }]})
            chunk({"tool_calls": [{"index": 0, "function": {"arguments": decision["arguments"]}}]})
            chunk({}, "tool_calls", usage)
        else:
            chunk({"role": "assistant", "content": ""})
            chunk({"content": decision["text"]})
            chunk({}, "stop", usage)
        if (body.get("stream_options") or {}).get("include_usage"):
            payload = {"id": ident, "object": "chat.completion.chunk", "created": created,
                       "model": model, "choices": [], "usage": usage}
            self.wfile.write(b"data: " + json.dumps(payload).encode() + b"\n\n")
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()
        self.close_connection = True


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
