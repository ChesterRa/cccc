import unittest


class TestMcpGroupBridgePairing(unittest.TestCase):
    def test_pairing_and_raw_delivery_tools_are_not_mcp_surface(self) -> None:
        from cccc.kernel.capabilities import BUILTIN_CAPABILITY_PACKS
        from cccc.ports.mcp.toolspecs import MCP_TOOLS

        names = {str(spec.get("name") or "") for spec in MCP_TOOLS}
        for tool_name in (
            "cccc_group_bridge_identity",
            "cccc_pairing_invite_create",
            "cccc_pairing_request_create",
            "cccc_pairing_request_list",
            "cccc_pairing_approve",
            "cccc_pairing_reject",
            "cccc_pairing_trust_list",
            "cccc_remote_send",
            "cccc_remote_delivery_status",
        ):
            self.assertNotIn(tool_name, names)

        pack_tools = set(BUILTIN_CAPABILITY_PACKS["pack:group_bridge"]["tool_names"])
        self.assertIn("cccc_remote_access", pack_tools)
        self.assertIn("cccc_remote_repo", pack_tools)
        self.assertIn("cccc_remote_exec_command", pack_tools)
        self.assertNotIn("cccc_group_bridge_identity", pack_tools)
        self.assertNotIn("cccc_pairing_invite_create", pack_tools)
        self.assertNotIn("cccc_pairing_request_create", pack_tools)
        self.assertNotIn("cccc_pairing_approve", pack_tools)
        self.assertNotIn("cccc_remote_send", pack_tools)
        self.assertNotIn("cccc_remote_delivery_status", pack_tools)


if __name__ == "__main__":
    unittest.main()
