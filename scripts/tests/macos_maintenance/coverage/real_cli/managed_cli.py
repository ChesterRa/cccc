"""Exercise a released CLI through its managed CCCC path with a private model and retained MCP."""

from contextlib import closing
import importlib.util
import json
import platform
import os
from pathlib import Path
import shutil
import signal
import socket
import sqlite3
import time
import subprocess
import sys

here = Path(__file__).resolve().parent
shared = here.parent / "portable/update_keep.py"
spec = importlib.util.spec_from_file_location("update_keep", shared)
u = importlib.util.module_from_spec(spec)
spec.loader.exec_module(u)


def tree_roots():
    """The session owner, for Claude Code also its own background supervisor, and
    for Hermes the gateway that holds the member's MCP server."""
    addresses = list(home.glob("daemon/session-owners/*/address.json"))
    if not addresses:
        return []
    assert len(addresses) == 1, "expected the owner of the private home"
    roots = [json.loads(addresses[0].read_text())["pid"]]
    if runtime == "claude":
        # CCCC holds only the attach viewer. Claude Code's own background
        # supervisor holds the session, its terminal host and its MCP servers.
        status = Path(os.environ["CLAUDE_CONFIG_DIR"]) / "daemon.status.json"
        if not status.is_file():
            return []
        roots.append(json.loads(status.read_text())["supervisorPid"])
    if runtime == "hermes":
        # Hermes's gateway starts the member's MCP server; the ACP client only asks
        # for it. On Linux the ACP client starts the gateway as its child, below the
        # owner. On macOS with a GUI session Hermes submits the gateway to launchd,
        # so the gateway and the MCP server sit outside the owner's tree.
        roots.extend(pid for pid in hermes_gateways() if pid not in roots)
    return roots


def hermes_process(command):
    """Whether a command line runs from this case's own Hermes installation."""
    return str(Path(os.environ["HOME"]) / ".hermes") + os.sep in command


def ps_rows():
    """[pid, ppid, command] of every process. Hermes passes its launcher a
    script with newlines; a command line that ps prints over several lines is
    joined back to its row."""
    rows = []
    for line in subprocess.run(
        ["ps", "-ax", "-o", "pid=,ppid=,command="], text=True, capture_output=True
    ).stdout.splitlines():
        fields = line.split(maxsplit=2)
        if len(fields) >= 2 and fields[0].isdigit() and fields[1].isdigit():
            rows.append([int(fields[0]), int(fields[1]), fields[2] if len(fields) > 2 else ""])
        elif rows:
            rows[-1][2] += " " + line.strip()
    return rows


def hermes_gateways(provider=None):
    """Hermes's gateway runs the session's turns. The member's ACP client starts
    it, and it outlives that client. Given the client's PID, its children that
    run the gateway; otherwise those that run from this case's installation."""
    return [
        pid
        for pid, ppid, command in ps_rows()
        if "gateway run" in " ".join(command.split()[-6:])
        and (ppid == provider if provider else hermes_process(command))
    ]


def hermes_gateway_identities(provider):
    """Every Hermes gateway this case can see, by identity: the ACP client's
    children that run one, and any that runs from this case's installation."""
    pids = set(hermes_gateways(provider)) | set(hermes_gateways())
    return sorted(u.process_identity(pid) for pid in pids)


def check_one_gateway(gateway):
    """One gateway served both turns: the update neither replaced it nor added
    another."""
    assert len(gateway["at_ready"]) == 1, gateway
    assert gateway["at_ready"] == gateway["after_first_turn"] == gateway["after_second_turn"], gateway


