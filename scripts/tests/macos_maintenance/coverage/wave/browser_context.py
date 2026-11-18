"""Check LaunchServices and a private headless Chrome on the same macOS runner."""

import ctypes
import json
import os
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path
from urllib.parse import urlsplit

APP = Path("/Applications/Google Chrome.app")
CHROME = APP / "Contents/MacOS/Google Chrome"
TIMEOUT = 25
HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def private_gui_context():
    """Inspect only public session-presence flags, without collecting the session dictionary."""
    cg = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
    cf = ctypes.CDLL(
        "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation"
    )
    cg.CGSessionCopyCurrentDictionary.restype = ctypes.c_void_p
    cf.CFDictionaryGetValue.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    cf.CFDictionaryGetValue.restype = ctypes.c_void_p
    cf.CFStringCreateWithCString.argtypes = [
        ctypes.c_void_p,
        ctypes.c_char_p,
        ctypes.c_uint32,
    ]
    cf.CFStringCreateWithCString.restype = ctypes.c_void_p
    cf.CFRelease.argtypes = [ctypes.c_void_p]
    cf.CFBooleanGetValue.argtypes = [ctypes.c_void_p]
    cf.CFBooleanGetValue.restype = ctypes.c_bool
    session = cg.CGSessionCopyCurrentDictionary()
    result = {"quartz_session": bool(session)}
    if session:
        try:
            for name in ("kCGSSessionOnConsoleKey", "kCGSessionLoginDoneKey"):
                key = cf.CFStringCreateWithCString(None, name.encode(), 0x08000100)
                try:
                    value = cf.CFDictionaryGetValue(session, key)
                    result[name] = bool(cf.CFBooleanGetValue(value)) if value else None
                finally:
                    cf.CFRelease(key)
        finally:
            cf.CFRelease(session)
    return result


def process_row(pid):
    result = subprocess.run(
        ["/bin/ps", "-ww", "-p", str(pid), "-o", "lstart=,command="],
        capture_output=True,
        text=True,
        timeout=3,
    )
    return result.stdout.strip() if result.returncode == 0 else ""


def profile_owner(profile):
    try:
        pid = int(os.readlink(profile / "SingletonLock").rsplit("-", 1)[-1])
    except (OSError, ValueError):
        return None
    if pid <= 1:
        return None
    row = process_row(pid)
    if "--user-data-dir=" + str(profile) not in row.split():
        return None
    return pid, row


def browser_version_at(port):
    try:
        with HTTP.open(
            f"http://127.0.0.1:{port}/json/version", timeout=0.5
        ) as response:
            version = json.load(response)
        ws = urlsplit(version["webSocketDebuggerUrl"])
        if ws.hostname in {"localhost", "127.0.0.1"} and ws.port == port:
            return version.get("Browser")
    except (OSError, ValueError, KeyError):
        pass
    return None


def stop_profile(owner):
    if not owner:
        return
    pid, original = owner
    for sig in (signal.SIGTERM, signal.SIGKILL):
        if process_row(pid) != original:
            return
        try:
            os.kill(pid, sig)
        except ProcessLookupError:
            return
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            if process_row(pid) != original:
                return
            time.sleep(0.1)
    raise RuntimeError("Private browser is still running after cleanup")


def os_environment(environment):
    overrides = {}
    if environment in ("system", "home"):
        overrides["HOME"] = os.environ["HOME"]
    if environment in ("system", "temp"):
        overrides["TMPDIR"] = os.environ["TMPDIR"]
    if environment == "system":
        for key in (
            "XPC_FLAGS",
            "XPC_SERVICE_NAME",
            "__CF_USER_TEXT_ENCODING",
            "COMMAND_MODE",
        ):
            if key in os.environ:
                overrides[key] = os.environ[key]
    return overrides


