from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path
from typing import Optional

from .daemon.server import DaemonPaths, call_daemon, default_paths, read_pid, serve_forever
from .ports.web.runtime_control import clear_web_runtime_state, read_web_runtime_state, web_runtime_pid_candidates
from .util.process import (
    resolve_background_python_argv,
    SOFT_TERMINATE_SIGNAL,
    best_effort_signal_pid,
    pid_is_alive,
    resolve_subprocess_argv,
    supervised_process_popen_kwargs,
    terminate_pid,
)


def _spawn_daemon(paths: DaemonPaths) -> int:
    paths.daemon_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CCCC_HOME"] = str(paths.home)
    with paths.log_path.open("a", encoding="utf-8") as log_f:
        p = subprocess.Popen(
            resolve_background_python_argv([sys.executable, "-m", "cccc.daemon_main", "run"]),
            stdout=log_f,
            stderr=log_f,
            stdin=subprocess.DEVNULL,
            env=env,
            cwd=str(paths.home),
            **supervised_process_popen_kwargs(),
        )
    return int(p.pid)


def _stop_supervised_web_runtime(paths: DaemonPaths) -> bool:
    try:
        runtime = read_web_runtime_state(home=paths.home)
    except Exception:
        runtime = {}
    candidate_pids = web_runtime_pid_candidates(runtime)
    for runtime_pid in candidate_pids:
        if not pid_is_alive(runtime_pid):
            continue
        if not terminate_pid(runtime_pid, timeout_s=2.0, include_group=True, force=True):
            return False
    try:
        clear_web_runtime_state(home=paths.home, pid=candidate_pids[0] if candidate_pids else None)
    except Exception:
        pass
    return True


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(prog="ccccd", description="CCCC vNext daemon (single writer)")
    sub = parser.add_subparsers(dest="cmd", required=True)

    sub.add_parser("run", help="Run daemon in foreground")
    sub.add_parser("start", help="Start daemon in background")
    sub.add_parser("stop", help="Stop daemon")
    sub.add_parser("status", help="Daemon status")

    args = parser.parse_args(argv)
    paths = default_paths()

    if args.cmd == "run":
        return int(serve_forever(paths))

    if args.cmd == "start":
        resp = call_daemon({"op": "ping"}, paths=paths)
        if resp.get("ok"):
            print("ccccd: already running")
            return 0
        # Clean up stale socket/pid if daemon is not actually running
        pid = read_pid(paths)
        if pid > 0:
            if pid_is_alive(pid):
                print(f"ccccd: pid file points to a live process (pid={pid}) but IPC is not responding; refusing to spawn duplicate daemon")
                return 1
            # The recorded process is gone; clean up stale state files.
            print("ccccd: cleaning up stale state from crashed daemon")
            try:
                paths.sock_path.unlink(missing_ok=True)
                paths.addr_path.unlink(missing_ok=True)
                paths.pid_path.unlink(missing_ok=True)
            except Exception as e:
                print(f"ccccd: failed to clean stale daemon state: {e}")
                return 1
        pid = _spawn_daemon(paths)
        print(f"ccccd: started pid={pid}")
        return 0

    if args.cmd == "stop":
        resp = call_daemon({"op": "shutdown"}, paths=paths)
        if resp.get("ok"):
            if not _stop_supervised_web_runtime(paths):
                print("ccccd: shutdown requested, but failed to stop supervised web runtime")
                return 1
            print("ccccd: shutdown requested")
            return 0
        pid = read_pid(paths)
        if pid > 0:
            try:
                if best_effort_signal_pid(pid, SOFT_TERMINATE_SIGNAL, include_group=True):
                    if not _stop_supervised_web_runtime(paths):
                        print("ccccd: SIGTERM sent, but failed to stop supervised web runtime")
                        return 1
                    print("ccccd: SIGTERM sent")
                    return 0
                raise RuntimeError("signal not delivered")
            except Exception as e:
                print(f"ccccd: failed to signal pid={pid}: {e}")
                return 1
        if not _stop_supervised_web_runtime(paths):
            print("ccccd: daemon not running, but failed to stop supervised web runtime")
            return 1
        print("ccccd: not running")
        return 0

    if args.cmd == "status":
        resp = call_daemon({"op": "ping"}, paths=paths)
        if resp.get("ok"):
            r = resp.get("result") if isinstance(resp.get("result"), dict) else {}
            print(f"ccccd: running pid={r.get('pid')} version={r.get('version')}")
            return 0
        print("ccccd: not running")
        return 1

    return 2


if __name__ == "__main__":
    raise SystemExit(main())
