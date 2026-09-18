"""Real Web/HTTP/daemon regression with an isolated home and no Actor startup.

Build web/dist and cccc first; run with --binary target/debug/cccc or a package.
The Unix IPC fixture is also run by Linux CI. No installed instance is touched.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/cccc")
    args = parser.parse_args()
    binary = args.binary.resolve()
    with tempfile.TemporaryDirectory(prefix="cc-actor-") as directory:
        temp = Path(directory)
        home = temp / "home"
        env = {**os.environ, "CCCC_HOME": str(home)}
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        processes = []
        logs = []

        def launch(*argv):
            log = (temp / f"process-{len(processes)}.log").open("w+")
            logs.append(log)
            process = subprocess.Popen([str(binary), *argv], env=env, stdout=log, stderr=subprocess.STDOUT)
            processes.append(process)
            return process

        def call(op, **kwargs):
            with socket.socket(socket.AF_UNIX) as sock:
                sock.settimeout(10)
                sock.connect(str(home / "daemon/ccccd.sock"))
                sock.sendall((json.dumps({"op": op, "args": kwargs}) + "\n").encode())
                result = json.loads(sock.makefile("rb").readline())
            assert result["ok"], (op, result.get("error"))
            return result["result"]

        try:
            daemon = launch("daemon", "run")
            for _ in range(200):
                try:
                    call("ping")
                    break
                except OSError:
                    assert daemon.poll() is None, "isolated daemon exited"
                    time.sleep(0.05)
            else:
                raise AssertionError("isolated daemon did not start")
            gid = call("group_create", title="Actor configuration fixture")["group"]["group_id"]
            for key, runtime in [("source", "claude"), ("target", "codex")]:
                call("actor_profile_upsert", profile_id=key, name=f"{key.title()} profile", runtime=runtime)
            call("actor_profile_secret_update", profile_id="target", set={"PROFILE_KEY": "synthetic"})
            call("actor_add", group_id=gid, actor_id="linked", profile_id="source", enabled=False, title="Linked actor", by="user")
            command = ["codex", "-c", 'model_provider="ZAI"', "--profile", "Team One", "", "O'Reilly", r"C:\Some Folder\config"]
            call("actor_add", group_id=gid, actor_id="custom-one", runtime="codex", command=command, enabled=False, title="Custom actor", by="user", env_private={"AUDIT_OLD": "synthetic"})
            web = launch("web", "--host", "127.0.0.1", "--port", str(port))
            base = f"http://127.0.0.1:{port}"
            for _ in range(200):
                try:
                    urllib.request.urlopen(f"{base}/ui/", timeout=0.5).close()
                    break
                except OSError:
                    assert web.poll() is None, "isolated Web exited"
                    time.sleep(0.05)
            else:
                raise AssertionError("isolated Web did not start")
            metadata = temp / "fixture.json"
            metadata.write_text(json.dumps({"base": base, "gid": gid, "originalCommand": command}))
            subprocess.run(["node", str(Path(__file__).with_suffix(".mjs")), str(metadata)], cwd=ROOT / "web", env=env, timeout=180, check=True)
            assert all(not actor["enabled"] for actor in call("actor_list", group_id=gid)["actors"]), "test must never start Actors"
        except BaseException:
            for log in logs:
                log.flush()
                log.seek(0)
                print(log.read())
            raise
        finally:
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
            for process in reversed(processes):
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            for log in logs:
                log.close()


if __name__ == "__main__":
    main()