def one(root, environment, launch):
    case = root / (environment + "-" + launch)
    case.mkdir()
    for name in ("home", "tmp", "profile"):
        (case / name).mkdir()
    profile = (case / "profile").resolve()
    env = {
        "PATH": os.environ["PATH"],
        "LANG": "en_US.UTF-8",
        "HOME": str(case / "home"),
        "TMPDIR": str(case / "tmp") + "/",
    }
    # Let shell scripts pick the OS paths without inheriting CI credentials.
    env.update(os_environment(environment))
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
    cmd = (
        [str(CHROME)]
        if launch == "direct"
        else ["/usr/bin/open", "-g", "-n", "-a", str(APP), "--args"]
    )
    ctx = subprocess.run(
        [sys.executable, "-I", __file__, "--session"],
        env=env,
        capture_output=True,
        text=True,
        timeout=8,
    )
    result = {
        "mode": launch,
        "environment": environment,
        "session": json.loads(ctx.stdout)
        if ctx.returncode == 0
        else {"error": ctx.returncode},
    }
    started = time.monotonic()
    owner = None
    with (case / "launcher.log").open("wb") as log:
        proc = subprocess.Popen(
            cmd + args,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            while time.monotonic() - started < TIMEOUT:
                elapsed = round(time.monotonic() - started, 2)
                if proc.poll() is not None and "exit_rc" not in result:
                    result.update(exit_rc=proc.returncode, exit_s=elapsed)
                if not owner:
                    owner = profile_owner(profile)
                    if owner:
                        result["lock_s"] = elapsed
                confirmed_listener = owner is not None
                if (
                    not confirmed_listener
                    and launch == "direct"
                    and proc.poll() is None
                ):
                    listener = subprocess.run(
                        [
                            "/usr/sbin/lsof",
                            "-nP",
                            "-a",
                            "-p",
                            str(proc.pid),
                            "-iTCP:" + str(port),
                            "-sTCP:LISTEN",
                            "-t",
                        ],
                        stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL,
                        text=True,
                        timeout=2,
                    )
                    confirmed_listener = str(proc.pid) in listener.stdout.split()
                if confirmed_listener and "browser" not in result:
                    browser = browser_version_at(port)
                    if browser:
                        result.update(browser=browser, cdp_s=elapsed)
                if "browser" in result and (launch == "direct" or "exit_rc" in result):
                    break
                time.sleep(0.25)
        finally:
            if launch == "open" and proc.poll() is None:
                proc.kill()
                proc.wait(timeout=3)
            owner = owner or profile_owner(profile)
            if owner:
                stop_profile(owner)
            if launch == "direct" and proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=3)
    result["ready"] = (
        "browser" in result
        and "lock_s" in result
        and (
            launch == "direct"
            or (result.get("exit_rc") == 0 and result["exit_s"] <= 20)
        )
    )
    print(json.dumps(result), flush=True)
    return result


def main():
    if sys.argv[1:] == ["--session"]:
        print(json.dumps(private_gui_context()))
        return
    output = Path(sys.argv[1])
    output.mkdir(parents=True, exist_ok=True)
    results = []
    # Chrome puts a Unix socket below TMPDIR; keep this diagnostic path short.
    with tempfile.TemporaryDirectory(prefix="cb-", dir="/tmp") as temp:
        root = Path(temp)
        cases = [("private", "open"), ("system", "open"), ("private", "direct")]
        try:
            for environment, launch in cases:
                try:
                    result = one(root, environment, launch)
                except Exception as error:
                    result = {
                        "environment": environment,
                        "mode": launch,
                        "error": str(error),
                    }
                results.append(result)
                (output / "browser-context.json").write_text(
                    json.dumps(results, indent=2) + "\n"
                )
                if (
                    len(results) == 3
                    and results[1].get("ready")
                    and not results[0].get("ready")
                ):
                    cases.extend([("home", "open"), ("temp", "open")])
        finally:
            # LaunchServices may finish a previous request after its client exited.
            for profile in root.glob("*/profile"):
                stop_profile(profile_owner(profile.resolve()))
    available = {
        r["environment"] for r in results if r.get("mode") == "open" and r.get("ready")
    }
    working = next(
        (m for m in ("private", "home", "temp", "system") if m in available), None
    )
    # Keep raw OS paths local to the runner. CCCC and browser profiles stay private.
    runner = Path(os.environ["RUNNER_TEMP"])
    (runner / "cccc-browser-environment.json").write_text(
        json.dumps(os_environment(working))
    )
    (runner / "cccc-browser-available").write_text("yes" if working else "no")


if __name__ == "__main__":
    main()
