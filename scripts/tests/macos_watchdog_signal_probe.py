#!/usr/bin/env python3
"""Exercise the packaged watchdog entry on macOS, without a daemon or provider.

This verifies the production entry's signal policy and EOF exit with an empty
watch list. Native process-group cleanup is checked separately by the runtime
guard tests; this smoke does not claim it has terminated a resource.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time


def require_alive(child: subprocess.Popen, description: str) -> None:
    if child.poll() is not None:
        raise RuntimeError(f"watchdog ended {description}: {child.returncode}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("requires a real macOS host; another platform is not a pass")
    binary = args.binary.resolve(strict=True)
    if not os.access(binary, os.X_OK):
        parser.error("binary must be executable")
    checks = {}
    with tempfile.TemporaryDirectory(prefix="cccc-watchdog-smoke-") as directory:
        root = Path(directory)
        home = root / "home"
        home.mkdir(mode=0o700)
        env = {"HOME": str(home), "TMPDIR": directory, "PATH": "/usr/bin:/bin"}
        watch = root / "empty.watchdog"
        watch.write_text(
            json.dumps(
                {
                    "platform": "darwin",
                    "written_at": int(time.time()),
                    "ledger": {"version": 2, "records": []},
                }
            )
            + "\n"
        )
        child = subprocess.Popen(
            [str(binary), "__cccc-process-watchdog", str(watch)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=root,
            env=env,
        )
        try:
            # Allow the native loader and first instruction to run. This is a
            # smoke check, not a readiness handshake exposed by the product.
            time.sleep(1)
            require_alive(child, "before terminal signals")
            for sig in (signal.SIGHUP, signal.SIGINT):
                child.send_signal(sig)
                time.sleep(0.2)
                require_alive(child, f"after {sig.name}")
                checks[f"alive_after_{sig.name}"] = True
            child.stdin.close()
            child.stdin = None
            stdout, stderr = child.communicate(timeout=5)
            if child.returncode != 0:
                raise RuntimeError(
                    f"watchdog exit after EOF: {child.returncode}; "
                    f"stdout bytes={len(stdout)}; stderr: "
                    f"{stderr.decode(errors='replace')[:1000]}"
                )
            checks["empty_watch_eof_exit_zero"] = True
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate(timeout=5)
    result = {
        "platform": sys.platform,
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "checks": checks,
        "scope": "production entry signal mask and empty-watch EOF; no resource cleanup claim",
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
