#!/usr/bin/env python3
"""Recover an interrupted Local Keep from each ordinary Unix startup entry point."""

import fcntl
import json
import os
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from update_keep import (  # noqa: E402
    daemon_ping,
    daemon_request,
    digest,
    post,
    process_identity,
    process_gone,
    read_calls,
    run_json,
    wait_for,
)


def unused_port():
    with socket.socket() as address:
        address.bind(("127.0.0.1", 0))
        return str(address.getsockname()[1])


def exercise(root, out, entry, old, new, stand_in):
    work = root / entry
    work.mkdir()
    case = out / entry
    case.mkdir()
    home = work / "cccc"
    project = work / "project"
    project.mkdir()
    program = work / "install" / "cccc"
    program.parent.mkdir()
    shutil.copy2(old, program)
    port = unused_port()
    environment = dict(
        os.environ,
        CCCC_HOME=str(home),
        CCCC_RELEASE_INDEX_URL="https://127.0.0.1:1/releases",
    )
    for build in (old, new):
        assert build.is_file() and os.access(build, os.X_OK), build
    expected = {}
    for name, binary in (("old", old), ("new", new)):
        expected[name] = run_json([binary, "update", "--self-check"], environment)[
            "build"
        ]
    assert expected["old"]["source_id"] != expected["new"]["source_id"]

    def cli(*args):
        return run_json([program, *args, "--port", port], environment)

    call_log = case / "calls.jsonl"

    def successful_calls():
        return [
            entry
            for entry in read_calls(call_log)
            if entry["ev"] == "call" and entry["ok"]
        ]

    restarted = None
    with (
        (case / "old.log").open("wb") as old_log,
        (case / "restart.log").open("wb") as new_log,
    ):
        original = subprocess.Popen(
            [program, "web", "--port", port],
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=old_log,
            stderr=subprocess.STDOUT,
        )
        try:
            old_daemon = wait_for(lambda: daemon_ping(home), "old daemon")
            assert old_daemon["build"] == expected["old"]
            group = cli("attach", str(project))["group_id"]
            cli(
                "actor",
                "add",
                "worker",
                "--runtime",
                "custom",
                "--command",
                shlex.join([sys.executable, str(stand_in)]),
                "--group",
                group,
                "--env",
                f"STANDIN_CCCC={program}",
                "--env",
                f"STANDIN_LOG={call_log}",
            )
            cli("actor", "start", "worker", "--group", group)

            def session():
                return daemon_request(
                    home,
                    "terminal_status",
                    group_id=group,
                    actor_id="worker",
                    by="user",
                )["session"]

            first = wait_for(
                lambda: len(successful_calls()) >= 2 and successful_calls()[0],
                "old MCP",
            )
            pids = [first["pid"], first["mcp_pid"]]
            retained = [process_identity(pid) for pid in pids]
            before = session()
            assert before["running"] and before["started_at"]

            shim = program.with_name("next")
            shim.write_text(
                "#!/bin/sh\n"
                'if [ "$1" = update ] && [ "$2" = --resume ]; then exit 73; fi\n'
                f'exec {shlex.quote(str(new))} "$@"\n'
            )
            shim.chmod(0o755)
            shim.replace(program)
            shim_hash = digest(program)
            checked = post(port, "/api/v1/update/check", {})["check"]["local"]
            assert checked["state"] == "rebuilt" and checked["keep"]["allowed"]
            assert checked["source_id"] == expected["new"]["source_id"]
            update = post(
                port,
                "/api/v1/update/start",
                {
                    "source": "local",
                    "token": checked["token"],
                    "source_id": checked["source_id"],
                    "mode": "keep",
                },
            )
            assert update["accepted"] and update["mode"] == "keep"
            assert original.wait(timeout=60) == 73
            active = home / "update/active.json"
            transaction = update["transaction"]
            assert json.loads(active.read_text())["transaction_id"] == transaction
            assert [process_identity(pid) for pid in pids] == retained
            command = ["web", "--port", port] if entry == "web" else ["daemon", entry]
            with (home / "daemon/ccccd.lock").open("rb") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                blocked = subprocess.run(
                    [new, *command],
                    env=environment,
                    capture_output=True,
                    timeout=15,
                    check=False,
                )
                assert (
                    blocked.returncode != 0 and b"cccc daemon status" in blocked.stderr
                )
                assert json.loads(active.read_text())["transaction_id"] == transaction

            restarted = subprocess.Popen(
                [new, *command],
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=new_log,
                stderr=subprocess.STDOUT,
            )
            if entry == "start":
                assert restarted.wait(timeout=30) == 0
            daemon = wait_for(lambda: daemon_ping(home), f"{entry}: ordinary recovery")
            assert (
                daemon["build"] == expected["new"]
                and daemon["pid"] != old_daemon["pid"]
            )
            reattached = daemon["retained_agents"]
            assert [
                (item["group_id"], item["actor_id"], item["generation"])
                for item in reattached
            ] == [(group, "worker", before["started_at"])]
            resumed = session()
            assert resumed["running"] and resumed["started_at"] == before["started_at"]
            offset = len(successful_calls())
            subsequent = wait_for(
                lambda: (
                    len(successful_calls()) >= offset + 2
                    and successful_calls()[offset:]
                ),
                "original MCP on the recovered home",
            )
            assert all([call["pid"], call["mcp_pid"]] == pids for call in subsequent)
            assert [process_identity(pid) for pid in pids] == retained
            last = json.loads((home / "update/last.json").read_text())
            assert (last["source"], last["result"], last["transaction"]) == (
                "local",
                "abandoned",
                transaction,
            )
            assert not active.exists() and not (home / "update" / transaction).exists()
            assert digest(program) == shim_hash
            assert (
                "did not complete and was abandoned"
                in (case / "restart.log").read_text()
            )
            if entry == "web":

                def web_status():
                    with urllib.request.urlopen(
                        f"http://127.0.0.1:{port}/api/v1/update/status", timeout=2
                    ) as response:
                        return json.load(response)["result"]["status"]

                status = wait_for(web_status, "recovered Web update status")
                assert status["last"]["result"] == "abandoned"
                assert status["last"]["transaction"] == transaction
        finally:
            if original.poll() is None:
                original.send_signal(signal.SIGINT)
                original.wait(timeout=45)
            with (case / "stop.log").open("wb") as log:
                stop = subprocess.run(
                    [new, "daemon", "stop"],
                    env=environment,
                    stdout=log,
                    stderr=subprocess.STDOUT,
                    timeout=45,
                    check=False,
                )
            if restarted is not None and restarted.poll() is None:
                restarted.wait(timeout=45)
    assert stop.returncode == 0
    wait_for(lambda: all(process_gone(pid) for pid in pids), "Clean Stop")
    (case / "result.json").write_text(
        json.dumps(
            {
                "entry": entry,
                "platform": sys.platform,
                "refused_to_clear_while_daemon_lock_was_held": True,
                "interrupted_candidate_exit": original.returncode,
                "builds": expected,
                "daemon_pids": [old_daemon["pid"], daemon["pid"]],
                "session_generation": resumed["started_at"],
                "daemon_reported_reattachment": True,
                "retained_process_identities": retained,
                "post_recovery_tool_calls": len(subsequent),
                "update_result": {"result": last["result"], "source": last["source"]},
                "web_status_verified": entry == "web",
                "clean_stop": True,
            },
            indent=2,
        )
        + "\n"
    )
    print(
        f"{entry}: lock respected, ordinary startup recovered the retained session and MCP",
        flush=True,
    )


def main():
    old, new, stand_in, out = (Path(arg).resolve() for arg in sys.argv[1:5])
    assert stand_in.is_file()
    if sys.platform == "darwin":
        assert os.access(os.environ["CCCC_TEST_SDK_INSPECTOR"], os.X_OK)
    isolated_dir = Path(os.environ["TMPDIR"]).parent
    assert isolated_dir.name.startswith("cccc-isolated.")
    assert Path(os.environ["HOME"]) == isolated_dir / "home"
    assert Path(os.environ["CCCC_HOME"]) == isolated_dir / "cccc"
    out.mkdir()
    entries = sys.argv[5:] or ["web", "start", "run"]
    assert set(entries) <= {"web", "start", "run"}
    with tempfile.TemporaryDirectory(
        prefix="recovery-", dir=os.environ["TMPDIR"]
    ) as directory:
        for entry in entries:
            exercise(Path(directory), out, entry, old, new, stand_in)


if __name__ == "__main__":
    main()
