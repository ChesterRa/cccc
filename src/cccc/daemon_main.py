from __future__ import annotations

import argparse
import os
import signal
import subprocess
import sys
from pathlib import Path
from typing import Optional

from .daemon.server import DaemonPaths, call_daemon, default_paths, read_pid, serve_forever


def _spawn_daemon(paths: DaemonPaths) -> int:
    paths.daemon_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CCCC_HOME"] = str(paths.home)
    with paths.log_path.open("a", encoding="utf-8") as log_f:
        p = subprocess.Popen(
            [sys.executable, "-m", "cccc.daemon_main", "run"],
            stdout=log_f,
            stderr=log_f,
            stdin=subprocess.DEVNULL,
            env=env,
            start_new_session=True,
            cwd=str(paths.home),
        )
    return int(p.pid)


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
            try:
                os.kill(pid, 0)  # Check if process exists
                print(f"ccccd: pid file points to a live process (pid={pid}) but IPC is not responding; refusing to spawn duplicate daemon")
                return 1
            except OSError:
                # Process doesn't exist, clean up stale files
                print("ccccd: cleaning up stale state from crashed daemon")
                try:
                    paths.sock_path.unlink(missing_ok=True)
                    paths.addr_path.unlink(missing_ok=True)
                    paths.pid_path.unlink(missing_ok=True)
                except Exception:
                    pass
        pid = _spawn_daemon(paths)
        print(f"ccccd: started pid={pid}")
        return 0

    if args.cmd == "stop":
        resp = call_daemon({"op": "shutdown"}, paths=paths)
        if resp.get("ok"):
            print("ccccd: shutdown requested")
            return 0
        pid = read_pid(paths)
        if pid > 0:
            try:
                os.kill(pid, signal.SIGTERM)
                print("ccccd: SIGTERM sent")
                return 0
            except Exception:
                pass
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
