import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestGroupBridgeRouteLookup(unittest.TestCase):
    def test_resolves_active_trust_by_local_and_remote_group(self) -> None:
        from cccc.daemon.group_bridge.route_lookup import resolve_remote_group_route
        from cccc.kernel.group_bridge import pairing as pairing_kernel

        with tempfile.TemporaryDirectory() as td, patch("cccc.kernel.group_bridge.pairing.ensure_home", return_value=Path(td)):
            invite = pairing_kernel.create_pairing_invite(group_id="g_local")
            request = pairing_kernel.create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_remote",
                requester_group_title="Remote Group",
                requester_peer_id="peer_remote",
                requester_endpoint="http://remote.example:8848",
            )
            approved = pairing_kernel.approve_pairing_request(request["request_id"], approver_user_id="user-a")

            route = resolve_remote_group_route(group_id="g_local", remote_group_id="g_remote")

        self.assertIsNotNone(route)
        assert route is not None
        self.assertEqual(route.registration_id, approved["registration"]["registration_id"])
        self.assertEqual(route.remote_group_id, "g_remote")
        self.assertEqual(route.remote_group_title, "Remote Group")

    def test_ignores_revoked_trusts(self) -> None:
        from cccc.daemon.group_bridge.route_lookup import resolve_remote_group_route
        from cccc.kernel.group_bridge import pairing as pairing_kernel

        with tempfile.TemporaryDirectory() as td, patch("cccc.kernel.group_bridge.pairing.ensure_home", return_value=Path(td)):
            invite = pairing_kernel.create_pairing_invite(group_id="g_local")
            request = pairing_kernel.create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_remote",
                requester_peer_id="peer_remote",
                requester_endpoint="http://remote.example:8848",
            )
            approved = pairing_kernel.approve_pairing_request(request["request_id"], approver_user_id="user-a")
            pairing_kernel.revoke_trust(approved["trust"]["trust_id"], revoked_by="user-a")

            route = resolve_remote_group_route(group_id="g_local", remote_group_id="g_remote")

        self.assertIsNone(route)


if __name__ == "__main__":
    unittest.main()
