import os
import tempfile
import unittest


class TestMcpGroupBridgePairing(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return cleanup

    def test_pairing_tools_are_registered(self) -> None:
        from cccc.ports.mcp.toolspecs import MCP_TOOLS

        names = {str(spec.get("name") or "") for spec in MCP_TOOLS}
        self.assertIn("cccc_group_bridge_identity", names)
        self.assertIn("cccc_pairing_invite_create", names)
        self.assertIn("cccc_pairing_request_create", names)
        self.assertIn("cccc_pairing_request_list", names)
        self.assertIn("cccc_pairing_approve", names)
        self.assertIn("cccc_pairing_reject", names)

    def test_pairing_handler_invite_request_approve_flow(self) -> None:
        from cccc.ports.mcp.handlers import cccc_group_bridge

        cleanup = self._with_home()
        try:
            identity = cccc_group_bridge.group_bridge_identity()
            self.assertTrue(identity["identity"]["peer_id"].startswith("12D3Koo"))

            invite = cccc_group_bridge.pairing_invite_create(
                group_id="g_local",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
                ttl_seconds=600,
            )
            self.assertRegex(invite["invite"]["pairing_code"], r"^[A-Z0-9]{4}-[A-Z0-9]{4}$")

            req = cccc_group_bridge.pairing_request_create(
                pairing_code=invite["invite"]["pairing_code"],
                requester_group_id="g_remote",
                requester_peer_id="peer_remote",
                requester_endpoint="http://remote.example:8848",
                requester_multiaddrs=["/ip4/127.0.0.1/tcp/4001/p2p/peer_remote"],
            )
            self.assertEqual(req["request"]["status"], "pending")

            listed = cccc_group_bridge.pairing_request_list(group_id="g_local")
            self.assertEqual([item["request_id"] for item in listed["requests"]], [req["request"]["request_id"]])

            approved = cccc_group_bridge.pairing_approve(
                request_id=req["request"]["request_id"],
                approver_user_id="user-a",
            )
            self.assertEqual(approved["request"]["status"], "approved")
            self.assertEqual(approved["registration"]["transport"], "group_bridge_session")
            self.assertEqual(approved["registration"]["url"], "http://remote.example:8848")

            trusts = cccc_group_bridge.pairing_trust_list(group_id="g_local")
            self.assertEqual(trusts["trusts"][0]["registration_id"], approved["registration"]["registration_id"])
        finally:
            cleanup()

    def test_pairing_reject_handler(self) -> None:
        from cccc.ports.mcp.handlers import cccc_group_bridge

        cleanup = self._with_home()
        try:
            invite = cccc_group_bridge.pairing_invite_create(
                group_id="g_local",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
            )
            req = cccc_group_bridge.pairing_request_create(
                pairing_code=invite["invite"]["pairing_code"],
                requester_group_id="g_remote",
                requester_peer_id="peer_remote",
            )
            rejected = cccc_group_bridge.pairing_reject(
                request_id=req["request"]["request_id"],
                rejected_by="user-a",
                reason="no",
            )
            self.assertEqual(rejected["request"]["status"], "rejected")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
