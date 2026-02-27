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
                "pinned_skills": [
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
        self.assertIn("triage", markdown)
        self.assertIn("review", markdown)


if __name__ == "__main__":
    unittest.main()

