import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from cccc.ports.im import bridge
from cccc.util.file_lock import LockUnavailableError


class _DummyLockFile:
    def __init__(self) -> None:
        self.writes: list[bytes] = []

    def seek(self, _offset: int) -> None:
        return None

    def write(self, data: bytes) -> None:
        self.writes.append(data)

    def flush(self) -> None:
        return None


class TestImBridgeSingletonLock(unittest.TestCase):
    def test_acquire_singleton_lock_does_not_unlink_or_retry_when_lock_is_busy(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            lock_path = Path(td) / "im_bridge.lock"
            lock_path.write_text("4321\n", encoding="utf-8")
            handle = _DummyLockFile()

            with patch.object(
                bridge,
                "acquire_lockfile",
                side_effect=[LockUnavailableError("busy"), handle],
            ) as acquire:
                result = bridge._acquire_singleton_lock(lock_path)

            self.assertIsNone(result)
            self.assertEqual(acquire.call_count, 1)
            self.assertTrue(lock_path.exists())
            self.assertEqual(lock_path.read_text(encoding="utf-8"), "4321\n")
            self.assertFalse(handle.writes)

    def test_acquire_singleton_lock_keeps_live_pid_lock(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            lock_path = Path(td) / "im_bridge.lock"
            lock_path.write_text("4321\n", encoding="utf-8")

            with patch.object(bridge, "acquire_lockfile", side_effect=LockUnavailableError("busy")) as acquire:
                result = bridge._acquire_singleton_lock(lock_path)

            self.assertIsNone(result)
            self.assertEqual(acquire.call_count, 1)
            self.assertTrue(lock_path.exists())