def end_hermes_gateways(recorded, wait=15):
    """Ends the gateways this case recorded and any that runs from its own
    installation. A process is signalled only while it is still the instance
    whose identity was taken, the delayed SIGKILL included, so a PID that a new
    process has taken over is left alone."""

    def same_instance(identity):
        try:
            return u.process_identity(identity[0]) == identity
        except Exception:
            return False

    def signal_same(identities, number):
        sent = []
        for identity in identities:
            if same_instance(identity):
                try:
                    os.kill(identity[0], number)
                    sent.append(identity[0])
                except ProcessLookupError:
                    pass
        return sent

    targets = []
    for identity in recorded:
        if identity not in targets:
            targets.append(identity)
    for pid in hermes_gateways():
        try:
            identity = u.process_identity(pid)
        except Exception:
            continue
        if identity not in targets:
            targets.append(identity)
    terminated = signal_same(targets, signal.SIGTERM)
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline and any(same_instance(identity) for identity in targets):
        time.sleep(0.2)
    return {"terminated": terminated, "killed_after_15_s": signal_same(targets, signal.SIGKILL)}


def roles_ready(found, required, optional=frozenset()):
    """Whether the processes found hold every required role and no role beyond
    those and the optional ones."""
    return required <= set(found) <= required | optional


def process_set():
    result = {}
    roots = tree_roots()
    if not roots:
        return result
    owner = roots[0]
    parent, up = {}, {}
    for line in subprocess.check_output(
        ["ps", "-ax", "-o", "pid=,ppid="], text=True
    ).splitlines():
        child, ppid = map(int, line.split())
        parent.setdefault(ppid, []).append(child)
        up[child] = ppid
    snapshots = []
    pending = [child for root in roots for child in parent.get(root, [])]
    seen = set()
    while pending:
        pid = pending.pop()
        # On Linux a Hermes gateway is a root and also below the owner: walk it once.
        if pid in seen:
            continue
        seen.add(pid)
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
        elif runtime == "claude" and len(argv) >= 2 and Path(argv[0]).name == "claude":
            if argv[1] == "attach":
                role = "viewer"
            else:
                # The supervisor keeps a spare terminal host ready. A session
                # that claims it keeps the spare's arguments, so the session's
                # host and program are found above its MCP server, below.
                continue
        elif (
            runtime == "cline"
            and len(argv) >= 2
            and Path(argv[0]).name == "cline"
            and argv[1] == "--tui"
        ):
            # Cline starts its detached shared Hub from the same executable.
            role = "provider"
        elif runtime == "grok" and len(argv) >= 2 and Path(argv[0]).name == "grok":
            # CCCC runs Grok as a leader that runs the agent, an ACP client and a
            # terminal, all connected to the leader's socket.
            words = command.stdout.split()
            if argv[1] == "agent" and "leader" in words:
                role = "provider"
            elif argv[1] == "agent" and "stdio" in words:
                role = "acp"
            elif "--resume" in words:
                role = "viewer"
            else:
                continue
        elif runtime == "antigravity" and argv and Path(argv[0]).name == "agy":
            if "--bg-updater" in command.stdout.split():
                # Antigravity's own updater, which it starts beside the session
                # and which may come and go; not the member's session.
                continue
            role = "provider"
        elif runtime == "hermes" and hermes_process(command.stdout):
            # CCCC runs Hermes as an ACP client and Hermes's own terminal attached to
            # the same session. Hermes starts both through a Python launcher, so the
            # program name says nothing; the member's arguments come last. The
            # terminal draws through a node child. The ACP client also starts
            # Hermes's gateway, which outlives it; that is recorded on its own.
            words = command.stdout.split()
            if words[-1] == "acp":
                role = "provider"
            elif words[-2:-1] == ["--resume"]:
                role = "viewer"
            elif Path(words[0]).name == "node" and words[-1].endswith("ui-tui/dist/entry.js"):
                role = "viewer-ui"
            else:
                continue
        elif runtime == "codex" and argv and Path(argv[0]).name == "codex":
            # CCCC runs Codex as an app server and a terminal attached to it.
            words = command.stdout.split()
            if "--remote" in words:
                role = "viewer"
            elif "app-server" in words:
                role = "provider"
            else:
                continue
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
        snapshots.append([pid, role, command.stdout.strip()[:240]])
        assert role not in result, f"two {role} processes: {snapshots}"
        result[role] = u.process_identity(pid)
    if runtime == "claude" and "mcp" in result:
        pid = next(row[0] for row in snapshots if row[1] == "mcp")
        for role, word in (("provider", None), ("terminal-host", "bg-pty-host")):
            pid = up.get(pid)
            command = subprocess.run(
                ["ps", "-ww", "-p", str(pid), "-o", "command="],
                text=True,
                capture_output=True,
            ).stdout.strip()
            argv = command.split(maxsplit=2)
            assert argv and Path(argv[0]).name == "claude", (role, pid, command)
            assert word is None or argv[1:2] == [word], (role, pid, command)
            snapshots.append([pid, role, command[:240]])
            result[role] = u.process_identity(pid)
    # Each role is a process of its own; a derived role must not land on one
    # already named, such as an attach viewer that happens to start the MCP.
    assert len({row[0] for row in snapshots}) == len(snapshots), snapshots
    (out / "owned-processes.json").write_text(
        json.dumps(
            {
                "owner": owner,
                "roots": roots,
                # The parent of each root and each named process, such as a
                # gateway that launchd runs.
                "parents": {
                    str(pid): up.get(pid) for pid in [*roots, *(row[0] for row in snapshots)]
                },
                "snapshot": snapshots,
            },
            indent=2,
        )
    )
    return result


