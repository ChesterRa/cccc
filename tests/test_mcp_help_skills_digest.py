from __future__ import annotations

import os
import unittest
from unittest.mock import patch


class TestMcpHelpSkillsDigest(unittest.TestCase):
    def test_cccc_help_appends_runtime_skill_digest(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False), patch(
            "cccc.ports.mcp.handlers.cccc_core.load_group",
            return_value=None,
        ), patch(
            "cccc.ports.mcp.handlers.cccc_core._call_daemon_or_raise",
            return_value={
                "active_skills": [
                    {
                        "capability_id": "skill:anthropic:triage",
                        "name": "triage",
                        "description_short": "Issue triage checklist",
                    }
                ],
                "autoload_skills": [
                    {
                        "capability_id": "skill:anthropic:review",
                        "name": "review",
                        "description_short": "Code review baseline",
                    }
                ],
            },
        ):
            out = handle_tool_call("cccc_help", {})

        markdown = str(out.get("markdown") or "")
        self.assertIn("## Active Skills (Runtime)", markdown)
        self.assertIn("## Capability Quick Use (Runtime)", markdown)
        self.assertIn("## Gap routing (high ROI)", markdown)
        self.assertIn("cccc_capability_search(kind=\"mcp_toolpack\")", markdown)
        self.assertIn("cccc_capability_use", markdown)
        self.assertIn("capability-skill is runtime capsule activation", markdown)
        self.assertIn("$CODEX_HOME/skills", markdown)
        self.assertIn("### Todo (runtime-first)", markdown)
        self.assertIn("Every concrete user ask/question (even simple) = one runtime todo item", markdown)
        self.assertIn("Capture implicit asks too", markdown)
        self.assertIn("If new evidence overturns prior assumptions, refactor todo immediately", markdown)
        self.assertIn("Anti-drip delivery: once implementation is approved, finish the agreed scope in one pass", markdown)
        self.assertIn('In-scope polish rule: include obvious low-risk in-scope polish in the same pass', markdown)
        self.assertIn("Scope boundary: do not use polish to expand scope; ask first if a change is beyond agreed scope", markdown)
        self.assertIn("For status replies, map current approved scope items explicitly to `done` / `pending` / `blocked(owner)`", markdown)
        self.assertIn("### Intent & scope alignment", markdown)
        self.assertIn("do not implement until explicit action intent", markdown)
        self.assertIn("### Planning balance (6D)", markdown)
        self.assertIn("value/ROI, complexity load, feasibility, verifiability, risk/side-effects, reversibility", markdown)
        self.assertIn("triage", markdown)
        self.assertIn("review", markdown)

    def test_cccc_help_includes_context_hygiene(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False), patch(
            "cccc.ports.mcp.handlers.cccc_core.load_group",
            return_value=None,
        ), patch(
            "cccc.ports.mcp.handlers.cccc_core._call_daemon_or_raise",
            return_value={},
        ), patch(
            "cccc.ports.mcp.server._call_daemon_or_raise",
            return_value={
                "agents": [
                    {
                        "id": "peer-1",
                        "focus": "test focus",
                        "next_action": "do next",
                        "what_changed": "updated",
                    }
                ]
            },
        ):
            out = handle_tool_call("cccc_help", {})

        hygiene = out.get("context_hygiene") if isinstance(out, dict) else None
        self.assertIsInstance(hygiene, dict)
        assert isinstance(hygiene, dict)
        self.assertEqual(str(hygiene.get("actor_id") or ""), "peer-1")
        self.assertEqual(bool(hygiene.get("present")), True)
        self.assertEqual(bool(hygiene.get("min_fields_ready")), True)


if __name__ == "__main__":
    unittest.main()
