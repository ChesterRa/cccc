#!/usr/bin/env python3
"""Exercise both real Web Keep actions with a retained PTY and MCP client.

The suite runs this fixture in a private home. Both executables
must be distinct builds of the same version. A loopback server supplies the
release archive. The Custom terminal fixture uses CCCC tools without a model.
"""

import hashlib
import http.server
import json
import os
import platform
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path


def wait_for(check, label, seconds=55):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            if value := check():
                return value
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(0.15)
    raise TimeoutError(label)


def run_json(command, env=None):
    return json.loads(subprocess.check_output(command, env=env, timeout=30))


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def process_identity(pid):
    if sys.platform == "darwin":
        row = run_json([os.environ["CCCC_TEST_SDK_INSPECTOR"], "--inspect", str(pid)])
        assert row["ok"] and row["pid"] == pid and row["uid"] == os.getuid()
        return [
            pid,
            [row["boot_uuid"], row["birth_seconds"], row["birth_microseconds"]],
        ]
    proc = Path("/proc", str(pid))
    stat = (proc / "stat").read_text().rsplit(")", 1)[1].split()
    executable = (proc / "exe").stat()
    return [pid, int(stat[19]), executable.st_dev, executable.st_ino]


def process_gone(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return True
    return False


def daemon_request(home, op, **args):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(2)
        connection.connect(str(home / "daemon/ccccd.sock"))
        request = json.dumps({"v": 1, "op": op, "args": args}) + "\n"
        connection.sendall(request.encode())
        return json.loads(connection.makefile("rb").readline())["result"]


def daemon_ping(home):
    return daemon_request(home, "ping")


def post(port, path, body):
    base = f"http://127.0.0.1:{port}"
    request = urllib.request.Request(
        base + path,
        json.dumps(body).encode(),
        headers={
            "Content-Type": "application/json",
            "Origin": base,
            "Sec-Fetch-Site": "same-origin",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            result = json.load(response)
    except urllib.error.HTTPError as error:
        detail = json.load(error)["error"]
        raise RuntimeError(
            f"{path} returned {error.code}: {detail['code']} {detail['message']}"
        ) from error
    assert result["ok"], result
    return result["result"]


def read_calls(log):
    try:
        return [json.loads(line) for line in log.read_text().splitlines()]
    except (FileNotFoundError, json.JSONDecodeError):
        return []


def run_case(kind, root, out, old, new, before, after, archive_server):
    case = out / kind
    case.mkdir()
    home = root / f"{kind}-home"
    install = root / f"{kind}-install"
    project = root / f"{kind}-project"
    install.mkdir()
    project.mkdir()
    program = install / "cccc"
    shutil.copy2(old, program)
    if kind == "release":
        (install / ".cccc-standalone").write_text("standalone-v1\n")
    with socket.socket() as address:
        address.bind(("127.0.0.1", 0))
        port = address.getsockname()[1]
    env = dict(
        os.environ,
        CCCC_HOME=str(home),
        CCCC_RELEASE_BASE_URL=archive_server,
        CCCC_RELEASE_INDEX_URL="https://127.0.0.1:1/releases",
    )

    def cli(*args):
        return run_json([program, *args, "--port", str(port)], env)

    call_log = case / "calls.jsonl"
    stand_in = Path(__file__).with_name("maintenance_mcp_member.py")
    with (case / "host.log").open("wb") as host_log:
        host = subprocess.Popen(
            [program, "web", "--port", str(port)],
            env=env,
            stdout=host_log,
            stderr=subprocess.STDOUT,
        )
        try:
            old_daemon = wait_for(lambda: daemon_ping(home), "source daemon")
            assert old_daemon["build"]["source_id"] == before["source_id"]
            host_identity = process_identity(host.pid)
            old_install = program.stat()
            gid = cli("attach", str(project))["group_id"]
            cli(
                "actor",
                "add",
                "worker",
                "--runtime",
                "custom",
                "--command",
                shlex.join([sys.executable, str(stand_in)]),
                "--group",
                gid,
                "--env",
                f"STANDIN_CCCC={program}",
                "--env",
                f"STANDIN_LOG={call_log}",
            )
            cli("actor", "start", "worker", "--group", gid)

            def successful_calls():
                return [
                    c for c in read_calls(call_log) if c["ev"] == "call" and c["ok"]
                ]

            initial = wait_for(
                lambda: len(successful_calls()) >= 2 and successful_calls(),
                "old MCP calls",
            )
            member_pid, mcp_pid = initial[0]["pid"], initial[0]["mcp_pid"]
            processes = [process_identity(member_pid), process_identity(mcp_pid)]
            if kind == "local":
                candidate = install / "cccc.next"
                shutil.copy2(new, candidate)
                candidate.replace(program)
                checked = post(port, "/api/v1/update/check", {})["check"]["local"]
                assert checked["state"] == "rebuilt" and checked["keep"]["allowed"], (
                    checked
                )
                assert checked["source_id"] == after["source_id"], checked
                selection = {
                    "source": "local",
                    "token": checked["token"],
                    "source_id": checked["source_id"],
                }
            else:
                selection = {"source": "release", "version": after["version"]}
            response = post(port, "/api/v1/update/start", {**selection, "mode": "keep"})
            assert response["accepted"] and response["mode"] == "keep", response

            def completed():
                record = json.loads((home / "update/last.json").read_text())
                return record if record["agents"]["state"] != "observing" else None

            result = wait_for(completed, f"{kind} update result", seconds=100)
            assert result["transaction"] == response["transaction"], result
            assert result["result"] == (
                "installed" if kind == "release" else "restarted"
            ), result
            assert result["source"] == kind and result["mode"] == "keep", result
            assert result["agents"]["state"] == "observed", result
            assert [
                (item["group_id"], item["actor_id"])
                for item in result["agents"]["reattached"]
            ] == [(gid, "worker")], result
            new_daemon = wait_for(lambda: daemon_ping(home), "target daemon")
            assert new_daemon["build"]["source_id"] == after["source_id"]
            # This Web entry point embeds the daemon in the relaying host.
            assert old_daemon["pid"] == new_daemon["pid"] == host.pid
            current_identity = process_identity(host.pid)
            assert current_identity[:2] == host_identity[:2]
            if sys.platform == "linux":
                assert current_identity[2:] != host_identity[2:]
            current_install = program.stat()
            assert (current_install.st_dev, current_install.st_ino) != (
                old_install.st_dev,
                old_install.st_ino,
            )
            assert digest(program) == digest(new)
            count = len(successful_calls())
            subsequent = wait_for(
                lambda: (
                    len(successful_calls()) >= count + 2 and successful_calls()[count:]
                ),
                "MCP calls after target daemon startup",
            )
            assert all(
                (c["pid"], c["mcp_pid"]) == (member_pid, mcp_pid) for c in subsequent
            )
            assert [
                process_identity(member_pid),
                process_identity(mcp_pid),
            ] == processes
            assert sum(c["ev"] == "initialize" for c in read_calls(call_log)) == 1
            assert not (home / "update/active.json").exists()
            observations = {
                "source": kind,
                "platform": sys.platform,
                "same_host_birth": current_identity[:2] == host_identity[:2],
                "installed_file_changed": True,
                "daemon_before": old_daemon["build"],
                "daemon_after": new_daemon["build"],
                "retained_processes": processes,
                "post_update_calls": len(subsequent),
                "result": {
                    name: value for name, value in result.items() if name != "token"
                },
            }
        finally:
            if host.poll() is None:
                host.send_signal(signal.SIGINT)
                host.wait(timeout=60)
        assert host.returncode == 0, host.returncode
        wait_for(
            lambda: all(process_gone(pid) for pid in (member_pid, mcp_pid)),
            "normal Stop retires both retained processes",
        )
        observations["stopped_after_validation"] = True
        (case / "result.json").write_text(json.dumps(observations, indent=2) + "\n")
        print(
            f"{kind}: Keep relayed to a distinct build, old MCP answered, clean Stop",
            flush=True,
        )


def main():
    old, new, output = (Path(arg).resolve() for arg in sys.argv[1:4])
    kinds = sys.argv[4:] or ["local", "release"]
    assert set(kinds) <= {"local", "release"}
    targets = {
        ("Linux", "x86_64"): "x86_64-unknown-linux-gnu",
        ("Darwin", "arm64"): "aarch64-apple-darwin",
    }
    target = targets[(platform.system(), platform.machine())]
    if sys.platform == "darwin":
        inspector = Path(os.environ["CCCC_TEST_SDK_INSPECTOR"])
        assert (
            inspector.is_absolute()
            and inspector.is_file()
            and os.access(inspector, os.X_OK)
        )
    private = Path(os.environ["TMPDIR"]).parent
    assert private.name.startswith("cccc-isolated.")
    assert Path(os.environ["HOME"]) == private / "home"
    assert Path(os.environ["CCCC_HOME"]) == private / "cccc"
    output.mkdir(parents=True, exist_ok=True)

    def build(binary):
        checked = run_json([binary, "update", "--self-check"])
        return {"version": checked["version"], **checked["build"]}

    before, after = build(old), build(new)
    assert (
        before["version"] == after["version"]
        and before["source_id"] != after["source_id"]
    )
    with tempfile.TemporaryDirectory(
        prefix="hold", dir=os.environ["TMPDIR"]
    ) as directory:
        root = Path(directory)
        release_root = root / "releases"
        release = release_root / "download" / ("v" + after["version"])
        release.mkdir(parents=True)
        version = after["version"]
        package = f"cccc-v{version}-{target}"
        archive = release / (package + ".tar.gz")
        installer = Path.cwd() / "scripts/install.sh"
        (release / "install.sh").write_text(
            installer.read_text().replace("@CCCC_VERSION@", version)
        )
        with tarfile.open(archive, "w:gz", compresslevel=1) as tar:
            tar.add(new, arcname=package + "/cccc")
        archive_names = [f"cccc-v{version}-{name}.tar.gz" for name in targets.values()]
        archive_names.append(f"cccc-v{version}-x86_64-pc-windows-msvc.zip")
        checksum = digest(archive)
        (release / "SHA256SUMS").write_text(
            "".join(
                f"{checksum if name == archive.name else '0' * 64}  {name}\n"
                for name in archive_names
            )
        )

        class ReleaseFiles(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=release_root, **kwargs)

        with http.server.ThreadingHTTPServer(("127.0.0.1", 0), ReleaseFiles) as server:
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                for kind in kinds:
                    run_case(
                        kind,
                        root,
                        output,
                        old,
                        new,
                        before,
                        after,
                        f"http://127.0.0.1:{server.server_port}",
                    )
            finally:
                server.shutdown()


if __name__ == "__main__":
    main()
