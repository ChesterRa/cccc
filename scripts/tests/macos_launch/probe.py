"""Compare three private, headless Chrome launch methods on macOS. Prints JSON."""

import json
import os
from pathlib import Path
import platform
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
APP = Path("/Applications/Google Chrome.app")
CHROME = APP / "Contents/MacOS/Google Chrome"
LSOF = "/usr/sbin/lsof"
OBSERVE = 25
LATE_GRACE = 4


def row(pid):
    p = subprocess.run(
        ["/bin/ps", "-ww", "-p", str(pid), "-o", "lstart=,command="],
        capture_output=True,
        text=True,
        timeout=3,
    )
    if p.returncode not in (0, 1):
        raise RuntimeError("could not inspect profile process")
    return p.stdout.strip()


def owned(profile):
    try:
        pid = int(os.readlink(profile / "SingletonLock").rsplit("-", 1)[-1])
    except (FileNotFoundError, ValueError):
        return None
    if pid <= 1:
        return None
    identity = row(pid)
    if "--user-data-dir=" + str(profile) in identity.split():
        return pid, identity
    return None


def is_listener(pid, port):
    check = subprocess.run(
        [
            LSOF,
            "-nP",
            "-a",
            "-p",
            str(pid),
            "-iTCP@127.0.0.1:" + str(port),
            "-sTCP:LISTEN",
            "-t",
        ],
        capture_output=True,
        text=True,
        timeout=2,
    )
    return check.returncode == 0 and str(pid) in check.stdout.split()


def cleanup_profile(owner):
    if not owner:
        return
    pid, identity = owner
    for sig in (signal.SIGTERM, signal.SIGKILL):
        if row(pid) != identity:
            return
        try:
            os.kill(pid, sig)
        except ProcessLookupError:
            return
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            if row(pid) != identity:
                return
            time.sleep(0.1)
    raise RuntimeError("verified private browser still running")


def end_child(child):
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=1)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=3)


def result_for(mode, result):
    owner = result.get("owner_pid")
    callback = result.get("callback", {})
    if "error_domain" in callback:
        return "appkit_error"
    if owner and mode == "appkit" and callback.get("pid") not in (None, owner):
        return "callback_pid_mismatch"
    if owner and result.get("cdp", {}).get("pid") not in (None, owner):
        return "cdp_pid_mismatch"
    if mode == "direct" and owner and owner != result["child_pid"]:
        return "child_pid_mismatch"
    if mode != "direct" and result.get("launcher_exit") not in (None, 0):
        return "launcher_failed"
    if mode == "direct" and "launcher_exit" in result:
        return "child_exited"
    if mode == "appkit" and callback.get("pid") != owner:
        return "inconclusive"
    if (
        owner
        and result.get("listener_pid") == owner
        and result.get("cdp", {}).get("pid") == owner
    ):
        if mode == "direct" or result.get("launcher_exit") == 0:
            return "ready"
    return "inconclusive"


