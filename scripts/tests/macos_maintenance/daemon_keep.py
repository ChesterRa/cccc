#!/usr/bin/env python3
"""Exercise the public daemon Keep/start/stop path with one retained MCP client."""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile


def load_shared(path):
    spec = importlib.util.spec_from_file_location("keep_fixture", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def exercise(program, helper, standin, sdk_identity, root):
    home = root / "cccc"
    project = root / "project"
    project.mkdir()
    (root / "home").mkdir()
    (root / "tmp").mkdir()
    env = {
        "PATH": os.defpath,
        "HOME": str(root / "home"),
        "TMPDIR": str(root / "tmp"),
        "CCCC_HOME": str(home),
        "CODEX_HOME": str(root / "codex"),
        "CLAUDE_CONFIG_DIR": str(root / "claude"),
        "LANG": "C",
    }

    def cli(*args):
        return helper.run_json([program, *args], env)

    def birth(pid):
        if sys.platform == "darwin":
            row = helper.run_json([sdk_identity, "--inspect", str(pid)], env)
            assert row["ok"] and row["pid"] == pid and row["uid"] == os.getuid()
            return [
                pid,
                row["boot_uuid"],
                row["birth_seconds"],
                row["birth_microseconds"],
            ]
        return helper.process_identity(pid)

    def gone(pid):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return True
        return False

    calls_file = root / "calls.jsonl"

    def calls():
        return [
            r for r in helper.read_calls(calls_file) if r["ev"] == "call" and r["ok"]
        ]

    def start():
        proc = subprocess.Popen(
            [program, "daemon", "run"], env=env, stdin=subprocess.DEVNULL
        )
        hosts.append(proc)
        ping = helper.wait_for(lambda: helper.daemon_ping(home), "live fixture daemon")
        assert ping["pid"] == proc.pid
        return proc, ping

    hosts = []
    try:
        old, first_ping = start()
        group = cli("attach", str(project))["group_id"]
        cli(
            "actor",
            "add",
            "worker",
            "--group",
            group,
            "--runtime",
            "custom",
            "--command",
            shlex.join([sys.executable, str(standin)]),
            "--env",
            f"STANDIN_CCCC={program}",
            "--env",
            f"STANDIN_LOG={calls_file}",
        )
        cli("actor", "start", "worker", "--group", group)

        def session():
            return helper.daemon_request(
                home, "terminal_status", group_id=group, actor_id="worker", by="user"
            )["session"]

        initial = helper.wait_for(
            lambda: len(calls()) >= 2 and calls()[0], "first MCP calls"
        )
        pids = [initial["pid"], initial["mcp_pid"]]
        identities = [birth(pid) for pid in pids]
        before = session()
        assert before["running"] and before["started_at"]
        subprocess.run(
            [program, "daemon", "stop", "--keep-agents"],
            env=env,
            check=True,
            timeout=60,
        )
        assert old.wait(timeout=30) == 0
        assert [birth(pid) for pid in pids] == identities
        new, second_ping = start()
        assert first_ping["build"] == second_ping["build"]
        assert [
            (a["group_id"], a["actor_id"], a["generation"])
            for a in second_ping["retained_agents"]
        ] == [(group, "worker", before["started_at"])]
        resumed = session()
        assert resumed["running"] and resumed["started_at"] == before["started_at"]
        count = len(calls())
        helper.wait_for(lambda: len(calls()) >= count + 2, "post-restart MCP calls")
        assert all([r["pid"], r["mcp_pid"]] == pids for r in calls())
        assert (
            len([r for r in helper.read_calls(calls_file) if r["ev"] == "initialize"])
            == 1
        )
        assert [birth(pid) for pid in pids] == identities
        subprocess.run([program, "daemon", "stop"], env=env, check=True, timeout=60)
        assert new.wait(timeout=30) == 0
        helper.wait_for(
            lambda: all(gone(pid) for pid in pids), "member and MCP stopped"
        )
        return {
            "platform": sys.platform,
            "daemon_pids": [old.pid, new.pid],
            "build": first_ping["build"],
            "generation": before["started_at"],
            "process_identities": identities,
            "calls_after_restart": len(calls()) - count,
            "normal_stop_clean": True,
        }
    finally:
        if hosts:
            subprocess.run(
                [program, "daemon", "stop"],
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=60,
            )
        for host in hosts:
            if host.poll() is None:
                host.kill()
            host.wait(timeout=15)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ["program", "shared-fixture", "standin", "result"]:
        parser.add_argument("--" + arg, type=Path, required=True)
    parser.add_argument("--sdk-identity", type=Path)
    args = parser.parse_args()
    if sys.platform == "darwin" and not args.sdk_identity:
        parser.error("Darwin requires the SDK process inspector")
    for path in (args.program, args.shared_fixture, args.standin):
        if not path.is_file():
            parser.error(f"missing fixture input: {path}")
    if sys.platform not in ("darwin", "linux"):
        parser.error("requires a Darwin or Linux fixture")
    with tempfile.TemporaryDirectory(prefix="ck-", dir="/tmp") as short:
        result = exercise(
            args.program.resolve(),
            load_shared(args.shared_fixture),
            args.standin.resolve(),
            args.sdk_identity,
            Path(short),
        )
    args.result.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