def write_process_dump(name):
    """Saves the fixture's processes as "pid ppid command": everything below the
    tree roots, whatever its command line, and anything else that runs from the
    fixture's private root."""
    rows = ps_rows()
    below = set(tree_roots())
    for _ in rows:
        below |= {pid for pid, ppid, _ in rows if ppid in below}
    (out / name).write_text(
        "".join(
            f"{pid} {ppid} {command}\n"
            for pid, ppid, command in rows
            if pid in below or str(private) in command
        )
    )


runtime, *files = sys.argv[1:]
if runtime not in (
    "pi", "kilo", "opencode", "copilot", "kimi", "claude", "codex", "cline", "grok", "antigravity", "hermes"
) or len(files) != 5:
    raise SystemExit("expected RUNTIME OLD NEW MOCK PACKAGE_DIRECTORY OUTPUT")
old, new, mock, package, out = [Path(s).resolve() for s in files]
out.mkdir()
private = Path(os.environ["TMPDIR"]).parent
assert (
    private.name.startswith("cccc-isolated.")
    and Path(os.environ["HOME"]) == private / "home"
)
root = private / f"{runtime}-case"
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


timings = {}
case_started = time.monotonic()


def mark(step):
    """Seconds from the case's start to the end of each step, saved as it goes,
    so a slow or failed run shows where its time went."""
    timings[step] = round(time.monotonic() - case_started, 1)
    (out / "timings.json").write_text(json.dumps(timings, indent=2) + "\n")


def cli(*args):
    if args[:2] == ("actor", "start"):
        # Hermes's start runs several Hermes commands before its session opens.
        timeout = 180 if runtime == "hermes" else 100
    else:
        timeout = 30
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


def reply_texts(eid):
    """Every reply the member posted to one message, whatever its text."""
    return [
        e["data"].get("text", "").strip()
        for e in ledger()
        if e["kind"] == "chat.message"
        and e["by"] == "m1"
        and e["data"].get("reply_to") == eid
    ]


def kilo_delivery_evidence(eid):
    events = ledger()
    requests = u.read_calls(model_log)
    report = {
        "source_event": eid,
        "delivery_states": [
            e["data"].get("state")
            for e in events
            if e["kind"] == "runtime.delivery" and e["data"].get("source_event_id") == eid
        ],
        "provider_requests": len(requests),
        "requests_containing_event_id": sum(eid in (r.get("last_user_tail") or "") for r in requests),
        "tool_calls_targeting_event": sum(
            eid in str((r.get("decision") or {}).get("arguments", "")) for r in requests
        ),
        "member_replies_to_event": sum(
            e["kind"] == "chat.message" and e["by"] == "m1" and e["data"].get("reply_to") == eid
            for e in events
        ),
    }
    try:
        visible = u.daemon_request(home, "terminal_tail", by="user", group_id=gid, actor_id="m1", lines=48)["text"]
        report["screen_nonempty"] = bool(visible.strip())
        report["screen_contains_event_id"] = eid in visible
        report["screen_mentions_mock_model"] = "Mock CCCC" in visible
        report["screen_has_commands_hint"] = "ctrl+p commands" in visible
    except (OSError, ValueError, KeyError):
        report["screen_available"] = False
    return report


