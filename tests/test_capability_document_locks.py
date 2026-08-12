from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from cccc.daemon.ops.capability_ops import (
    _CATALOG_LOCK,
    _POLICY_LOCK,
    _RUNTIME_LOCK,
    _STATE_LOCK,
)


class TestCapabilityDocumentLocks(unittest.TestCase):
    def test_python_locks_use_the_cross_engine_lockfiles(self) -> None:
        probe = """
import sys
from pathlib import Path
from cccc.util.file_lock import LockUnavailableError, acquire_lockfile, release_lockfile

try:
    held = acquire_lockfile(Path(sys.argv[1]), blocking=False)
except LockUnavailableError:
    raise SystemExit(23)
else:
    release_lockfile(held)
"""
        with tempfile.TemporaryDirectory(prefix="cccc-capability-locks.") as raw_home:
            home = Path(raw_home)
            with patch.dict(os.environ, {"CCCC_HOME": str(home)}):
                for lock, filename in (
                    (_STATE_LOCK, "state.json.lock"),
                    (_CATALOG_LOCK, "catalog.json.lock"),
                    (_RUNTIME_LOCK, "runtime.json.lock"),
                    (_POLICY_LOCK, "capability-allowlist.user.yaml.lock"),
                ):
                    lock_path = (
                        home / "config" / filename
                        if filename.startswith("capability-allowlist")
                        else home / "state" / "capabilities" / filename
                    )
                    with self.subTest(filename=filename), lock:
                        result = subprocess.run(
                            [sys.executable, "-c", probe, str(lock_path)],
                            check=False,
                            capture_output=True,
                            text=True,
                        )
                        self.assertEqual(
                            result.returncode,
                            23,
                            result.stdout + result.stderr,
                        )

    def test_malformed_capability_state_fails_closed_without_overwrite(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        with tempfile.TemporaryDirectory(prefix="cccc-capability-corrupt.") as raw_home:
            home = Path(raw_home)
            with patch.dict(os.environ, {"CCCC_HOME": str(home)}):
                created, _ = handle_request(
                    DaemonRequest(op="group_create", args={"title": "probe", "topic": "", "by": "user"})
                )
                self.assertTrue(created.ok, getattr(created, "error", None))
                group_id = str((created.result or {}).get("group_id") or "")
                state_path = home / "state" / "capabilities" / "state.json"
                state_path.parent.mkdir(parents=True, exist_ok=True)
                malformed = b"{malformed"
                state_path.write_bytes(malformed)

                response, _ = handle_request(
                    DaemonRequest(
                        op="capability_enable",
                        args={
                            "group_id": group_id,
                            "by": "user",
                            "actor_id": "user",
                            "capability_id": "pack:automation",
                            "scope": "group",
                            "enabled": True,
                        },
                    )
                )

                self.assertFalse(response.ok)
                self.assertEqual(state_path.read_bytes(), malformed)


if __name__ == "__main__":
    unittest.main()
