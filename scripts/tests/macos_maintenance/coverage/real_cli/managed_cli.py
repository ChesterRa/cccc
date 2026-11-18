"""Exercise a released CLI through its managed CCCC path with a private model and retained MCP."""

import importlib.util
import json
import platform
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys

here = Path(__file__).resolve().parent
shared = here.parent / "portable/update_keep.py"
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
            runtime == "pi"
            and argv
            and (
                Path(argv[0]).name == "pi"
                or (
                    Path(argv[0]).name == "node"
                    and any(
                        "pi-coding-agent/" in a or a.endswith("/.bin/pi")
                        for a in argv[1:3]
                    )
                )
            )
        ):
            role = "provider"
        elif (
            runtime == "kilo"
            and len(argv) >= 2
            and Path(argv[0]).name in ("kilo", ".kilo")
        ):
            if argv[1] == "acp":
                role = "provider"
            elif argv[1] == "attach":
                role = "viewer"
            else:
                continue
        elif (
            runtime == "opencode"
            and len(argv) >= 2
            and Path(argv[0]).name == "opencode"
        ):
            if argv[1] == "serve":
                role = "provider"
            elif "--server" in argv or any(a.startswith("--server ") for a in argv):
                role = "viewer"
            else:
                continue
        elif (
            runtime == "kimi"
            and argv
            and (
                Path(argv[0]).name in ("kimi", "kimi-code")
                or (
                    Path(argv[0]).name == "node"
                    and len(argv) >= 2
                    and (
                        argv[1].endswith("/.bin/kimi")
                        or "/kimi-code/dist/main.mjs" in argv[1]
                    )
                )
            )
        ):
            role = "provider"
        elif (
            runtime == "copilot"
            and len(argv) >= 2
            and Path(argv[0]).name == "copilot"
            and "--acp" in command.stdout.split()
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


runtime, *files = sys.argv[1:]
if runtime not in ("pi", "kilo", "opencode", "copilot", "kimi") or len(files) != 5:
    raise SystemExit("expected RUNTIME OLD NEW MOCK PACKAGE_DIRECTORY OUTPUT")
old, new, mock, package, out = [Path(s).resolve() for s in files]
out.mkdir()
private = Path(os.environ["TMPDIR"]).parent
assert (
    private.name.startswith("cccc-isolated.")
    and Path(os.environ["HOME"]) == private / "home"
)
root = private / runtime
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
    timeout = 100 if args[:2] == ("actor", "start") else 30
    return json.loads(
        subprocess.check_output([program, *args], env=env, timeout=timeout)
    )


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
    profile_args = []
    if runtime == "pi":
        v = "1.5.2"
        executable = package / "node_modules/.bin/pi"
        assert executable.is_file()
        actual = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=15
        )
        assert actual.strip() == v, actual
        agent = root / "pi-profile"
        agent.mkdir()
        (agent / "models.json").write_text(
            json.dumps(
                {
                    "providers": {
                        "mock": {
                            "baseUrl": f"http://127.0.0.1:{port}/v1",
                            "api": "openai-completions",
                            "apiKey": "fixture",
                            "models": [{"id": "mock-cccc"}],
                        }
                    }
                }
            )
        )
        (agent / "settings.json").write_text(
            json.dumps({"defaultProvider": "mock", "defaultModel": "mock-cccc"})
        )
        profile_args = [
            "--command",
            str(executable),
            "--env",
            f"PI_CODING_AGENT_DIR={agent}",
        ]
    elif runtime == "kimi":
        v = "2.6.3"
        executable = package / "node_modules/.bin/kimi"
        version = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=20
        )
        assert v in version.split(), version
        cfg = Path(env["HOME"]) / ".kimi-code"
        cfg.mkdir(parents=True, exist_ok=True)
        (cfg / "config.toml").write_text(
            'default_model = "mock"\n'
            '[providers.mock]\ntype = "openai"\n'
            f'base_url = "http://127.0.0.1:{port}/v1"\napi_key = "fixture"\n'
            '[models.mock]\nprovider = "mock"\nmodel = "mock-cccc"\nmax_context_size = 128000\n'
        )
        profile_args = ["--runtime-mode", "acp", "--command", f"{executable} --yolo"]
    elif runtime == "copilot":
        v = "1.0.113"
        arch = {"x86_64": "x64", "arm64": "arm64", "aarch64": "arm64"}[
            platform.machine()
        ]
        executable = (
            package / f"node_modules/@github/copilot-{sys.platform}-{arch}/copilot"
        )
        version = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=20
        )
        assert version.splitlines()[0] == f"GitHub Copilot CLI {v}.", version
        cfg = Path(env["HOME"]) / ".copilot"
        cfg.mkdir(parents=True, exist_ok=True)
        (cfg / "providers.json").write_text(
            json.dumps(
                {
                    "providers": [
                        {
                            "name": "mock-local",
                            "type": "openai",
                            "baseUrl": f"http://127.0.0.1:{port}/v1",
                            "apiKey": "fixture",
                        }
                    ],
                    "models": [
                        {
                            "id": "mock-cccc",
                            "provider": "mock-local",
                            "maxPromptTokens": 128000,
                        }
                    ],
                }
            )
        )
        (cfg / "settings.json").write_text(
            json.dumps({"autoUpdate": False, "model": "mock-local/mock-cccc"})
        )
        profile_args = [
            "--runtime-mode",
            "acp",
            "--command",
            f"{executable} --allow-all",
        ]
    elif runtime == "opencode":
        v = "2.3.2"
        arch = {"x86_64": "x64", "arm64": "arm64", "aarch64": "arm64"}[
            platform.machine()
        ]
        executable = (
            package / f"node_modules/@opencode/cli-{sys.platform}-{arch}/bin/opencode"
        )
        assert executable.is_file()
        actual = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=20
        )
        assert actual.strip() == f"opencode v{v}", actual
        cfg = Path(env["HOME"]) / ".config/opencode"
        cfg.mkdir(parents=True, exist_ok=True)
        (cfg / "opencode.jsonc").write_text(
            json.dumps(
                {
                    "model": "mock/mock-cccc",
                    "providers": {
                        "mock": {
                            "name": "Mock",
                            "package": "@opencode/ai/providers/openai-compatible",
                            "settings": {
                                "baseURL": f"http://127.0.0.1:{port}/v1",
                                "apiKey": "fixture",
                            },
                            "models": {"mock-cccc": {"name": "Mock CCCC"}},
                        }
                    },
                }
            )
        )
        profile_args = ["--command", str(executable)]
    else:
        v = "7.8.27"
        executable = package / "node_modules/.bin/kilo"
        assert executable.is_file()
        actual = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=20
        )
        assert actual.strip() == v, actual
        cfg = Path(env["HOME"]) / ".config/kilo"
        cfg.mkdir(parents=True, exist_ok=True)
        (cfg / "kilo.json").write_text(
            json.dumps(
                {
                    "enabled_providers": ["openai-compatible"],
                    "model": "openai-compatible/mock-cccc",
                    "small_model": "openai-compatible/mock-cccc",
                    "provider": {
                        "openai-compatible": {
                            "options": {
                                "baseURL": f"http://127.0.0.1:{port}/v1",
                                "apiKey": "fixture",
                            },
                            "models": {"mock-cccc": {"name": "Mock CCCC"}},
                        }
                    },
                }
            )
        )
        profile_args = [
            "--command",
            str(executable),
            "--env",
            f"KILO_DB={root}/kilo.db",
        ]
    cli("actor", "add", "m1", "--group", gid, "--runtime", runtime, *profile_args)
    cli("actor", "start", "m1", "--group", gid)

    def ready_set():
        try:
            rows = process_set()
        except (ValueError, OSError, KeyError) as error:
            raise RuntimeError("could not identify owned processes") from error
        if set(rows) == (
            {"provider", "mcp", "viewer"}
            if runtime in ("kilo", "opencode")
            else {"provider", "mcp"}
        ):
            return rows

    before = u.wait_for(ready_set, "real node and member MCP", 25)
    if runtime == "kilo":

        def screen():
            response = u.daemon_request(
                home, "terminal_tail", by="user", group_id=gid, actor_id="m1", lines=48
            )
            return response["text"]

        (out / "before-terminal.txt").write_text(screen())

        def composer():
            visible = screen()
            (out / "current-terminal.txt").write_text(visible)
            return "ctrl+p commands" in visible and "Mock CCCC" in visible

        if os.environ.get("CCCC_TEST_KILO_EARLY") == "1":
            early = start("EARLY")
            u.wait_for(composer, "Kilo local model and composer are visible", 70)

            def accepted():
                return any(
                    r.get("kind") == "runtime.delivery"
                    and r["data"].get("source_event_id") == early
                    and r["data"].get("state") == "accepted"
                    for r in ledger()
                )

            u.wait_for(accepted, "first input accepted before any resend")
            (out / "early-diagnostic.json").write_text(
                json.dumps(
                    {
                        "event": early,
                        "model_requests": len(u.read_calls(model_log)),
                        "initial_screen_empty": not (out / "before-terminal.txt")
                        .read_text()
                        .strip(),
                        "early_reply": len(replies("EARLY", early)),
                    }
                )
            )
        else:
            u.wait_for(composer, "Kilo local model and composer are visible", 70)
    e1 = start("ALPHA")
    u.wait_for(lambda: replies("ALPHA", e1), "reply before Keep", 40)
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
    u.wait_for(lambda: replies("BRAVO", e2), "reply after Keep", 40)
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
    if runtime == "kilo" and os.environ.get("CCCC_TEST_KILO_EARLY") == "1":
        ev = json.loads((out / "early-diagnostic.json").read_text())
        ev["early_final_replies"] = len(replies("EARLY", ev["event"]))
        ev["decisions"] = [r.get("decision") for r in u.read_calls(model_log)]
        (out / "early-diagnostic.json").write_text(json.dumps(ev, indent=2))
    final = cli("daemon", "stop")["receipt"]
    assert [r["actor_id"] for r in final["confirmed_stopped"]] == ["m1"]
    u.wait_for(
        lambda: all(u.process_gone(identity[0]) for identity in before.values()),
        "normal stop of node and MCP",
    )
    result = dict(
        runtime=runtime,
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
    print(f"real {runtime} {v} kept its process and MCP, both replies matched")
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
