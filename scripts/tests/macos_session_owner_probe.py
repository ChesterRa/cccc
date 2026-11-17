#!/usr/bin/env python3
"""Probe Darwin mechanisms with private fixtures; never start CCCC or a provider."""

import argparse
import asyncio
import contextlib
import json
import os
from pathlib import Path
import platform
import selectors
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import time
import uuid

NATIVE_TIMEOUT = 20


@contextlib.contextmanager
def socket_alias(parent):
    with tempfile.TemporaryDirectory(prefix="ccmp-", dir="/tmp") as temporary:
        alias = Path(temporary)
        metadata = alias.stat()
        assert stat.S_IMODE(metadata.st_mode) == 0o700
        assert metadata.st_uid == os.getuid()
        (alias / "d").symlink_to(parent, target_is_directory=True)
        path = alias / "d" / "control.sock"
        assert len(os.fsencode(path)) < 104
        yield path


class Fixture:
    def __init__(self, arguments, environment):
        self.process = subprocess.Popen(
            [str(value) for value in arguments],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment,
            start_new_session=True,
            bufsize=0,
        )
        self.buffer = bytearray()

    def line(self):
        deadline = time.monotonic() + NATIVE_TIMEOUT
        with selectors.DefaultSelector() as selector:
            selector.register(self.process.stdout, selectors.EVENT_READ)
            while b"\n" not in self.buffer:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not selector.select(remaining):
                    raise RuntimeError("native fixture response timed out")
                chunk = os.read(self.process.stdout.fileno(), 4096)
                if not chunk:
                    status = self.process.poll()
                    diagnostic = (
                        self.process.stderr.read(2048).decode(errors="replace")
                        if status is not None
                        else ""
                    )
                    raise RuntimeError(
                        f"native fixture exited without a response: rc={status}, stderr={diagnostic}"
                    )
                self.buffer.extend(chunk)
                if len(self.buffer) > 16384:
                    raise RuntimeError("native fixture response exceeded its bound")
        line, _, rest = self.buffer.partition(b"\n")
        self.buffer = bytearray(rest)
        try:
            result = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"invalid native fixture frame: {line[:512]!r}") from error
        if result.get("ok") is not True:
            raise RuntimeError(f"native fixture failed: {result}")
        return result

    def command(self, value):
        self.process.stdin.write(value.encode() + b"\n")
        self.process.stdin.flush()
        return self.line()

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=2)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            stream.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


async def async_exchange(listener, parent, checks):
    with socket_alias(parent) as path:
        alias = path.parent.parent
        reader, writer = await asyncio.wait_for(
            asyncio.open_unix_connection(str(path)), timeout=NATIVE_TIMEOUT
        )
    checks["socket.async_alias_removed"] = not alias.exists()
    peer, _ = listener.accept()
    with peer:
        writer.write(b"client")
        await writer.drain()
        checks["socket.async_client_to_server"] = peer.recv(64) == b"client"
        peer.sendall(b"server")
        checks["socket.async_server_to_client"] = (
            await asyncio.wait_for(reader.readexactly(6), timeout=NATIVE_TIMEOUT) == b"server"
        )
    writer.close()
    await writer.wait_closed()


def short_socket_probe(root, report):
    checks = report["checks"]
    listeners = []
    parents = []
    try:
        for index in range(2):
            parent = root / ("long-更新-" + "x" * 110) / f"home {index}" / "owner"
            parent.mkdir(parents=True, mode=0o700)
            parents.append(parent)
            real = parent / "control.sock"
            report.setdefault("real_socket_path_bytes", []).append(len(os.fsencode(real)))
            with socket.socket(socket.AF_UNIX) as direct:
                try:
                    direct.bind(str(real))
                except OSError:
                    checks[f"socket.{index}.direct_long_rejected"] = True
                else:
                    checks[f"socket.{index}.direct_long_rejected"] = False
                    real.unlink()
            listener = socket.socket(socket.AF_UNIX)
            listener.settimeout(NATIVE_TIMEOUT)
            listeners.append(listener)
            with socket_alias(parent) as path:
                alias = path.parent.parent
                listener.bind(str(path))
                listener.listen(4)
            checks[f"socket.{index}.bind_alias_removed"] = not alias.exists()
            checks[f"socket.{index}.stays_in_home"] = stat.S_ISSOCK(real.lstat().st_mode)
            os.chmod(real, 0o600)
            checks[f"socket.{index}.private_socket"] = stat.S_IMODE(real.stat().st_mode) == 0o600

        for index, (parent, listener) in enumerate(zip(parents, listeners)):
            with socket.socket(socket.AF_UNIX) as client:
                client.settimeout(NATIVE_TIMEOUT)
                with socket_alias(parent) as path:
                    alias = path.parent.parent
                    client.connect(str(path))
                checks[f"socket.{index}.connect_alias_removed"] = not alias.exists()
                peer, _ = listener.accept()
                with peer:
                    payload = f"home-{index}".encode()
                    client.sendall(payload)
                    checks[f"socket.{index}.correct_home"] = peer.recv(64) == payload
                    peer.sendall(b"reply:" + payload)
                    checks[f"socket.{index}.reply_after_alias_removed"] = (
                        client.recv(64) == b"reply:" + payload
                    )
        asyncio.run(async_exchange(listeners[0], parents[0], checks))
    finally:
        for listener in listeners:
            listener.close()


