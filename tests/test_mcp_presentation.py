import os
import unittest
from unittest.mock import patch


class TestMcpPresentation(unittest.TestCase):
    def test_handle_tool_call_dispatches_get(self) -> None:
        import cccc.ports.mcp.server as mcp_server

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_test", "CCCC_ACTOR_ID": "peer-1"}, clear=False):
            with patch.object(mcp_server, "presentation_get", return_value={"ok": True, "result": {"presentation": {}}}) as get_mock:
                out = mcp_server.handle_tool_call("cccc_presentation", {"action": "get"})

        self.assertTrue(bool(out.get("ok")))
        get_mock.assert_called_once_with(group_id="g_test")

    def test_handle_tool_call_dispatches_publish_with_runtime_actor(self) -> None:
        import cccc.ports.mcp.server as mcp_server

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_demo", "CCCC_ACTOR_ID": "peer-42"}, clear=False):
            with patch.object(mcp_server, "presentation_publish", return_value={"ok": True}) as publish_mock:
                out = mcp_server.handle_tool_call(
                    "cccc_presentation",
                    {
                        "action": "publish",
                        "slot": "slot-3",
                        "title": "Demo",
                        "content": "# demo",
                    },
                )

        self.assertTrue(bool(out.get("ok")))
        publish_mock.assert_called_once_with(
            group_id="g_demo",
            actor_id="peer-42",
            slot="slot-3",
            card_type="",
            title="Demo",
            summary="",
            source_label="",
            source_ref="",
            content="# demo",
            table=None,
            path="",
            url="",
            blob_rel_path="",
        )

    def test_handle_tool_call_dispatches_clear_with_bool_coercion(self) -> None:
        import cccc.ports.mcp.server as mcp_server

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_demo", "CCCC_ACTOR_ID": "peer-7"}, clear=False):
            with patch.object(mcp_server, "presentation_clear", return_value={"ok": True}) as clear_mock:
                out = mcp_server.handle_tool_call(
                    "cccc_presentation",
                    {
                        "action": "clear",
                        "all": "true",
                    },
                )

        self.assertTrue(bool(out.get("ok")))
        clear_mock.assert_called_once_with(
            group_id="g_demo",
            actor_id="peer-7",
            slot="",
            clear_all=True,
        )


if __name__ == "__main__":
    unittest.main()
