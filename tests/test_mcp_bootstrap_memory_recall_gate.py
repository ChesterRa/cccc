import os
import unittest
from unittest.mock import patch


class TestMcpBootstrapMemoryRecallGate(unittest.TestCase):
    def test_bootstrap_returns_memory_recall_gate(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_core, cccc_group_actor
        from cccc.ports.mcp.handlers import context as cccc_context

        def _fake_daemon(req, *args, **kwargs):
            op = str(req.get("op") or "")
            if op == "memory_reme_search":
                return {
                    "hits": [
                        {
                            "path": "/tmp/memory/MEMORY.md",
                            "start_line": 12,
                            "end_line": 18,
                            "score": 0.98,
                            "snippet": "Stable decision: keep lifecycle deterministic.",
                        }
                    ]
                }
            return {}

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_test", "CCCC_ACTOR_ID": "peer1"}, clear=False), patch.object(
            cccc_group_actor, "group_info", return_value={"group": {"group_id": "g_test"}}
        ), patch.object(
            cccc_group_actor, "actor_list", return_value={"actors": [{"id": "peer1"}]}
        ), patch.object(
            cccc_core, "project_info", return_value={"found": False}
        ), patch.object(
            cccc_context,
            "context_get",
            return_value={
                "overview": {"manual": {"current_focus": "memory lifecycle"}},
                "tasks": [{"id": "T001", "name": "B2 rollout", "status": "active"}],
                "agents": [{"id": "peer1", "focus": "memory lane", "next_action": "verify recall gate"}],
            },
        ), patch.object(
            cccc_core, "inbox_list", return_value={"messages": []}
        ), patch.object(
            cccc_core, "load_group", return_value=None
        ), patch.object(
            cccc_core, "_append_runtime_skill_digest", side_effect=lambda markdown, **_: markdown
        ), patch.object(
            cccc_core, "_call_daemon_or_raise", side_effect=_fake_daemon
        ):
            out = mcp_server.bootstrap(group_id="g_test", actor_id="peer1")

        gate = out.get("memory_recall_gate") if isinstance(out, dict) else None
        self.assertIsInstance(gate, dict)
        assert isinstance(gate, dict)
        self.assertTrue(bool(gate.get("required")))
        self.assertEqual(str(gate.get("status") or ""), "ready")
        self.assertTrue(str(gate.get("query") or "").strip())
        hits = gate.get("hits") if isinstance(gate.get("hits"), list) else []
        self.assertGreaterEqual(len(hits), 1)


if __name__ == "__main__":
    unittest.main()

