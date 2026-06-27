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
                    "tasks": [
                        {
                            "id": "T001",
                            "title": "B2 rollout",
                            "outcome": "Ship memory",
                            "parent_id": "T000",
                            "status": "active",
                            "assignee": "peer1",
                            "task_type": "optimization",
                        }
                    ],
                    "recent_decisions": [{"summary": "Keep lifecycle deterministic.", "by": "foreman", "at": "2026-03-07T00:00:00Z"}],
                    "recent_handoffs": [],
                },
                "agent_states": [{
                    "id": "peer1",
                    "hot": {"focus": "memory lane", "next_action": "verify recall gate", "active_task_id": "T001", "blockers": []},
                    "warm": {
                        "what_changed": "seeded",
                        "open_loops": ["verify recall gate evidence", "do not forget scoped dirty repo"],
                        "commitments": ["report validation evidence to user"],
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

        self.assertEqual(
            set(out.keys()),
            {"session", "recovery", "inbox_preview", "context_hygiene", "memory_recall_gate", "next_calls"},
        )
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
        self.assertEqual(recovery["self_state"]["recovery"]["open_loops"], ["verify recall gate evidence", "do not forget scoped dirty repo"])
        self.assertEqual(recovery["self_state"]["recovery"]["commitments"], ["report validation evidence to user"])
        self.assertEqual(recovery["self_state"]["recovery"]["environment_summary"], "repo dirty but scoped")
        self.assertEqual(recovery["self_state"]["recovery"]["user_model"], "prefers concise evidence")
        self.assertEqual(recovery["self_state"]["recovery"]["persona_notes"], "be precise and low-noise")
        self.assertEqual(recovery["self_state"]["mind_context_mini"]["environment_summary"], "repo dirty but scoped")
        self.assertEqual(recovery["task_slice"]["assigned_active"][0]["id"], "T001")
        self.assertEqual(recovery["task_slice"]["assigned_active"][0]["parent_id"], "T000")
        self.assertEqual(recovery["task_slice"]["assigned_active"][0]["task_type"], "optimization")
        self.assertEqual(recovery["recent_notes"]["decisions"][0]["summary"], "Keep lifecycle deterministic.")

        hygiene = out["context_hygiene"]
        self.assertEqual(str(hygiene["execution_health"]["status"] or ""), "stale")
        self.assertEqual(str(hygiene["mind_context_health"]["status"] or ""), "stale")

        inbox_preview = out["inbox_preview"]
        self.assertEqual(inbox_preview["messages"][0]["id"], "ev1")
        self.assertTrue(inbox_preview["messages"][0]["reply_required"] is True)
        self.assertEqual(inbox_preview["messages"][0]["text_preview"], "please verify memory lane")

        gate = out["memory_recall_gate"]
        self.assertTrue(bool(gate.get("required")))
        self.assertEqual(str(gate.get("status") or ""), "ready")
        hits = gate.get("hits") if isinstance(gate.get("hits"), list) else []
        self.assertGreaterEqual(len(hits), 1)

        next_calls = out["next_calls"]
        self.assertEqual(next_calls["inbox_list"], 'cccc_inbox_list(kind_filter="all")')
        self.assertIn('signal_family="interrupt"', next_calls["interrupt_triage"])
        self.assertIn('resume the current task', next_calls["interrupt_triage"])

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
                        "open_loops": ["re-check shared assumptions"],
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

    def test_build_bootstrap_context_preserves_mind_context_mini_under_budget_pressure(self) -> None:
        from cccc.ports.mcp.handlers.cccc_core import _build_bootstrap_context

        huge_text = "very long project detail " * 160
        noisy_note = "task note " * 120
        context_payload = {
            "coordination": {
                "brief": {
                    "objective": "Ship continuity",
                    "current_focus": "Bootstrap resilience",
                    "constraints": ["keep continuity under pressure"] * 4,
                    "project_brief": huge_text,
                    "project_brief_stale": False,
                },
                "tasks": [
                    {
                        "id": f"T{i:03d}",
                        "title": f"Task {i}",
                        "outcome": noisy_note,
                        "parent_id": "T000" if i == 1 else None,
                        "status": "active" if i == 1 else "planned",
                        "assignee": "peer1",
                        "task_type": "optimization" if i == 1 else "standard",
                        "notes": noisy_note,
                        "checklist": [
                            {"id": "C001", "text": noisy_note, "status": "pending"},
                            {"id": "C002", "text": noisy_note, "status": "pending"},
                        ],
                    }
                    for i in range(1, 8)
                ],
                "recent_decisions": [{"summary": noisy_note, "by": "foreman", "at": "2026-03-07T00:00:00Z"}] * 3,
                "recent_handoffs": [{"summary": noisy_note, "by": "peer1", "at": "2026-03-07T00:00:00Z"}] * 3,
            },
            "agent_states": [
                {
                    "id": "peer1",
                    "hot": {
                        "focus": "stabilize recovery",
                        "next_action": "trim bootstrap safely",
                        "active_task_id": "T001",
                        "blockers": [],
                    },
                    "warm": {
                        "what_changed": "picked up recovery hardening",
                        "open_loops": ["preserve recovery fields under budget"],
                        "commitments": ["keep bootstrap packet slim"],
                        "environment_summary": "workspace has many parallel changes but current scope is limited",
                        "user_model": "prefers simple mechanisms with high ROI",
                        "persona_notes": "preserve continuity and do not overbuild",
                    },
                    "updated_at": "2026-03-12T00:00:00Z",
                }
            ],
        }

        pack = _build_bootstrap_context(context=context_payload, actor_id="peer1")
        agent_state = pack.get("agent_state") if isinstance(pack.get("agent_state"), dict) else {}
        mini = agent_state.get("mind_context_mini") if isinstance(agent_state.get("mind_context_mini"), dict) else {}
        tasks = pack.get("tasks") if isinstance(pack.get("tasks"), dict) else {}
        assigned_active = tasks.get("assigned_active") if isinstance(tasks.get("assigned_active"), list) else []

        self.assertTrue(mini)
        self.assertIn("workspace has many parallel changes", str(mini.get("environment_summary") or ""))
        self.assertIn("prefers simple mechanisms", str(mini.get("user_model") or ""))
        self.assertIn("preserve continuity", str(mini.get("persona_notes") or ""))
        warm = agent_state.get("warm") if isinstance(agent_state.get("warm"), dict) else {}
        self.assertEqual(warm.get("open_loops"), ["preserve recovery fields under budget"])
        self.assertEqual(warm.get("commitments"), ["keep bootstrap packet slim"])
        self.assertTrue(assigned_active)
        self.assertEqual(assigned_active[0].get("task_type"), "optimization")
        self.assertEqual(assigned_active[0].get("parent_id"), "T000")


if __name__ == "__main__":
    unittest.main()