def wait_for_reply(word, eid, label):
    try:
        u.wait_for(lambda: replies(word, eid), label, 90)
    except TimeoutError:
        # What the member's terminal showed, such as a question that appears only
        # on this machine. Best effort: it must not replace the timeout.
        try:
            (out / f"reply-timeout-{word}-terminal.txt").write_text(
                u.daemon_request(
                    home, "terminal_tail", by="user", group_id=gid, actor_id="m1", lines=48
                )["text"]
            )
        except Exception:
            pass
        if runtime == "kilo":
            print("Kilo delivery evidence:", json.dumps(kilo_delivery_evidence(eid)), flush=True)
        raise


def verify_kilo_turns(inputs, submitted_ms=None, cold_screen_empty=None):
    binding = home / "groups" / gid / "state" / "runtime_sessions" / "m1.json"
    sid = json.loads(binding.read_text())["provider_session_id"]
    assert isinstance(sid, str) and sid.startswith("ses_")
    # This private database belongs to the member. Count actual user parts in any
    # session, then verify the one match belongs to the session recorded by CCCC.
    uri = (root / "kilo.db").resolve().as_uri() + "?mode=ro"
    with closing(sqlite3.connect(uri, uri=True)) as db:
        parts = db.execute(
            "SELECT m.session_id, p.session_id, json_extract(p.data, '$.text') "
            "FROM message m JOIN part p ON p.message_id = m.id "
            "WHERE json_extract(m.data, '$.role') = 'user' AND json_extract(p.data, '$.type') = 'text'"
        ).fetchall()
    turns = []
    rows = ledger()
    for word, event in inputs:
        matches = [(m, p) for m, p, text in parts if text and event in text]
        deliveries = [row["data"] for row in rows if row["kind"] == "runtime.delivery"
                      and row["data"].get("source_event_id") == event]
        states = [record["state"] for record in deliveries]
        # A zero-byte attempt may be safely released and reclaimed; each such release is explicit.
        claimed, accepted = False, False
        for record in deliveries:
            if record["state"] == "claimed" and not claimed and not accepted:
                claimed = True
            elif record["state"] == "failed" and claimed and record.get("retryable") is True:
                claimed = False
            elif record["state"] == "accepted" and claimed and not accepted:
                claimed, accepted = False, True
            else:
                raise AssertionError((word, deliveries))
        member_replies = [row["data"].get("text", "").strip() for row in rows
                          if row["kind"] == "chat.message" and row["by"] == "m1"
                          and row["data"].get("reply_to") == event]
        turns.append({"word": word, "event": event, "native_matches": matches,
                      "states": states, "accepted": accepted, "replies": member_replies})
    (out / "native-turns.json").write_text(json.dumps(
        {"session": sid, "early_send_completed_ms_after_start": submitted_ms,
         "initial_screen_empty": cold_screen_empty, "turns": turns}, indent=2
    ) + "\n")
    assert all(turn["native_matches"] == [(sid, sid)] and turn["accepted"]
               and turn["replies"] == [turn["word"]] for turn in turns), turns
    if submitted_ms is not None:
        assert cold_screen_empty, "delivery passed; the cold startup window was not observed"


