from __future__ import annotations

from cccc.runners.deepseek import DeepSeekSupervisor


class _Process:
    def __init__(self, stdin) -> None:
        self.stdin = stdin


def test_active_request_is_registered_before_write(tmp_path) -> None:
    supervisor = DeepSeekSupervisor([], cwd=str(tmp_path), env={})

    class FastStdin:
        def write(self, payload: bytes) -> int:
            assert supervisor._active_request_id == 3
            return len(payload)

        def flush(self) -> None:
            assert supervisor._active_request_id == 3

    supervisor._process = _Process(FastStdin())
    supervisor._send_request(
        {"jsonrpc": "2.0", "id": 3, "method": "session/prompt"},
        request_id=3,
        activate=True,
    )

    assert supervisor._active_request_id == 3
    assert 3 in supervisor._protocol.pending


def test_failed_write_rolls_back_active_request_and_protocol_slot(tmp_path) -> None:
    supervisor = DeepSeekSupervisor([], cwd=str(tmp_path), env={})

    class BrokenStdin:
        def write(self, _payload: bytes) -> int:
            raise BrokenPipeError("closed")

    supervisor._process = _Process(BrokenStdin())
    try:
        supervisor._send_request(
            {"jsonrpc": "2.0", "id": 3, "method": "session/prompt"},
            request_id=3,
            activate=True,
        )
    except BrokenPipeError:
        pass
    else:
        raise AssertionError("broken stdin must fail")

    assert supervisor._active_request_id is None
    assert 3 not in supervisor._protocol.pending
