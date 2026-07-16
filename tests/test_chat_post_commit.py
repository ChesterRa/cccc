import threading
import time

from cccc.daemon.messaging import post_commit


def setup_function() -> None:
    post_commit.reset_chat_post_commit_lanes_for_tests()


def teardown_function() -> None:
    post_commit.reset_chat_post_commit_lanes_for_tests()


def test_group_serial_post_commit_preserves_fifo_when_first_task_blocks(monkeypatch) -> None:
    monkeypatch.delenv("CCCC_CHAT_POST_COMMIT_MODE", raising=False)
    first_started = threading.Event()
    release_first = threading.Event()
    ran: list[str] = []

    def first() -> None:
        first_started.set()
        assert release_first.wait(timeout=2.0)
        ran.append("first")

    def second() -> None:
        ran.append("second")

    post_commit.run_group_chat_post_commit("g1", "first", first)
    assert first_started.wait(timeout=2.0)

    post_commit.run_group_chat_post_commit("g1", "second", second)
    time.sleep(0.05)

    assert ran == []

    release_first.set()
    post_commit.wait_for_chat_post_commit_lanes_for_tests(timeout=2.0)

    assert ran == ["first", "second"]


def test_group_serial_post_commit_allows_other_groups_to_run(monkeypatch) -> None:
    monkeypatch.delenv("CCCC_CHAT_POST_COMMIT_MODE", raising=False)
    first_started = threading.Event()
    release_first = threading.Event()
    ran: list[str] = []

    def blocked_group_task() -> None:
        first_started.set()
        assert release_first.wait(timeout=2.0)
        ran.append("g1")

    def other_group_task() -> None:
        ran.append("g2")

    post_commit.run_group_chat_post_commit("g1", "blocked", blocked_group_task)
    assert first_started.wait(timeout=2.0)

    post_commit.run_group_chat_post_commit("g2", "other", other_group_task)
    deadline = time.monotonic() + 2.0
    while "g2" not in ran and time.monotonic() < deadline:
        time.sleep(0.01)

    assert ran == ["g2"]

    release_first.set()
    post_commit.wait_for_chat_post_commit_lanes_for_tests(timeout=2.0)

    assert ran == ["g2", "g1"]


def test_group_serial_post_commit_runs_task_inline_when_thread_start_fails(monkeypatch) -> None:
    from cccc.daemon.messaging import post_commit_lanes

    monkeypatch.delenv("CCCC_CHAT_POST_COMMIT_MODE", raising=False)
    ran: list[str] = []

    class FailingThread:
        def __init__(self, *args, **kwargs) -> None:
            pass

        def start(self) -> None:
            raise RuntimeError("thread limit")

    monkeypatch.setattr(post_commit_lanes.threading, "Thread", FailingThread)

    post_commit.run_group_chat_post_commit("g1", "deliver", lambda: ran.append("deliver"))

    assert ran == ["deliver"]
    assert post_commit.wait_for_chat_post_commit_lanes_for_tests(timeout=0.1)