normal_stop_done = False
try:
    initial = u.wait_for(lambda: u.daemon_ping(home), "old daemon")
    mark("old daemon answered")
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
    elif runtime == "claude":
        v = "2.1.327"
        arch = {"x86_64": "x64", "arm64": "arm64", "aarch64": "arm64"}[
            platform.machine()
        ]
        # npm's bin entry is a placeholder until a postinstall script, which the
        # fixture does not run, copies in this platform's native program.
        executable = (
            package / f"node_modules/@anthropic-ai/claude-code-{sys.platform}-{arch}/claude"
        )
        actual = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=20
        )
        assert actual.split()[:1] == [v], actual
        cfg = Path(env["CLAUDE_CONFIG_DIR"])
        cfg.mkdir(parents=True, exist_ok=True)
        # The answers a person gives once: onboarding done, this project trusted,
        # and the warning for CCCC's default --dangerously-skip-permissions accepted.
        (cfg / ".claude.json").write_text(
            json.dumps(
                {
                    "hasCompletedOnboarding": True,
                    "bypassPermissionsModeAccepted": True,
                    "projects": {str(work): {"hasTrustDialogAccepted": True}},
                }
            )
        )
        (cfg / "settings.json").write_text(
            json.dumps({"skipDangerousModePermissionPrompt": True})
        )
        profile_args = [
            "--command",
            f"{executable} --dangerously-skip-permissions",
            "--env",
            f"ANTHROPIC_BASE_URL=http://127.0.0.1:{port}",
            "--env",
            "ANTHROPIC_AUTH_TOKEN=fixture",
            "--env",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1",
            "--env",
            "DISABLE_AUTOUPDATER=1",
        ]
    elif runtime == "codex":
        v = "0.170.0"
        # npm's launcher, which runs this platform's native program as its child.
        executable = package / "node_modules/.bin/codex"
        actual = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=20
        )
        assert actual.strip() == f"codex-cli {v}", actual
        cfg = Path(env["CODEX_HOME"])
        cfg.mkdir(parents=True, exist_ok=True)
        (cfg / "config.toml").write_text(
            'model = "mock-cccc"\nmodel_provider = "mock"\n'
            '[model_providers.mock]\nname = "Mock"\n'
            f'base_url = "http://127.0.0.1:{port}/v1"\nwire_api = "responses"\n'
            f'[projects."{work}"]\ntrust_level = "trusted"\n'
        )
        # CCCC's default Codex command line, with the installed launcher.
        profile_args = [
            "--command",
            f"{executable} -c check_for_update_on_startup=false"
            " -c shell_environment_policy.inherit=all"
            " --dangerously-bypass-approvals-and-sandbox --search",
        ]
    elif runtime == "cline":
        v = "3.0.74"
        arch = {"x86_64": "x64", "arm64": "arm64", "aarch64": "arm64"}[
            platform.machine()
        ]
        # The platform's native program. npm's `cline` bin entry may resolve to it
        # or to a node wrapper that starts it, depending on how npm links the two.
        executable = package / f"node_modules/@cline/cli-{sys.platform}-{arch}/bin/cline"
        actual = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=30
        )
        assert actual.strip() == v, actual
        settings = Path(env["HOME"]) / ".cline/data/settings"
        settings.mkdir(parents=True, exist_ok=True)
        (settings / "providers.json").write_text(
            json.dumps(
                {
                    "version": 1,
                    "lastUsedProvider": "openai-compatible",
                    "modes": {},
                    "providers": {
                        "openai-compatible": {
                            "settings": {
                                "provider": "openai-compatible",
                                "apiKey": "fixture",
                                "model": "mock-cccc",
                                "baseUrl": f"http://127.0.0.1:{port}/v1",
                            },
                            # Without it Cline 3.0.74 drops the entry and opens
                            # its own sign-in screen instead.
                            "updatedAt": "2026-11-18T00:00:00.000Z",
                            "tokenSource": "manual",
                        }
                    },
                }
            )
        )
        # The notices a person dismisses once on the first start.
        (settings / "cli-notices.json").write_text(
            json.dumps(
                {
                    "shown": {
                        "cline-cli-cline-pass-intro": True,
                        "cline-cli-desktop-launch": True,
                    }
                }
            )
        )
        profile_args = [
            "--command",
            f"{executable} --tui --auto-approve true",
            # CCCC registers its MCP entry with `cline mcp add`, found on the
            # member's PATH, where a global install would put it.
            "--env",
            f"PATH={executable.parent}{os.pathsep}{env['PATH']}",
        ]
    elif runtime == "grok":
        v = "1.0.88"
        # The official program, installed under its own name: CCCC runs Grok's
        # leader, ACP client and terminal only from an executable named grok.
        executable = package / "bin/grok"
        actual = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=30
        )
        assert actual.split()[:2] == ["grok", v], actual
        cfg = Path(env["HOME"]) / ".grok"
        cfg.mkdir(parents=True, exist_ok=True)
        (cfg / "config.toml").write_text(
            '[model.mock]\nmodel = "mock-cccc"\n'
            f'base_url = "http://127.0.0.1:{port}/v1"\n'
            'api_key = "fixture"\napi_backend = "chat_completions"\n\n'
            '[models]\ndefault = "mock"\n'
        )
        profile_args = ["--command", f"{executable} --always-approve"]
    elif runtime == "antigravity":
        v = "1.5.11"
        executable = package / "bin/agy"
        actual = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=30
        )
        assert actual.strip() == v, actual
        cfg = Path(env["HOME"]) / ".gemini/antigravity-cli"
        (cfg / "cache").mkdir(parents=True, exist_ok=True)
        # An isolated configuration that has finished Antigravity's first start:
        # its onboarding (a terms and data-use page, then four setup steps) is
        # marked done. It does not record an answer to the data-use question;
        # every model request goes to the loopback mock.
        (cfg / "settings.json").write_text(
            json.dumps(
                {
                    "modelProvider": "gemini",
                    "showFeedbackSurvey": False,
                    "trustedWorkspaces": [str(work)],
                    "enableTelemetry": False,
                }
            )
        )
        (cfg / "cache/onboarding.json").write_text(
            json.dumps(
                {
                    "consumerOnboardingComplete": True,
                    "enterpriseOnboardingComplete": False,
                    "onboardingComplete": True,
                }
            )
        )
        (cfg / "jetski_state.pbtxt").write_text(
            "post_onboarding: {\n"
            + "".join(
                f"  completed_steps: POST_ONBOARDING_STEP_TYPE_{step}\n"
                for step in (
                    "MANAGER_WELCOME",
                    "USAGE_MODE",
                    "AGENT_CONFIGURATION",
                    "ADD_WORKSPACE",
                )
            )
            + "}\n"
        )
        profile_args = [
            "--command",
            f"{executable} --dangerously-skip-permissions",
            "--env",
            "GEMINI_API_KEY=fixture",
            "--env",
            f"GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:{port}",
        ]
    elif runtime == "hermes":
        v = "0.23.2"
        # What run.sh's official install put on the user's PATH. Its first start
        # after the install also finishes the install's own setup.
        executable = Path(env["HOME"]) / ".local/bin/hermes"
        actual = subprocess.check_output(
            [executable, "--version"], env=env, text=True, timeout=180
        )
        assert actual.split()[:3] == ["Hermes", "Agent", f"v{v}"], actual
        # The installed config's version, so Hermes migrates nothing; the mock
        # model; and the answer to the one question a fresh home asks, whether
        # to share usage counters with Nous: "No thanks".
        (Path(env["HOME"]) / ".hermes/config.yaml").write_text(
            "_config_version: 53\n"
            "model:\n  default: mock-cccc\n  provider: custom\n"
            f"  base_url: http://127.0.0.1:{port}/v1\n  api_key: fixture\n"
            "telemetry:\n  shared_metrics:\n    enabled: false\n    send: false\n"
            "    offer_version: 2\n"
        )
        profile_args = ["--command", f"{executable} --tui --yolo"]
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
    mark("runtime version checked and configured")
    cli("actor", "add", "m1", "--group", gid, "--runtime", runtime, *profile_args)
    kilo_mode = os.environ.get("CCCC_TEST_KILO_EARLY", "0")
    check_kilo_first = runtime == "kilo" and kilo_mode in ("1", "only")
    if kilo_mode == "only":
        assert runtime == "kilo" and old == new, "cold-only exercises one specified candidate"
    cli("actor", "start", "m1", "--group", gid)
    mark("member start returned")
    if check_kilo_first:
        # Send before reading the terminal or enumerating the new viewer's processes.
        started = time.monotonic()
        early = start("EARLY")
        submitted_ms = round((time.monotonic() - started) * 1000, 1)
        first_screen = u.daemon_request(home, "terminal_tail", by="user", group_id=gid, actor_id="m1", lines=48)["text"]
        (out / "screen-immediately-after-send.txt").write_text(first_screen)
        cold_screen_empty = not first_screen.strip()

    expected_roles = {
        "kilo": {"provider", "viewer", "mcp"},
        "opencode": {"provider", "viewer", "mcp"},
        "codex": {"provider", "viewer", "mcp"},
        "claude": {"provider", "viewer", "terminal-host", "mcp"},
        "grok": {"provider", "acp", "viewer", "mcp"},
        "hermes": {"provider", "viewer", "viewer-ui", "mcp"},
    }.get(runtime, {"provider", "mcp"})
    # Hermes starts the member's MCP server when the session's first turn runs.
    first_turn_roles = {"mcp"} if runtime == "hermes" else set()

    def ready_set(roles, optional=frozenset()):
        try:
            rows = process_set()
        except (ValueError, OSError, KeyError) as error:
            raise RuntimeError("could not identify owned processes") from error
        if roles_ready(rows, roles, optional):
            return rows

    try:
        before = u.wait_for(
            # An MCP that starts before the first turn is accepted, not required.
            lambda: ready_set(expected_roles - first_turn_roles, first_turn_roles),
            "real node and member MCP",
            90 if runtime == "hermes" else 60,
        )
    except TimeoutError:
        # Keep what the client shows, such as a question asked on first start.
        (out / "ready-timeout-terminal.txt").write_text(
            u.daemon_request(
                home, "terminal_tail", by="user", group_id=gid, actor_id="m1", lines=48
            )["text"]
        )
        write_process_dump("ready-timeout-processes.txt")
        raise
    mark("member processes ready")
    if runtime == "hermes":
        # Hermes's gateway runs the turns. It is not counted with the kept
        # processes, because it outlives the member's normal stop by design. Each
        # snapshot lists every gateway this case can see, by identity.
        gateway = {
            "at_ready": u.wait_for(
                lambda: hermes_gateway_identities(before["provider"][0]), "Hermes gateway", 30
            )
        }
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

        if check_kilo_first:
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
                        "initial_screen_empty": cold_screen_empty,
                        "early_reply": len(replies("EARLY", early)),
                    }
                )
            )
        else:
            u.wait_for(composer, "Kilo local model and composer are visible", 70)
    if check_kilo_first and kilo_mode == "only":
        wait_for_reply("EARLY", early, "candidate's original startup reply")
        receipt = cli("daemon", "stop")["receipt"]
        assert [r["actor_id"] for r in receipt["confirmed_stopped"]] == ["m1"]
        assert daemon.wait(timeout=40) == 0
        u.wait_for(lambda: all(u.process_gone(pid[0]) for pid in before.values()), "candidate cleanup")
        normal_stop_done = True
        verify_kilo_turns([("EARLY", early)], submitted_ms, cold_screen_empty)
        (out / "result.json").write_text(json.dumps({
            "case": "candidate_cold", "runtime": runtime, "version": v, "build": initial["build"],
            "model": "loopback mock", "first_delivery_verified": True, "clean_stop": True,
        }, indent=2) + "\n")
        print("Kilo accepted the original candidate startup message and replied once.")
        sys.exit(0)
    e1 = start("ALPHA")
    wait_for_reply("ALPHA", e1, "reply before Keep")
    mark("first reply")
    if first_turn_roles:
        started = before
        try:
            before = u.wait_for(lambda: ready_set(expected_roles), "member MCP after the first turn", 30)
        except TimeoutError:
            write_process_dump("first-turn-timeout-processes.txt")
            raise
        # The processes that ran before the turn are still the same instances.
        before_turn_roles = expected_roles - first_turn_roles
        assert {role: before[role] for role in before_turn_roles} == {
            role: started[role] for role in before_turn_roles
        }, (started, before)
    if runtime == "hermes":
        gateway["after_first_turn"] = hermes_gateway_identities(before["provider"][0])
    stopped = cli("daemon", "stop", "--keep-agents")["receipt"]
    assert daemon.wait(timeout=40) == 0
    assert process_set() == before and not stopped["unconfirmed"]
    mark("keep stop, processes unchanged")
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
    mark("new daemon answered")
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
    wait_for_reply("BRAVO", e2, "reply after Keep")
    mark("second reply")
    assert process_set() == before
    if runtime == "hermes":
        gateway["after_second_turn"] = hermes_gateway_identities(before["provider"][0])
        (out / "hermes-gateway.json").write_text(json.dumps(gateway, indent=2) + "\n")
        check_one_gateway(gateway)

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
    if check_kilo_first:
        ev = json.loads((out / "early-diagnostic.json").read_text())
        ev["early_final_replies"] = len(replies("EARLY", ev["event"]))
        ev["decisions"] = [r.get("decision") for r in u.read_calls(model_log)]
        ev["delivery_evidence"] = kilo_delivery_evidence(ev["event"])
        print("Kilo first delivery evidence:", json.dumps(ev["delivery_evidence"]), flush=True)
        (out / "early-diagnostic.json").write_text(json.dumps(ev, indent=2))
    final = cli("daemon", "stop")["receipt"]
    assert [r["actor_id"] for r in final["confirmed_stopped"]] == ["m1"]
    u.wait_for(
        lambda: all(u.process_gone(identity[0]) for identity in before.values()),
        "normal stop of node and MCP",
    )
    normal_stop_done = True
    mark("normal stop, member processes gone")
    # Counted only now that the member has ended, so a late or repeated reply
    # to either message, or one with another text, fails the case.
    for word, event in (("ALPHA", e1), ("BRAVO", e2)):
        texts = reply_texts(event)
        assert texts == [word], (word, texts)
    # Evidence only: what still runs from this case's private root after the
    # member's processes ended, such as a runtime's own helpers.
    (out / "after-stop-processes.txt").write_text(
        "".join(
            f"{pid} {ppid} {command[:300]}\n"
            for pid, ppid, command in ps_rows()
            if str(private) in command and pid != os.getpid()
        )
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
    if runtime == "hermes":
        result["hermes_gateway"] = gateway
    if runtime == "kilo":
        inputs = [("ALPHA", e1), ("BRAVO", e2)]
        if check_kilo_first:
            inputs.insert(0, ("EARLY", early))
        verify_kilo_turns(inputs, submitted_ms if check_kilo_first else None,
                          cold_screen_empty if check_kilo_first else None)
        result["first_delivery_verified"] = check_kilo_first
    (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"real {runtime} {v} kept its process and MCP, both replies matched")
finally:
    if not normal_stop_done:
        # After a keep stop no daemon answers, but the session owner still holds
        # the member; `cccc daemon stop` then stops it directly. Its own failure
        # must not replace the error that brought the case here.
        try:
            subprocess.run(
                [program, "daemon", "stop"],
                env=env,
                stdout=hostlog,
                stderr=hostlog,
                timeout=120,
            )
        except Exception:
            pass
    if daemon.poll() is None:
        daemon.terminate()
        daemon.wait(timeout=20)
    if runtime == "hermes":
        # Hermes keeps its gateway for ten idle minutes after the last client, and
        # a runner without a PID namespace would keep it after run.sh deletes this
        # case. End the gateways this case saw or its installation started, and only
        # those; a run that failed before any snapshot relies on the installation.
        try:
            recorded = (
                [identity for snapshot in gateway.values() for identity in snapshot]
                if "gateway" in globals()
                else []
            )
            (out / "hermes-gateway-ended.json").write_text(
                json.dumps(end_hermes_gateways(recorded)) + "\n"
            )
        except Exception:
            pass
    model.terminate()
    model.wait(timeout=10)
    if "gid" in globals():
        shutil.copy2(home / "groups" / gid / "ledger.jsonl", out / "ledger.jsonl")
    hostlog.close()
    model_output.close()
