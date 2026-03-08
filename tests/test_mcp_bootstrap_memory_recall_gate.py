import os
import unittest
from unittest.mock import patch


class TestMcpBootstrapMemoryRecallGate(unittest.TestCase):
    def test_bootstrap_returns_slim_packet_and_recovery_fields(self) -> None:
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
            cccc_group_actor,
            "group_info",
            return_value={
                "group": {
                    "group_id": "g_test",
                    "title": "temp_task",
                    "active_scope_key": "s1",
                    "scopes": [{"scope_key": "s1", "url": "/tmp/workspace"}],
                }
            },
        ), patch.object(
            cccc_group_actor,
            "actor_list",
            return_value={"actors": [{"id": "peer1", "role": "peer", "runner": "pty"}]},
        ), patch.object(
            cccc_core,
            "project_info",
            return_value={"found": True, "path": "/tmp/workspace/PROJECT.md"},
        ), patch.object(
            cccc_context,
            "context_get",
            return_value={
                "coordination": {
                    "brief": {"current_focus": "memory lifecycle", "objective": "Ship memory", "project_brief": "Hot project brief"},
                    "tasks": [{"id": "T001", "title": "B2 rollout", "outcome": "Ship memory", "status": "active", "assignee": "peer1"}],
                    "recent_decisions": [{"summary": "Keep lifecycle deterministic.", "by": "foreman", "at": "2026-03-07T00:00:00Z"}],
                    "recent_handoffs": [],
                },
                "agent_states": [{
                    "id": "peer1",
                    "hot": {"focus": "memory lane", "next_action": "verify recall gate", "active_task_id": "T001", "blockers": []},
                    "warm": {
                        "what_changed": "seeded",
                        "resume_hint": "open memory lane",
                        "environment_summary": "repo dirty but scoped",
                        "user_model": "prefers concise evidence",
                        "persona_notes": "be precise and low-noise",
                    },
                    "updated_at": "2026-03-07T00:00:00Z",
                }],
            },
        ), patch.object(
            cccc_core,
            "inbox_list",
            return_value={
                "messages": [
                    {
                        "id": "ev1",
                        "ts": "2026-03-07T00:00:00Z",
                        "kind": "chat.message",
                        "by": "user",
                        "data": {"text": "please verify memory lane", "reply_required": True},
                    }
                ]
            },
        ), patch.object(
            cccc_core,
            "_call_daemon_or_raise",
            side_effect=_fake_daemon,
        ):
            out = mcp_server.bootstrap(group_id="g_test", actor_id="peer1")

        self.assertEqual(set(out.keys()), {"session", "recovery", "inbox_preview", "memory_recall_gate", "next_calls"})
        self.assertNotIn("help", out)
        self.assertNotIn("context", out)
        self.assertNotIn("actors", out)
        self.assertNotIn("group", out)
        self.assertNotIn("ledger_tail", out)

        session = out["session"]
        self.assertEqual(session["group_id"], "g_test")
        self.assertEqual(session["group_title"], "temp_task")
        self.assertEqual(session["actor_id"], "peer1")
        self.assertEqual(session["role"], "peer")
        self.assertEqual(session["runner"], "pty")
        self.assertEqual(session["active_scope"]["scope_key"], "s1")
        self.assertEqual(session["active_scope"]["path"], "/tmp/workspace")
        self.assertTrue(bool(session["project_md"]["found"]))

        recovery = out["recovery"]
        self.assertEqual(recovery["self_state"]["hot"]["active_task_id"], "T001")
        self.assertEqual(recovery["self_state"]["recovery"]["environment_summary"], "repo dirty but scoped")
        self.assertEqual(recovery["self_state"]["recovery"]["user_model"], "prefers concise evidence")
        self.assertEqual(recovery["self_state"]["recovery"]["persona_notes"], "be precise and low-noise")
        self.assertEqual(recovery["task_slice"]["assigned_active"][0]["id"], "T001")
        self.assertEqual(recovery["recent_notes"]["decisions"][0]["summary"], "Keep lifecycle deterministic.")

        inbox_preview = out["inbox_preview"]
        self.assertEqual(inbox_preview["messages"][0]["id"], "ev1")
        self.assertTrue(inbox_preview["messages"][0]["reply_required"] is True)
        self.assertEqual(inbox_preview["messages"][0]["text_preview"], "please verify memory lane")

        gate = out["memory_recall_gate"]
        self.assertTrue(bool(gate.get("required")))
        self.assertEqual(str(gate.get("status") or ""), "ready")
        hits = gate.get("hits") if isinstance(gate.get("hits"), list) else []
        self.assertGreaterEqual(len(hits), 1)

    def test_recall_gate_query_uses_rich_warm_cues_when_hot_cues_are_missing(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_core, cccc_group_actor
        from cccc.ports.mcp.handlers import context as cccc_context

        captured = {"query": ""}

        def _fake_daemon(req, *args, **kwargs):
            op = str(req.get("op") or "")
            if op == "memory_reme_search":
                args = req.get("args") if isinstance(req.get("args"), dict) else {}
                captured["query"] = str(args.get("query") or "")
                return {"hits": []}
            return {}

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_test", "CCCC_ACTOR_ID": "peer1"}, clear=False), patch.object(
            cccc_group_actor,
            "group_info",
            return_value={"group": {"group_id": "g_test", "title": "temp_task", "active_scope_key": "s1", "scopes": []}},
        ), patch.object(
            cccc_group_actor,
            "actor_list",
            return_value={"actors": [{"id": "peer1", "role": "peer", "runner": "pty"}]},
        ), patch.object(
            cccc_core,
            "project_info",
            return_value={"found": False, "path": None},
        ), patch.object(
            cccc_context,
            "context_get",
            return_value={
                "coordination": {"brief": {"objective": "Stabilize collaboration"}, "tasks": [], "recent_decisions": [], "recent_handoffs": []},
                "agent_states": [{
                    "id": "peer1",
                    "hot": {"focus": "", "next_action": "", "active_task_id": None, "blockers": []},
                    "warm": {
                        "what_changed": "",
                        "resume_hint": "re-check shared assumptions",
                        "environment_summary": "workspace has multiple parallel edits",
                        "user_model": "cares about ROI and low noise",
                        "persona_notes": "ask before overbuilding",
                    },
                }],
            },
        ), patch.object(
            cccc_core,
            "inbox_list",
            return_value={"messages": []},
        ), patch.object(
            cccc_core,
            "_call_daemon_or_raise",
            side_effect=_fake_daemon,
        ):
            out = mcp_server.bootstrap(group_id="g_test", actor_id="peer1")

        gate = out["memory_recall_gate"]
        self.assertTrue(str(gate.get("query") or "").strip())
        query = captured["query"]
        self.assertIn("re-check shared assumptions", query)
        self.assertIn("workspace has multiple parallel edits", query)
        self.assertIn("cares about ROI and low noise", query)


if __name__ == "__main__":
    unittest.main()