def one(mode, root, helper, node, result):
    case = root / mode
    case.mkdir()
    profile = case / "profile"
    for path in (profile, case / "home", case / "tmp"):
        path.mkdir()
    env = {
        "HOME": str(case / "home"),
        "TMPDIR": str(case / "tmp") + "/",
        "PATH": os.defpath,
        "LANG": "en_US.UTF-8",
    }
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    args = [
        f"--remote-debugging-port={port}",
        f"--user-data-dir={profile}",
        "--no-first-run",
        "--no-default-browser-check",
        "--headless=new",
        "--disable-background-networking",
        "about:blank",
    ]
    cmds = {
        "open": ["/usr/bin/open", "-g", "-n", "-a", str(APP), "--args"],
        "appkit": [str(helper), str(APP)],
        "direct": [str(CHROME)],
    }
    result["port"] = port
    started = time.monotonic()
    callback_path = case / "callback.json"
    output_path = callback_path if mode == "appkit" else Path(os.devnull)
    with output_path.open("wb") as output:
        child = subprocess.Popen(
            cmds[mode] + args,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=output,
            stderr=subprocess.DEVNULL,
        )
        result["child_pid"] = child.pid
        owner = None
        try:
            while time.monotonic() - started < OBSERVE:
                elapsed = round(time.monotonic() - started, 2)
                if child.poll() is not None and "launcher_exit" not in result:
                    result.update(
                        launcher_exit=child.returncode, launcher_exit_s=elapsed
                    )
                    if mode == "appkit" and callback_path.stat().st_size:
                        result["callback"] = json.loads(callback_path.read_text())
                if not owner:
                    owner = owned(profile)
                    if owner:
                        result.update(owner_pid=owner[0], lock_s=elapsed)
                if owner and "cdp" not in result and row(owner[0]) == owner[1]:
                    if is_listener(owner[0], port):
                        result["listener_pid"] = owner[0]
                        check = subprocess.run(
                            [node, str(HERE / "cdp.mjs"), str(port)],
                            env=env,
                            capture_output=True,
                            text=True,
                            timeout=3,
                        )
                        if check.returncode == 0:
                            result.update(
                                cdp=json.loads(check.stdout),
                                cdp_s=round(time.monotonic() - started, 2),
                            )
                result["status"] = result_for(mode, result)
                if result["status"] != "inconclusive":
                    break
                time.sleep(0.2)
            result["elapsed_s"] = round(time.monotonic() - started, 2)
            if result.get("status") == "inconclusive":
                result["deadline_reached"] = True
        finally:
            # The helper and direct Chrome are our own children. An AppKit
            # callback alone never authorizes a signal to its reported PID.
            try:
                end_child(child)
            finally:
                owner = owner or owned(profile)
                cleanup_profile(owner)
                result["cleanup"] = (
                    "observed_owner_ended" if owner else "no_owner_observed"
                )


def main():
    if sys.platform != "darwin":
        raise SystemExit("The three-way probe requires macOS.")
    node = shutil.which("node")
    if not node or not CHROME.is_file():
        raise SystemExit(
            "This probe requires Node 22+ and /Applications/Google Chrome.app."
        )
    major = subprocess.check_output(
        [node, "-p", "process.versions.node.split('.')[0]"], text=True, timeout=5
    )
    if int(major) < 22:
        raise SystemExit("This probe requires Node 22+.")
    root = Path(tempfile.mkdtemp(prefix="ca-", dir="/tmp")).resolve()
    results = {}
    cleanup_ok = False
    try:
        helper = root / "launch"
        subprocess.run(
            ["/usr/bin/xcrun", "swiftc", str(HERE / "launch.swift"), "-o", str(helper)],
            check=True,
            timeout=120,
        )
        for mode in ("open", "appkit", "direct"):
            result = results[mode] = {}
            try:
                one(mode, root, helper, node, result)
            except (
                OSError,
                ValueError,
                RuntimeError,
                subprocess.SubprocessError,
            ) as error:
                result.update(status="probe_error", error=type(error).__name__)
                if isinstance(error, subprocess.TimeoutExpired):
                    result["timed_out_command"] = Path(error.cmd[0]).name
        # LaunchServices can complete after its client goes away. Recheck every
        # private profile, including cases where the launcher returned an error.
        deadline = time.monotonic() + LATE_GRACE
        while True:
            for mode, result in results.items():
                owner = owned(root / mode / "profile")
                if owner:
                    cleanup_profile(owner)
                    result["late_owner_stopped"] = owner[0]
            if time.monotonic() >= deadline:
                break
            time.sleep(0.25)
        cleanup_ok = True
    finally:
        if cleanup_ok or not results:
            shutil.rmtree(root)
        else:
            print(f"Private diagnostic directory retained: {root}", file=sys.stderr)
        report = {
            "os": platform.mac_ver()[0],
            "arch": platform.machine(),
            "observation_limit_s": OBSERVE,
            "late_grace_s": LATE_GRACE,
            "cases": results,
        }
        print(json.dumps(report, indent=2), flush=True)
    return int(any(r.get("status") == "probe_error" for r in results.values()))


if __name__ == "__main__":
    raise SystemExit(main())
