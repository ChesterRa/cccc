"""Sample only the verified fixture Chrome and its NetworkService on a disposable macOS runner."""

from pathlib import Path
import os
import subprocess
import sys


def owns_listener(pid, port):
    executable = "/usr/sbin/lsof" if sys.platform == "darwin" else "/usr/bin/lsof"
    check = subprocess.run(
        [
            executable,
            "-nP",
            "-a",
            "-p",
            str(pid),
            f"-iTCP@127.0.0.1:{port}",
            "-sTCP:LISTEN",
            "-t",
        ],
        text=True,
        capture_output=True,
        timeout=2,
    )
    return check.returncode == 0 and str(pid) in check.stdout.split()


def sample_processes(pid, profile, directory):
    if sys.platform != "darwin":
        return {}

    def row(process):
        return subprocess.run(
            ["/bin/ps", "-ww", "-p", str(process), "-o", "ppid=,command="],
            text=True,
            capture_output=True,
            timeout=2,
        ).stdout.strip()

    result = {}
    try:
        lock = int(os.readlink(Path(profile) / "SingletonLock").rsplit("-", 1)[-1])
        browser = row(pid)
        if lock != pid or "--user-data-dir=" + str(profile) not in browser.split():
            return {"error": "private Chrome identity changed"}
        children = subprocess.run(
            ["/usr/bin/pgrep", "-P", str(pid)],
            text=True,
            capture_output=True,
            timeout=2,
        )
        targets = [("browser", pid, browser)]
        for child in children.stdout.split():
            command = row(int(child))
            if (
                command.split()[:1] == [str(pid)]
                and "--utility-sub-type=network.mojom.NetworkService" in command.split()
            ):
                targets.append(("network-service", int(child), command))
                break
        for role, process, command in targets:
            if row(pid) != browser or row(process) != command:
                result[role] = "process ended or changed"
                continue
            try:
                sample = directory / (role + ".sample.txt")
                p = subprocess.run(
                    ["/usr/bin/sample", str(process), "1", "-file", str(sample)],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    timeout=9,
                )
                result[role] = {
                    "exit": p.returncode,
                    "bytes": sample.stat().st_size if sample.exists() else 0,
                }
            except subprocess.TimeoutExpired:
                result[role] = "sample timed out"
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        result["error"] = str(error)
    return result
