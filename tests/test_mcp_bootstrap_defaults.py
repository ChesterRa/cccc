import os
import unittest
from unittest.mock import patch


class TestMcpBootstrapDefaults(unittest.TestCase):
    def test_bootstrap_forwards_only_supported_defaults(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_test", "CCCC_ACTOR_ID": "peer1"}, clear=False), patch.object(
            mcp_server, "bootstrap", return_value={"ok": True}
        ) as mock_bootstrap:
            mcp_server.handle_tool_call("cccc_bootstrap", {})

        kwargs = mock_bootstrap.call_args.kwargs
        self.assertEqual(kwargs["group_id"], "g_test")
        self.assertEqual(kwargs["actor_id"], "peer1")
        self.assertEqual(kwargs["inbox_limit"], 50)
        self.assertEqual(kwargs["inbox_kind_filter"], "all")
        self.assertNotIn("ledger_tail_limit", kwargs)
        self.assertNotIn("ledger_tail_max_chars", kwargs)


if __name__ == "__main__":
    unittest.main()
