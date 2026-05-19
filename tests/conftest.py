import os
import tempfile
import time

import pytest


@pytest.fixture(autouse=True)
def _inline_chat_post_commit_tasks():
    old_mode = os.environ.get("CCCC_CHAT_POST_COMMIT_MODE")
    os.environ["CCCC_CHAT_POST_COMMIT_MODE"] = "inline"
    try:
        yield
    finally:
        if old_mode is None:
            os.environ.pop("CCCC_CHAT_POST_COMMIT_MODE", None)
        else:
            os.environ["CCCC_CHAT_POST_COMMIT_MODE"] = old_mode


@pytest.fixture(autouse=True)
def _retry_temporary_directory_cleanup(monkeypatch):
    original_rmtree = tempfile.TemporaryDirectory._rmtree

    def retrying_rmtree(cls, name, ignore_errors=False, repeated=False):
        for attempt in range(6):
            try:
                return original_rmtree(name, ignore_errors=ignore_errors, repeated=repeated)
            except OSError:
                if ignore_errors or attempt >= 5:
                    raise
                time.sleep(0.05)
        return None

    monkeypatch.setattr(tempfile.TemporaryDirectory, "_rmtree", classmethod(retrying_rmtree))