def one_shot(arguments, environment):
    result = subprocess.run(
        [str(value) for value in arguments],
        env=environment,
        capture_output=True,
        text=True,
        timeout=NATIVE_TIMEOUT,
        check=True,
    )
    value = json.loads(result.stdout)
    if value.get("ok") is not True:
        raise RuntimeError(f"identity fixture failed: {value}")
    return value


def same_birth(left, right):
    return all(
        left[key] == right[key]
        for key in ("pid", "birth_seconds", "birth_microseconds", "boot_uuid")
    )


def verify_signature(path, environment):
    subprocess.run(
        ["/usr/bin/codesign", "--verify", "--strict", str(path)],
        env=environment,
        capture_output=True,
        text=True,
        timeout=NATIVE_TIMEOUT,
        check=True,
    )


def darwin_probe(root, old, new, report, completed):
    checks = report["checks"]
    environment = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(root / "fixture-home"),
        "TMPDIR": str(root / "tmp"),
        "LC_ALL": "C",
    }
    for name in ("fixture-home", "tmp"):
        (root / name).mkdir(mode=0o700)
    install = root / "install space 更新"
    install.mkdir(mode=0o700)
    executable = install / "native-probe"
    staged = install / "staged-probe"
    backup = install / "old-probe"
    shutil.copy2(old, executable)
    shutil.copy2(new, staged)
    verify_signature(executable, environment)
    verify_signature(staged, environment)
    checks["rename.initial_signatures_valid"] = True

    with Fixture([executable, "--serve"], environment) as owner:
        before = owner.line()
        report["before"] = before
        boot = uuid.UUID(before["boot_uuid"])
        checks["api.boot_uuid_valid"] = boot.int != 0
        checks["api.non_root"] = os.geteuid() != 0
        checks["api.pid_matches_child"] = before["pid"] == owner.process.pid
        checks["api.uid_matches_user"] = before["uid"] == os.geteuid()
        checks["api.own_group"] = before["pgid"] == owner.process.pid
        checks["api.own_session"] = before["sid"] == owner.process.pid
        checks["api.birth_present"] = (
            before["birth_seconds"] > 0 and 0 <= before["birth_microseconds"] < 1_000_000
        )
        inspected = one_shot([executable, "--inspect", str(owner.process.pid)], environment)
        checks["api.inspector_matches_birth"] = same_birth(before, inspected)
        another = one_shot([staged, "--identity"], environment)
        checks["api.boot_matches_other_process"] = another["boot_uuid"] == before["boot_uuid"]
        absent = subprocess.run(
            [str(executable), "--inspect", "2147483647"],
            env=environment,
            capture_output=True,
            text=True,
            timeout=NATIVE_TIMEOUT,
            check=False,
        )
        missing = json.loads(absent.stdout)
        report["lookup_failure"] = missing
        checks["api.failed_lookup_explicit"] = (
            absent.returncode == 1
            and missing.get("ok") is False
            and missing.get("stage") == "bsd_info"
        )
        completed.add("darwin_api")

        parent = root / ("credential-" + "y" * 110)
        parent.mkdir(mode=0o700)
        with socket_alias(parent) as path:
            alias = path.parent.parent
            peer_fixture = Fixture([executable, "--peer", path], environment)
            try:
                ready = peer_fixture.line()
                checks["peer.server_ready"] = ready["stage"] == "peer_ready"
            except BaseException:
                peer_fixture.close()
                raise
        with peer_fixture:
            checks["peer.bind_alias_removed"] = not alias.exists()
            checks["peer.private_socket"] = (
                stat.S_IMODE((parent / "control.sock").stat().st_mode) == 0o600
            )
            with socket.socket(socket.AF_UNIX) as client:
                client.settimeout(NATIVE_TIMEOUT)
                with socket_alias(parent) as path:
                    client.connect(str(path))
                client.sendall(b"x")
                checks["peer.reply"] = client.recv(3) == b"ok\n"
            credentials = peer_fixture.line()
            report["peer_credentials"] = credentials
            checks["peer.kernel_uid"] = credentials["uid"] == os.geteuid()
            checks["peer.kernel_pid"] = credentials["pid"] == os.getpid()
            checks["peer.fixture_exit"] = peer_fixture.process.wait(timeout=NATIVE_TIMEOUT) == 0
        completed.add("peer_credentials")

        old_inode = executable.stat().st_ino
        executable.rename(backup)
        staged.rename(executable)
        checks["rename.new_inode"] = executable.stat().st_ino != old_inode
        backup.unlink()
        checks["rename.backup_removed"] = not backup.exists()
        still_old = owner.command("identity")
        report["after_replace_old_process"] = still_old
        checks["rename.old_mapping_continues"] = still_old["build"] == 1
        checks["rename.old_identity_stable"] = same_birth(before, still_old)
        verify_signature(executable, environment)
        checks["rename.replacement_signature_valid"] = True
        spawned = owner.command(f"spawn {executable}")
        completion = owner.line()
        report["old_process_spawned"] = spawned
        checks["rename.old_spawn_gets_new_build"] = spawned["build"] == 2
        checks["rename.spawn_parent_is_old_process"] = spawned["ppid"] == owner.process.pid
        checks["rename.spawn_completed"] = completion["stage"] == "spawn_complete"
        checks["rename.parent_still_old_after_spawn"] = owner.command("identity")["build"] == 1
        after_exec = owner.command(f"exec {executable}")
        report["after_exec"] = after_exec
        checks["rename.exec_gets_new_build"] = after_exec["build"] == 2
        checks["rename.exec_keeps_pid_and_birth"] = same_birth(before, after_exec)
        owner.process.stdin.write(b"quit\n")
        owner.process.stdin.flush()
        checks["rename.fixture_exit"] = owner.process.wait(timeout=NATIVE_TIMEOUT) == 0
        completed.add("signed_replace")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old", type=Path)
    parser.add_argument("--new", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--socket-only", action="store_true", help="Mechanism check, not Darwin evidence")
    args = parser.parse_args()
    if not args.socket_only:
        if sys.platform != "darwin" or platform.machine() != "arm64":
            parser.error("full probe requires native arm64 macOS")
        if args.old is None or args.new is None:
            parser.error("--old and --new signed native fixture binaries are required")
    required = {"short_socket"}
    if not args.socket_only:
        required |= {"darwin_api", "peer_credentials", "signed_replace"}
    completed = set()
    report = {
        "platform": sys.platform,
        "machine": platform.machine(),
        "os_release": platform.release(),
        "uid": os.getuid(),
        "darwin_evidence": not args.socket_only and sys.platform == "darwin",
        "checks": {},
        "unverified": [
            "Rust session-owner integration and watchdog recovery",
            "different-uid rejection and actual reboot",
            "provider CLIs, Gatekeeper download quarantine, and desktop host lifecycle",
        ],
    }
    root = None
    try:
        with tempfile.TemporaryDirectory(prefix="cccc-platform-probe-") as temporary:
            root = Path(temporary).resolve()
            short_socket_probe(root, report)
            completed.add("short_socket")
            if not args.socket_only:
                darwin_probe(root, args.old.resolve(), args.new.resolve(), report, completed)
        report["checks"]["fixture_removed"] = not root.exists()
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
        if isinstance(error, subprocess.CalledProcessError):
            report["native_stderr"] = (error.stderr or "")[:2048]
        report["checks"]["fixture_removed"] = root is not None and not root.exists()
    report["required_phases"] = sorted(required)
    report["completed_phases"] = sorted(completed)
    report["passed"] = (
        "error" not in report
        and completed == required
        and bool(report["checks"])
        and all(report["checks"].values())
    )
    serialized = json.dumps(report, indent=2, ensure_ascii=False) + "\n"
    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(serialized)
    print(serialized, end="")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
