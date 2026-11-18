#!/usr/bin/env python3
"""Terminal stand-in that keeps one `cccc mcp` child and calls a read-only CCCC tool.

Run as a Custom PTY member. It starts `$STANDIN_CCCC mcp` once, with this process's whole
environment (as a runtime that passes its environment to MCP servers), initializes it, then
calls `cccc_task` (action=list) every STANDIN_PERIOD seconds, one call at a time. Each call
is one JSON line in $STANDIN_LOG: ts, own pid, the MCP child's pid, ok, error text.
Never contacts a provider and never writes anything to CCCC.
"""
import json
import os
import subprocess
import sys
import threading
import time

log_path = os.environ["STANDIN_LOG"]
cccc = os.environ["STANDIN_CCCC"]
period = float(os.environ.get("STANDIN_PERIOD", "1"))
call_timeout = float(os.environ.get("STANDIN_CALL_TIMEOUT", "30"))


def record(**fields):
    fields["ts"] = time.time()
    fields["pid"] = os.getpid()
    with open(log_path, "a", encoding="utf-8") as handle:
        handle.write(json.dumps(fields) + "\n")


child = subprocess.Popen(
    [cccc, "mcp"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=open(log_path + ".mcp-stderr", "ab"),
    env=dict(os.environ),
)
replies = {}
ready = threading.Condition()


def reader():
    for line in child.stdout:
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if "id" in message:
            with ready:
                replies[message["id"]] = message
                ready.notify_all()


threading.Thread(target=reader, daemon=True).start()
next_id = 0


def request(method, params):
    global next_id
    next_id += 1
    wanted = next_id
    child.stdin.write((json.dumps({"jsonrpc": "2.0", "id": wanted, "method": method, "params": params}) + "\n").encode())
    child.stdin.flush()
    deadline = time.time() + call_timeout
    with ready:
        while wanted not in replies:
            left = deadline - time.time()
            if left <= 0:
                return None
            ready.wait(left)
        return replies.pop(wanted)


init = request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "standin", "version": "0"}})
record(ev="initialize", mcp_pid=child.pid, ok=bool(init and "result" in init), error=None if init and "result" in init else json.dumps(init)[:300])
child.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
child.stdin.flush()
print(f"stand-in {os.getpid()} with cccc mcp {child.pid}", flush=True)
while True:
    started = time.time()
    reply = request("tools/call", {"name": "cccc_task", "arguments": {"action": "list", "limit": 1}})
    if reply is None:
        record(ev="call", mcp_pid=child.pid, ok=False, error="no reply within timeout", took=time.time() - started)
    else:
        result = reply.get("result") or {}
        failed = "error" in reply or result.get("isError")
        text = json.dumps(reply.get("error") or result.get("content"))[:300] if failed else None
        record(ev="call", mcp_pid=child.pid, ok=not failed, error=text, took=time.time() - started)
    if child.poll() is not None:
        record(ev="mcp_exited", mcp_pid=child.pid, ok=False, error=f"exit {child.returncode}")
        sys.exit(1)
    time.sleep(period)
