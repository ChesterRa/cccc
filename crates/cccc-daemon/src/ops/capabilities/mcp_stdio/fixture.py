"""Protocol fixture: delayed responses are cancelled by premature stdin EOF."""
import json
import os
import sys
import threading
import time

mode, evidence = sys.argv[1:3]
lock = threading.Lock()
initialized = False


def record(event):
    with lock, open(evidence, "a", encoding="utf-8") as output:
        output.write(json.dumps(event) + "\n")


def respond(request, result):
    time.sleep(0.04)
    with lock:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)


record({"pid": os.getpid()})
if mode == "exit_required":
    value = os.environ.get("DATABASE_URL", "")
    record({"environment_matches_override": value == "fixture-invoker-override"})
    print("fixture connection failed: " + value, file=sys.stderr, flush=True)
    sys.exit(7)
if mode == "exit":
    print("installer fixture: enable prereleases; token=" + os.environ.get("FIXTURE_TOKEN", ""),
          file=sys.stderr, flush=True)
    sys.exit(7)

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    record({"method": method})
    if method == "initialize":
        if mode == "hang_init":
            continue
        result = {"protocolVersion": request["params"]["protocolVersion"],
                  "capabilities": {"tools": {}},
                  "serverInfo": {"name": "fixture", "version": "1"}}
    elif method == "notifications/initialized":
        initialized = True
        continue
    elif method == "tools/list":
        if not initialized:
            sys.exit(8)
        if request.get("params", {}).get("cursor") == "last":
            result = {"tools": [{"name": "tail", "inputSchema": {"type": "object"}}]}
        else:
            result = {"tools": [{"name": "echo", "inputSchema": {"type": "object"}}],
                      "nextCursor": "last"}
    elif method == "tools/call":
        record({"arguments": request.get("params", {}).get("arguments", {})})
        if not initialized:
            sys.exit(9)
        if mode == "hang_call":
            continue
        if mode == "wait_call":
            while not os.path.exists(sys.argv[3]):
                time.sleep(0.01)
        result = {"content": [{"type": "text", "text": "stdio-fixture-result"}],
                  "isError": mode == "tool_error"}
    else:
        continue
    # The old client closed stdin immediately after sending its batch. Daemon
    # response threads then never run, deterministically exposing that race.
    threading.Thread(target=respond, args=(request, result), daemon=True).start()

record({"eof": True})
if mode == "ignore_eof":
    time.sleep(60)
