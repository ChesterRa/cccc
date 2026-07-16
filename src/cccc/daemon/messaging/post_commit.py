"""Post-commit side-effect runner for chat operations."""

from __future__ import annotations

import logging
import os
import threading
from typing import Callable

from .post_commit_lanes import KeyedPostCommitLanes

logger = logging.getLogger("cccc.chat.post_commit")

_CHAT_POST_COMMIT_LANES = KeyedPostCommitLanes(thread_name_prefix="cccc-chat-post-commit", logger=logger)


def run_chat_post_commit(label: str, fn: Callable[[], None]) -> None:
    name = str(label or "chat-post-commit").strip() or "chat-post-commit"

    def _run() -> None:
        try:
            fn()
        except Exception:
            logger.exception("chat post-commit task failed label=%s", name)

    if str(os.environ.get("CCCC_CHAT_POST_COMMIT_MODE") or "").strip().lower() == "inline":
        _run()
        return

    thread = threading.Thread(target=_run, name=f"cccc-{name}", daemon=True)
    try:
        thread.start()
    except Exception:
        logger.exception("chat post-commit task could not start label=%s", name)


def run_group_chat_post_commit(group_id: str, label: str, fn: Callable[[], None]) -> None:
    name = str(label or "chat-post-commit").strip() or "chat-post-commit"

    if str(os.environ.get("CCCC_CHAT_POST_COMMIT_MODE") or "").strip().lower() == "inline":
        try:
            fn()
        except Exception:
            logger.exception("chat post-commit task failed label=%s group=%s", name, group_id)
        return

    _CHAT_POST_COMMIT_LANES.submit(str(group_id or "global").strip() or "global", name, fn)


def wait_for_chat_post_commit_lanes_for_tests(timeout: float = 2.0) -> bool:
    return _CHAT_POST_COMMIT_LANES.wait_for_idle_for_tests(timeout=timeout)


def reset_chat_post_commit_lanes_for_tests() -> None:
    _CHAT_POST_COMMIT_LANES.reset_for_tests()
