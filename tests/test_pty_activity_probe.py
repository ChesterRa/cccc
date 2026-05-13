from cccc.daemon.pty_activity_probe import read_pty_activity_signal


def test_read_pty_activity_signal_uses_override_without_tail() -> None:
    class Supervisor:
        tail_calls = 0

        @staticmethod
        def terminal_override(*, group_id: str, actor_id: str):
            return {"effective_working_state": "idle"}

        @classmethod
        def tail_output(cls, *, group_id: str, actor_id: str, max_bytes: int) -> bytes:
            cls.tail_calls += 1
            return b"ignored"

    signal = read_pty_activity_signal(Supervisor(), group_id="g1", actor_id="a1")

    assert signal.terminal_override == {"effective_working_state": "idle"}
    assert signal.terminal_text == ""
    assert Supervisor.tail_calls == 0


def test_read_pty_activity_signal_reads_tail_when_override_missing() -> None:
    class Supervisor:
        @staticmethod
        def terminal_override(*, group_id: str, actor_id: str):
            return None

        @staticmethod
        def tail_output(*, group_id: str, actor_id: str, max_bytes: int) -> bytes:
            return "previous output\n› \n".encode("utf-8")

    signal = read_pty_activity_signal(Supervisor(), group_id="g1", actor_id="a1")

    assert signal.terminal_override is None
    assert signal.terminal_text == "previous output\n› \n"


def test_read_pty_activity_signal_tolerates_probe_failures() -> None:
    class Supervisor:
        @staticmethod
        def terminal_override(*, group_id: str, actor_id: str):
            raise RuntimeError("missing session")

        @staticmethod
        def tail_output(*, group_id: str, actor_id: str, max_bytes: int) -> bytes:
            raise RuntimeError("missing session")

    signal = read_pty_activity_signal(Supervisor(), group_id="g1", actor_id="a1")

    assert signal.terminal_override is None
    assert signal.terminal_text == ""
