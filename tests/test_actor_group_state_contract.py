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


if __name__ == "__main__":
    unittest.main()
