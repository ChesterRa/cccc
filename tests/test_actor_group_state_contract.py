import os
import tempfile
import unittest
from typing import get_args


class TestActorGroupStateContract(unittest.TestCase):
    def test_group_state_includes_stopped(self) -> None:
        from cccc.contracts.v1.actor import GroupState

        values = set(get_args(GroupState))
        self.assertIn("active", values)
        self.assertIn("idle", values)
        self.assertIn("paused", values)
        self.assertIn("stopped", values)

    def test_get_group_state_preserves_stopped(self) -> None:
        from cccc.kernel.group import create_group, get_group_state
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                reg = load_registry()
                group = create_group(reg, title="state-contract")
                group.doc["state"] = "stopped"
                group.save()

                self.assertEqual(get_group_state(group), "stopped")
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
