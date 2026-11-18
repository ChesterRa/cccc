"""Run a single exact libtest with a process-level deadline and a private browser home."""

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

CASES = {
    "connect": "system_browser_answers_cdp_and_stops_its_original_process",
    "cleanup": "failed_cdp_connection_stops_the_browser_opened_for_its_profile",
}


def read_command(args):
    try:
        result = subprocess.run(
            args, capture_output=True, text=True, timeout=2, check=False
        )
        return result.stdout.strip() if result.returncode == 0 else ""
    except (OSError, subprocess.TimeoutExpired):
        return ""


def process_row(pid):
    values = read_command(
        ["/bin/ps", "-ww", "-p", str(pid), "-o", "ppid=,stat=,comm="]
    ).split(maxsplit=2)
    if len(values) == 3 and values[0].isdigit():
        return {
            "pid": pid,
            "ppid": int(values[0]),
            "state": values[1],
            "program": values[2],
        }
    return None


def owned_processes(proc, root, evidence):
    children = []
    profiles = []
    for word in read_command(["/usr/bin/pgrep", "-P", str(proc.pid)]).split():
        if (
            word.isdigit()
            and (row := process_row(int(word)))
            and row["ppid"] == proc.pid
        ):
            children.append(row)
    # Each test makes its single profile directly below its private TMPDIR.
    with os.scandir(root / "tmp") as entries:
        locks = [
            Path(entry.path) / "SingletonLock"
            for entry in entries
            if entry.is_dir(follow_symlinks=False)
        ]
    for lock in locks:
        try:
            pid = int(os.readlink(lock).rsplit("-", 1)[1])
        except (IndexError, OSError, ValueError):
            continue
        if pid <= 0:
            continue
        command = read_command(
            ["/bin/ps", "-ww", "-p", str(pid), "-o", "command="]
        ).split()
        paths = {str(lock.parent), str(lock.parent.resolve())}
        if any("--user-data-dir=" + path in command for path in paths) and (
            row := process_row(pid)
        ):
            profiles.append(row)
    evidence.with_suffix(".processes.json").write_text(
        json.dumps(
            {
                "python": sys.version,
                "test": proc.pid,
                "children": children,
                "profile_processes": profiles,
            },
            indent=2,
        )
        + "\n"
    )
    return children


def capture_stack(pid, evidence, deadline):
    allowance = min(20, deadline - time.monotonic())
    if allowance <= 0:
        return
    try:
        subprocess.run(
            ["/usr/bin/sample", str(pid), "1", "10", "-file", str(evidence)],
            timeout=allowance,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass


def observe_stall(proc, root, evidence, deadline):
    children = owned_processes(proc, root, evidence)
    for row in children:
        # Read only the actual launcher directly owned by this test.
        if row["program"] not in ("/usr/bin/open", "open"):
            continue
        args = read_command(
            [
                "/bin/ps",
                "-ww",
                "-p",
                str(row["pid"]),
                "-o",
                "ppid=,command=",
            ]
        ).split(maxsplit=2)
        if args[:2] == [str(proc.pid), "/usr/bin/open"]:
            capture_stack(
                row["pid"],
                evidence.with_suffix(".open.sample"),
                deadline,
            )
    if proc.poll() is None:
        capture_stack(proc.pid, evidence.with_suffix(".test.sample"), deadline)


def main():
    case = sys.argv[1]
    if sys.argv[2:] not in ([], ["--trace-open"]):
        raise SystemExit("expected CASE [--trace-open]")
    trace = len(sys.argv) == 3
    name = "browser_surface::system_browser::macos_tests::" + CASES[case]
    runner = Path(os.environ["RUNNER_TEMP"])
    rows = (
        json.loads(s) for s in (runner / "native-build.jsonl").read_text().splitlines()
    )
    bins = {
        Path(r["executable"])
        for r in rows
        if r.get("reason") == "compiler-artifact"
        and r.get("executable")
        and r["target"]["name"] == "cccc_web"
        and "lib" in r["target"]["kind"]
        and r["profile"]["test"]
    }
    if len(bins) != 1:
        raise RuntimeError(f"Expected one Web unit test executable, found {len(bins)}")
    binary = bins.pop()
    label = case + "-trace" if trace else case
    evidence = runner / "cccc-native-tests" / (label + ".log")
    evidence.parent.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="cccc-native-", dir="/tmp"))
    try:
        for sub in ("home", "tmp", "cccc", "codex", "claude"):
            (root / sub).mkdir()
        env = dict(
            PATH=os.environ["PATH"],
            HOME=str(root / "home"),
            TMPDIR=str(root / "tmp") + "/",
            CCCC_HOME=str(root / "cccc"),
            CODEX_HOME=str(root / "codex"),
            CLAUDE_CONFIG_DIR=str(root / "claude"),
            LANG="en_US.UTF-8",
        )
        if trace:
            env["CCCC_TEST_TRACE_MACOS_OPEN"] = "1"
        with evidence.open("wb") as log:
            started = time.monotonic()
            with subprocess.Popen(
                [str(binary), name, "--exact", "--nocapture", "--test-threads=1"],
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
            ) as proc:
                try:
                    rc = proc.wait(timeout=45)
                except subprocess.TimeoutExpired:
                    deadline = started + 90
                    try:
                        observe_stall(proc, root, evidence, deadline)
                    except Exception as error:
                        print(f"Browser diagnostics unavailable: {error}", flush=True)
                    try:
                        rc = proc.wait(
                            timeout=max(0, 90 - (time.monotonic() - started))
                        )
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait(timeout=10)
                        rc = 124
        result = evidence.read_text(errors="replace")
        print(result, flush=True)
        if rc or not re.search(r"test result: ok\. 1 passed; 0 failed;", result):
            raise SystemExit(rc or 1)
    finally:
        shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    main()
