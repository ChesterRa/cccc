from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Optional

from . import __version__
from .implementation import (
    ImplementationError,
    ImplementationName,
    daemon_implementation,
    implementation_lock_path,
    load_selected_implementation,
    require_rust_implementation,
    save_selected_implementation,
)
from .paths import ensure_home
from .util.file_lock import LockUnavailableError, acquire_lockfile, release_lockfile
from .util.process import pid_is_alive, terminate_pid

_SELECTORS = {"python", "rust"}
_PYTHON_META_COMMANDS = {"status", "update", "version"}
_PYTHON_GLOBAL_OPTIONS_WITH_VALUE = {"--host", "--web-host", "--port", "--web-port"}
_LAUNCHER_PATH_ENV = "CCCC_LAUNCHER_PATH"


def _public_launcher_path() -> Optional[Path]:
    raw = str(sys.argv[0] or "").strip()
    if raw:
        candidate = Path(raw).expanduser()
        if not candidate.is_absolute():
            located = shutil.which(raw)
            candidate = Path(located) if located else candidate
        try:
            candidate = candidate.resolve()
        except Exception:
            pass
        if candidate.name.lower() in {"cccc", "cccc.exe"} and candidate.is_file():
            return candidate
        if candidate.name.lower() in {"ccccd", "ccccd.exe"} and candidate.is_file():
            sibling = candidate.with_name("cccc.exe" if os.name == "nt" else "cccc")
            if sibling.is_file():
                return sibling.resolve()
    located = shutil.which("cccc")
    if not located:
        return None
    candidate = Path(located).resolve()
    if candidate.name.lower() not in {"cccc", "cccc.exe"} or not candidate.is_file():
        return None
    return candidate


def _python_main(argv: list[str]) -> int:
    from .cli.main import main as python_main

    return int(
        python_main(
            argv,
            before_product_update=lambda: _stop_active_processes(ensure_home()),
        )
    )


def _dispatch_rust(binary: Path, argv: list[str]) -> int:
    env = os.environ.copy()
    launcher = _public_launcher_path()
    if launcher is not None:
        env[_LAUNCHER_PATH_ENV] = str(launcher)
    command = [str(binary), *argv]
    if os.name == "nt":
        return int(subprocess.call(command, env=env))
    os.execve(str(binary), command, env)
    raise AssertionError("os.execve returned unexpectedly")


def _dispatch(implementation: ImplementationName, argv: list[str]) -> int:
    if implementation == "python":
        return _python_main(argv)
    return _dispatch_rust(require_rust_implementation(), argv)


def _dispatch_after_switch(
    implementation: ImplementationName,
    argv: list[str],
    *,
    previous: Optional[ImplementationName],
) -> int:
    try:
        return _dispatch(implementation, argv)
    except (ImplementationError, OSError) as launch_error:
        if previous is not None and previous != implementation:
            try:
                save_selected_implementation(previous)
            except Exception as rollback_error:
                raise ImplementationError(
                    f"could not launch {implementation} ({launch_error}); "
                    f"could not restore {previous} selection ({rollback_error})"
                ) from launch_error
        raise


def _rust_web_launcher_pid(home: Path) -> int:
    path = home / "daemon" / "cccc-web.lock"
    if not path.exists():
        return 0
    try:
        probe_lock = acquire_lockfile(path, blocking=False)
    except LockUnavailableError:
        pass
    except Exception:
        return 0
    else:
        # An unlocked file may contain a stale PID. Never signal it: the PID may
        # already have been recycled for an unrelated process.
        release_lockfile(probe_lock)
        return 0
    try:
        value = path.read_text(encoding="utf-8").strip()
        return int(value) if value.isdigit() else 0
    except Exception:
        return 0


def _stop_active_processes(home: Path) -> None:
    from .cli.common import _stop_existing_daemon, _stop_existing_web_runtime

    if not _stop_existing_web_runtime(home):
        raise ImplementationError("could not stop the running Python Web process")

    rust_web_pid = _rust_web_launcher_pid(home)
    if rust_web_pid > 0 and rust_web_pid != os.getpid() and pid_is_alive(rust_web_pid):
        if not terminate_pid(rust_web_pid, timeout_s=4.0, include_group=True, force=True):
            raise ImplementationError(f"could not stop the running Rust Web process (pid={rust_web_pid})")

    if not _stop_existing_daemon(home):
        raise ImplementationError("could not stop the running CCCC daemon")


def _ping_daemon() -> dict[str, object]:
    from .daemon.server import call_daemon

    return call_daemon({"op": "ping"}, timeout_s=1.0)


def _switch(target: ImplementationName) -> Optional[ImplementationName]:
    home = ensure_home()
    try:
        lock = acquire_lockfile(implementation_lock_path(home), blocking=False)
    except LockUnavailableError as error:
        raise ImplementationError("another CCCC implementation switch is already in progress") from error
    try:
        try:
            previous = load_selected_implementation(home)
        except ImplementationError:
            # An explicit selector is also the recovery path for a corrupt or
            # newer selection file. Commands without a selector still fail.
            previous = None
        if target == "rust":
            require_rust_implementation()

        ping = _ping_daemon()
        running = daemon_implementation(ping)
        if target != previous or (running is not None and running != target):
            _stop_active_processes(home)
        save_selected_implementation(target, home)
        return previous
    finally:
        release_lockfile(lock)


def _print_launcher_help() -> None:
    print("CCCC implementation selectors:")
    print("  cccc python [COMMAND ...]  Select Python persistently, then run COMMAND")
    print("  cccc rust [COMMAND ...]    Select Rust persistently, then run COMMAND")
    print("  cccc status                Show selected, running, and available implementations")
    print()


def _run_python_meta(argv: list[str]) -> int:
    return _python_main(argv)


def _command_index_after_global_options(argv: list[str]) -> Optional[int]:
    """Return the first command token after supported top-level options."""
    index = 0
    while index < len(argv):
        option, separator, _value = argv[index].partition("=")
        if option not in _PYTHON_GLOBAL_OPTIONS_WITH_VALUE:
            return index
        if separator:
            index += 1
            continue
        if index + 1 >= len(argv):
            return None
        index += 2
    return None


def main(argv: Optional[list[str]] = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    selected_for_invocation: Optional[ImplementationName] = None
    previous_selection: Optional[ImplementationName] = None
    try:
        if args and args[0] in _SELECTORS:
            target: ImplementationName = args.pop(0)  # type: ignore[assignment]
            previous_selection = _switch(target)
            selected_for_invocation = target
            if not args:
                return _dispatch_after_switch(target, [], previous=previous_selection)

        if not args:
            return _dispatch(load_selected_implementation(), [])

        if args in (["--help"], ["-h"], ["help"]):
            _print_launcher_help()
            selected = selected_for_invocation or load_selected_implementation()
            return _dispatch_after_switch(selected, ["--help"], previous=previous_selection)

        command_index = _command_index_after_global_options(args)
        if command_index is not None and args[command_index] in _PYTHON_META_COMMANDS:
            if command_index == 0 and args[command_index] == "version":
                print(__version__)
                return 0
            return _run_python_meta(args)

        selected = selected_for_invocation or load_selected_implementation()
        return _dispatch_after_switch(selected, args, previous=previous_selection)
    except ImplementationError as error:
        print(f"cccc: {error}", file=sys.stderr)
        return 1
    except OSError as error:
        print(f"cccc: could not launch the selected implementation: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
