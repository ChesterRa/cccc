"""Real pinned DeepSeek Agent Harness with a private Chat Completions server and retained MCP."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys

here = Path(__file__).resolve().parent
shared = here / "update_keep.py"
spec = importlib.util.spec_from_file_location("update_keep", shared)
u = importlib.util.module_from_spec(spec)
spec.loader.exec_module(u)


def process_set():
    result = {}
    addresses = list(home.glob("daemon/session-owners/*/address.json"))
    if not addresses:
        return result
    assert len(addresses) == 1, "expected the owner of the private home"
    owner = json.loads(addresses[0].read_text())["pid"]
    parent = {}
    for line in subprocess.check_output(
        ["ps", "-ax", "-o", "pid=,ppid="], text=True
    ).splitlines():
        child, ppid = map(int, line.split())
        parent.setdefault(ppid, []).append(child)
    snapshots = []
    pending = list(parent.get(owner, []))
    while pending:
        pid = pending.pop()
        pending.extend(parent.get(pid, []))
        command = subprocess.run(
            ["ps", "-ww", "-p", str(pid), "-o", "command="],
            text=True,
            capture_output=True,
        )
        if command.returncode:
            continue
        # ps renders the command for display. These private fixture paths have no spaces.
        argv = command.stdout.strip().split(maxsplit=3)
        if (
            len(argv) >= 2
            and Path(argv[0]).name == "node"
            and any("dsh-acp-demo" in value for value in argv[1:3])
        ):
            role = "provider"
        elif len(argv) >= 2 and Path(argv[0]).name == "cccc" and argv[1] == "mcp":
            role = "mcp"
        else:
            continue
        if role == "mcp" and sys.platform == "linux":
            context_keys = {b"CCCC_ACTOR_ID", b"CCCC_GROUP_ID", b"CCCC_HOME"}
            context = {}
            for item in Path("/proc", str(pid), "environ").read_bytes().split(b"\0"):
                key, sep, value = item.partition(b"=")
                if sep and key in context_keys:
                    context[key] = value
            assert context == {
                b"CCCC_ACTOR_ID": b"m1",
                b"CCCC_GROUP_ID": gid.encode(),
                b"CCCC_HOME": os.fsencode(home),
            }
        snapshots.append([pid, role])
        assert role not in result
        result[role] = u.process_identity(pid)
    (out / "owned-processes.json").write_text(
        json.dumps({"owner": owner, "snapshot": snapshots}, indent=2)
    )
    return result


old, new, mock, out = [Path(s).resolve() for s in sys.argv[1:5]]
out.mkdir()
private = Path(os.environ["TMPDIR"]).parent
assert (
    private.name.startswith("cccc-isolated.")
    and Path(os.environ["HOME"]) == private / "home"
)
root = private / "deepseek"
root.mkdir()
work = root / "work"
work.mkdir()
home = Path(os.environ["CCCC_HOME"])
program = root / "cccc"
shutil.copy2(old, program)
env = dict(
    os.environ,
    npm_config_registry="http://127.0.0.1:1/",
    npm_config_ignore_scripts="true",
)


def cli(*args):
    return u.run_json([program, *args], env)


with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
model_log = out / "model.jsonl"
model_output = (out / "mock.log").open("wb")
model = subprocess.Popen(
    [sys.executable, "-I", mock, str(port), model_log],
    stdout=model_output,
    stderr=subprocess.STDOUT,
    env=env,
)
hostlog = (out / "daemon.log").open("wb")
daemon = subprocess.Popen(
    [program, "daemon", "run"], env=env, stdout=hostlog, stderr=subprocess.STDOUT
)


def ledger():
    return u.read_calls(home / "groups" / gid / "ledger.jsonl")


def start(word):
    return cli(
        "send",
        "--group",
        gid,
        "--to",
        "m1",
        "Keep check. Reply to this message through CCCC with exactly one word: "
        + word
        + ". Do nothing else.",
    )["event"]["id"]


def replies(word, eid):
    return [
        e
        for e in ledger()
        if e["kind"] == "chat.message"
        and e["by"] == "m1"
        and e["data"].get("text", "").strip() == word
        and e["data"].get("reply_to") == eid
    ]


try:
    initial = u.wait_for(lambda: u.daemon_ping(home), "old daemon")
    gid = cli("attach", str(work))["group_id"]
    v = "0.1.0-rc.6"
    cache = Path(
        os.environ.get("CCCC_TEST_DEEPSEEK_CACHE", here.parent / "cache/deepseek" / v)
    )
    installed = home / "runtimes/deepseek" / v
    shutil.copytree(
        cache,
        installed,
        symlinks=True,
        ignore=shutil.ignore_patterns("profiles", "*.lock"),
    )
    cli(
        "actor",
        "add",
        "m1",
        "--group",
        gid,
        "--runtime",
        "deepseek",
        "--env",
        "DEEPSEEK_API_KEY=fixture",
        "--env",
        "DEEPSEEK_BASE_URL=http://127.0.0.1:" + str(port) + "/v1",
    )
    cli("actor", "start", "m1", "--group", gid)

    def ready_set():
        try:
            rows = process_set()
        except (ValueError, OSError, KeyError) as error:
            raise RuntimeError("could not identify owned processes") from error
        if set(rows) == {"provider", "mcp"}:
            return rows

    before = u.wait_for(ready_set, "real node and member MCP", 25)
    e1 = start("ALPHA")
    u.wait_for(lambda: replies("ALPHA", e1), "original MCP reply", 35)
    stopped = cli("daemon", "stop", "--keep-agents")["receipt"]
    assert daemon.wait(timeout=40) == 0
    assert process_set() == before and not stopped["unconfirmed"]
    candidate = root / "cccc.next"
    shutil.copy2(new, candidate)
    candidate.replace(program)
    subprocess.run(
        [program, "daemon", "start"],
        check=True,
        env=env,
        stdout=hostlog,
        stderr=hostlog,
        timeout=40,
    )
    resumed = u.wait_for(lambda: u.daemon_ping(home), "new daemon")
    assert (
        resumed["pid"] != initial["pid"]
        and resumed["build"]["source_id"] != initial["build"]["source_id"]
    )
    expected = [(gid, "m1", stopped["retained"][0]["generation"])]
    assert [
        (r["group_id"], r["actor_id"], r["generation"])
        for r in resumed["retained_agents"]
    ] == expected
    e2 = start("BRAVO")
    u.wait_for(lambda: replies("BRAVO", e2), "MCP reply through target daemon", 35)
    assert (
        process_set() == before
        and len(replies("ALPHA", e1)) == len(replies("BRAVO", e2)) == 1
    )

    def after_notice():
        return next(
            (
                row["last_tool_result"]
                for row in u.read_calls(model_log)
                if "CCCC source notice:" in (row.get("last_tool_result") or "")
            ),
            None,
        )

    notice = u.wait_for(after_notice, "notice delivered to real client")
    for fact in (
        initial["build"]["source_id"][:12],
        resumed["build"]["source_id"][:12],
        "Tool definitions remain those of this MCP build",
        "after active work finishes",
    ):
        assert fact in notice, fact
    final = cli("daemon", "stop")["receipt"]
    assert [r["actor_id"] for r in final["confirmed_stopped"]] == ["m1"]
    u.wait_for(
        lambda: all(u.process_gone(identity[0]) for identity in before.values()),
        "normal stop of node and MCP",
    )
    result = dict(
        version=v,
        actors=expected,
        pids=before,
        old=initial["build"],
        new=resumed["build"],
        reply_to=[e1, e2],
        authors=["m1", "m1"],
        clean_stop=True,
        model="loopback mock",
        notice_verified=True,
    )
    (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print("real DeepSeek kept its process and MCP, both replies matched")
finally:
    try:
        if (home / "daemon/ccccd.sock").exists():
            cli("daemon", "stop")
    except Exception:
        pass
    if daemon.poll() is None:
        daemon.terminate()
        daemon.wait(timeout=20)
    model.terminate()
    model.wait(timeout=10)
    if "gid" in globals():
        shutil.copy2(home / "groups" / gid / "ledger.jsonl", out / "ledger.jsonl")
    hostlog.close()
    model_output.close()
