from __future__ import annotations

import threading
from types import SimpleNamespace

from cccc.runners import pty
from cccc.runners import pty_win


def test_session_observer_fires_once_after_successful_write(monkeypatch) -> None:
    seen: list[tuple[str, str, bytes]] = []
    session = pty.PtySession.__new__(pty.PtySession)
    session.group_id = "g1"
    session.actor_id = "peer"
    session._master_fd = 7
    session._input_observer = lambda current, data: seen.append(
        (current.group_id, current.actor_id, data)
    )
    monkeypatch.setattr(pty.os, "write", lambda fd, data: len(data))

    assert session.write_input(b"payload") is True
    assert seen == [("g1", "peer", b"payload")]


def test_failed_write_does_not_advance_input_observer(monkeypatch) -> None:
    seen: list[bytes] = []
    session = pty.PtySession.__new__(pty.PtySession)
    session.group_id = "g1"
    session.actor_id = "peer"
    session._master_fd = 7
    session._input_observer = lambda _current, data: seen.append(data)

    def fail(_fd: int, _data: bytes) -> int:
        raise OSError("closed")

    monkeypatch.setattr(pty.os, "write", fail)
    assert session.write_input(b"\r") is False
    assert seen == []


def test_raw_attached_socket_uses_observed_write_path() -> None:
    class _Socket:
        def recv(self, _limit: int) -> bytes:
            return b"\r"

    session = pty.PtySession.__new__(pty.PtySession)
    session._lock = threading.Lock()
    session._clients = {4: SimpleNamespace(sock=_Socket(), writer=True)}
    session._writer_fd = 4
    received: list[bytes] = []
    session.write_input = lambda data: received.append(data) or True
    session.detach_client = lambda _fd: None

    session._on_client_readable(4)
    assert received == [b"\r"]


def test_windows_session_observer_fires_after_successful_write() -> None:
    seen: list[bytes] = []
    session = pty_win.PtySession.__new__(pty_win.PtySession)
    session.group_id = "g1"
    session.actor_id = "peer"
    session._proc = SimpleNamespace(write=lambda _text: None)
    session._input_observer = lambda _current, data: seen.append(data)
    assert session.write_input(b"\r") is True
    assert seen == [b"\r"]


def test_session_exit_revokes_bound_input_capability() -> None:
    supervisor = pty.PtySupervisor()
    session = pty.PtySession.__new__(pty.PtySession)
    session.group_id = "g1"
    session.actor_id = "peer"
    session._proc = SimpleNamespace(pid=123, poll=lambda: 0)
    session._input_observer = lambda _current, _data: None
    session._runtime_hook_capability = object()
    session.is_running = lambda: True
    supervisor._sessions[("g1", "peer")] = session

    supervisor._on_session_exit(session)

    assert session._input_observer is None
    assert session.input_capability() is None
    assert (
        supervisor.input_capability(
            group_id="g1", actor_id="peer"
        )
        is None
    )
