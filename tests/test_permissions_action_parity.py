import re
import unittest
from pathlib import Path
from typing import get_args


class TestPermissionsActionParity(unittest.TestCase):
    def test_group_action_literals_match_daemon_usage(self) -> None:
        from cccc.kernel.permissions import GroupAction

        repo_root = Path(__file__).resolve().parents[1]
        daemon_server = repo_root / "src" / "cccc" / "daemon" / "server.py"
        text = daemon_server.read_text(encoding="utf-8")

        used = set(re.findall(r'require_group_permission\(group,\s*by=by,\s*action="([^"]+)"\)', text))
        declared = set(get_args(GroupAction))
        self.assertEqual(
            sorted(used - declared),
            [],
            msg=f"GroupAction missing literals used by daemon: {sorted(used - declared)}",
        )

    def test_actor_action_literals_match_daemon_usage(self) -> None:
        from cccc.kernel.permissions import ActorAction

        repo_root = Path(__file__).resolve().parents[1]
        daemon_server = repo_root / "src" / "cccc" / "daemon" / "server.py"
        text = daemon_server.read_text(encoding="utf-8")

        used = set(re.findall(r'require_actor_permission\(group,\s*by=by,\s*action="([^"]+)"', text))
        declared = set(get_args(ActorAction))
        self.assertEqual(
            sorted(used - declared),
            [],
            msg=f"ActorAction missing literals used by daemon: {sorted(used - declared)}",
        )


if __name__ == "__main__":
    unittest.main()
