from __future__ import annotations

import json
import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from types import SimpleNamespace
from unittest.mock import patch


class TestMcpHelpSkillsDigest(unittest.TestCase):
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

        return td, cleanup

    def test_cccc_help_appends_runtime_skill_digest(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False), patch(
            "cccc.ports.mcp.handlers.cccc_core.load_group",
            return_value=None,
        ), patch(
            "cccc.ports.mcp.handlers.cccc_core._call_daemon_or_raise",
            return_value={
                "active_capsule_skills": [
                    {
                        "capability_id": "skill:anthropic:triage",
                        "name": "triage",
                        "description_short": "Issue triage checklist",
                        "capsule_preview": "Restate the symptom first.\nGather evidence before changing anything.",
                        "activation_sources": [{"scope": "actor", "actor_id": "peer-1"}],
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
        self.assertIn("## Working Stance", markdown)
        self.assertIn("## Communication Patterns", markdown)
        self.assertIn("## Core Routes", markdown)
        self.assertIn("## Control Plane", markdown)
        self.assertIn("## Memory and Recall", markdown)
        self.assertIn("## Capability", markdown)
        self.assertIn("## Role Notes", markdown)
        self.assertIn("## Active Skills (Runtime)", markdown)
        self.assertIn("Capsule skill is runtime capsule activation", markdown)
        self.assertIn("Codex's skills directory", markdown)
        self.assertIn("if `CODEX_HOME` is explicitly set", markdown)
        self.assertIn("### Todo and Scope Discipline", markdown)
        self.assertIn("Every concrete or implicit user ask becomes a runtime todo item.", markdown)
        self.assertIn("Once implementation is approved, finish the agreed scope in one pass unless a real blocker stops progress.", markdown)
        self.assertIn("### Planning and Scope Gates", markdown)
        self.assertIn("For non-trivial plans, run a 6D check", markdown)
        self.assertIn("triage", markdown)
        self.assertIn("[scope: actor]", markdown)
        self.assertIn("review", markdown)
        self.assertIn("working_rules:", markdown)
        self.assertIn("Restate the symptom first.", markdown)
        self.assertIn("Gather evidence before changing anything.", markdown)
        self.assertNotIn("### NotebookLM Artifact Runs", markdown)

    def test_cccc_help_appends_group_space_runtime_only_when_bound(self) -> None:
        from cccc.ports.mcp.server import handle_tool_call

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False), patch(
            "cccc.ports.mcp.handlers.cccc_core.load_group",
            return_value=None,
        ), patch(
            "cccc.ports.mcp.handlers.cccc_core._call_daemon_or_raise",
            return_value={"enabled_capabilities": []},
        ), patch(
            "cccc.ports.mcp.handlers.cccc_core.get_group_space_prompt_state",
            return_value={
                "provider": "notebooklm",
                "mode": "active",
                "work_bound": True,
                "memory_bound": True,
            },
        ):
            out = handle_tool_call("cccc_help", {})

        markdown = str(out.get("markdown") or "")
        self.assertIn("## Group Space (Runtime)", markdown)
        self.assertIn("If `cccc_space` is hidden in this session", markdown)
        self.assertIn('Use `cccc_space(action="query", lane="work")` for shared/project knowledge lookup.', markdown)
        self.assertIn("do not poll. Wait for the later `system.notify`", markdown)
        self.assertIn("continue other work or standby", markdown)
        self.assertIn("one-shot reminder", markdown)
        self.assertIn('use `cccc_space(action="query", lane="memory")` only as a deeper recall fallback.', markdown)

    def test_cccc_help_group_space_runtime_tracks_bind_unbind_rebind(self) -> None:
        from cccc.daemon.space.group_space_store import set_space_binding_unbound, set_space_provider_state, upsert_space_binding
        from cccc.ports.mcp.server import handle_tool_call

        _, cleanup = self._with_home()
        try:
            with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False), patch(
                "cccc.ports.mcp.handlers.cccc_core._call_daemon_or_raise",
                return_value={"enabled_capabilities": []},
            ):
                initial = handle_tool_call("cccc_help", {})
                initial_markdown = str(initial.get("markdown") or "")
                self.assertNotIn("## Group Space (Runtime)", initial_markdown)

                upsert_space_binding(
                    "g1",
                    provider="notebooklm",
                    lane="work",
                    remote_space_id="nb_work_1",
                    by="user",
                    status="bound",
                )
                set_space_provider_state(
                    "notebooklm",
                    enabled=True,
                    mode="active",
                    last_error="",
                    touch_health=True,
                )
                bound = handle_tool_call("cccc_help", {})
                bound_markdown = str(bound.get("markdown") or "")
                self.assertIn("## Group Space (Runtime)", bound_markdown)
                self.assertIn("work_bound=true memory_bound=false", bound_markdown)

                # Even if provider state remains active, removing the bound lane must remove the runtime addendum.
                set_space_binding_unbound("g1", provider="notebooklm", lane="work", by="user")
                unbound = handle_tool_call("cccc_help", {})
                unbound_markdown = str(unbound.get("markdown") or "")
                self.assertNotIn("## Group Space (Runtime)", unbound_markdown)
                self.assertNotIn("work_bound=true", unbound_markdown)

                upsert_space_binding(
                    "g1",
                    provider="notebooklm",
                    lane="work",
                    remote_space_id="nb_work_2",
                    by="user",
                    status="bound",
                )
                rebound = handle_tool_call("cccc_help", {})
                rebound_markdown = str(rebound.get("markdown") or "")
                self.assertIn("## Group Space (Runtime)", rebound_markdown)
                self.assertIn("work_bound=true memory_bound=false", rebound_markdown)
        finally:
            cleanup()

    def test_cccc_help_group_space_runtime_tracks_partial_lane_unbind(self) -> None:
        from cccc.daemon.space.group_space_store import set_space_binding_unbound, set_space_provider_state, upsert_space_binding
        from cccc.ports.mcp.server import handle_tool_call

        _, cleanup = self._with_home()
        try:
            upsert_space_binding(
                "g1",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_work_1",
                by="user",
                status="bound",
            )
            upsert_space_binding(
                "g1",
                provider="notebooklm",
                lane="memory",
                remote_space_id="nb_memory_1",
                by="user",
                status="bound",
            )
            set_space_provider_state(
                "notebooklm",
                enabled=True,
                mode="active",
                last_error="",
                touch_health=True,
            )

            with patch.dict(os.environ, {"CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer-1"}, clear=False), patch(
                "cccc.ports.mcp.handlers.cccc_core._call_daemon_or_raise",
                return_value={"enabled_capabilities": []},
            ):
                both = handle_tool_call("cccc_help", {})
                both_markdown = str(both.get("markdown") or "")
                self.assertIn("work_bound=true memory_bound=true", both_markdown)
                self.assertIn('`cccc_space(action="query", lane="work")`', both_markdown)
                self.assertIn('`cccc_space(action="query", lane="memory")`', both_markdown)

                set_space_binding_unbound("g1", provider="notebooklm", lane="work", by="user")
                memory_only = handle_tool_call("cccc_help", {})
                memory_only_markdown = str(memory_only.get("markdown") or "")
                self.assertIn("## Group Space (Runtime)", memory_only_markdown)
                self.assertIn("work_bound=false memory_bound=true", memory_only_markdown)
                self.assertNotIn('Use `cccc_space(action="query", lane="work")` for shared/project knowledge lookup.', memory_only_markdown)
                self.assertIn('use `cccc_space(action="query", lane="memory")` only as a deeper recall fallback.', memory_only_markdown)
        finally:
            cleanup()

    def test_runtime_help_addenda_replace_stale_reserved_sections(self) -> None:
        from cccc.ports.mcp.handlers.cccc_core import _append_runtime_help_addenda

        base = (
            "## Core Routes\n"
            "- base content\n\n"
            "## Group Space (Runtime)\n"
            "- stale work_bound=false memory_bound=false\n\n"
            "## Active Skills (Runtime)\n"
            "- stale skill digest\n\n"
            "## Role Notes\n"
            "- keep this\n"
        )

        with patch(
            "cccc.ports.mcp.handlers.cccc_core._call_daemon_or_raise",
            return_value={
                "enabled_capabilities": [],
                "active_capsule_skills": [
                    {
                        "capability_id": "skill:test:triage",
                        "name": "triage",
                        "description_short": "Issue triage checklist",
                        "capsule_preview": "Restate the symptom first.\nGather evidence before changing anything.",
                    }
                ],
                "autoload_skills": [],
            },
        ), patch(
            "cccc.ports.mcp.handlers.cccc_core.get_group_space_prompt_state",
            return_value={
                "provider": "notebooklm",
                "mode": "active",
                "work_bound": True,
                "memory_bound": False,
            },
        ):
            markdown = _append_runtime_help_addenda(base, group_id="g1", actor_id="peer-1")

        self.assertEqual(markdown.count("## Group Space (Runtime)"), 1)
        self.assertEqual(markdown.count("## Active Skills (Runtime)"), 1)
        self.assertIn("work_bound=true memory_bound=false", markdown)
        self.assertIn("triage", markdown)
        self.assertIn("Restate the symptom first.", markdown)
        self.assertNotIn("stale work_bound=false memory_bound=false", markdown)
        self.assertNotIn("stale skill digest", markdown)
        self.assertIn("## Role Notes", markdown)
        self.assertIn("- keep this", markdown)

    def test_cccc_help_appends_web_model_transport_runtime_note(self) -> None:
        from cccc.ports.mcp.handlers.cccc_core import _append_runtime_help_addenda

        group = SimpleNamespace(doc={"actors": [{"id": "web-1", "runtime": "web_model"}]})
        base = (
            "## Core Routes\n"
            "- base content\n\n"
            "## Web Model Transport (Runtime)\n"
            "- stale standalone web-model instructions\n"
        )

        with patch(
            "cccc.ports.mcp.handlers.cccc_core.load_group",
            return_value=group,
        ), patch(
            "cccc.ports.mcp.handlers.cccc_core._call_daemon_or_raise",
            return_value={"enabled_capabilities": []},
        ), patch(
            "cccc.ports.mcp.handlers.cccc_core.get_group_space_prompt_state",
            return_value={},
        ):
            markdown = _append_runtime_help_addenda(base, group_id="g1", actor_id="web-1")

        self.assertEqual(markdown.count("## Web Model Transport (Runtime)"), 1)
        self.assertIn("normal CCCC agent", markdown)
        self.assertIn("same bootstrap/help/message/coordination/capability rules", markdown)
        self.assertIn("do not call `cccc_runtime_wait_next_turn` first", markdown)
        self.assertIn("remote MCP pull", markdown)
        self.assertIn("Web chat text alone is not a visible CCCC reply", markdown)
        self.assertIn("`cccc_shell`", markdown)
        self.assertIn("`cccc_git`", markdown)
        self.assertIn("`cccc_runtime_complete_turn`", markdown)
        self.assertNotIn("stale standalone web-model instructions", markdown)

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
                "agent_states": [
                    {
                        "id": "peer-1",
                        "hot": {
                            "focus": "test focus",
                            "next_action": "do next",
                            "blockers": [],
                        },
                        "warm": {"what_changed": "updated"},
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
        self.assertEqual(str((hygiene.get("execution_health") or {}).get("status") or ""), "stale")
        self.assertEqual(str((hygiene.get("mind_context_health") or {}).get("status") or ""), "missing")

    def test_cccc_help_marks_mind_context_stale_from_runtime_churn(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry
        from cccc.ports.mcp.server import handle_tool_call

        _, cleanup = self._with_home()
        try:
            now = datetime.now(timezone.utc)
            touched_at = (now - timedelta(minutes=5)).isoformat().replace("+00:00", "Z")
            updated_at = now.isoformat().replace("+00:00", "Z")
            group = create_group(load_registry(), title="help-hygiene")
            state_path = group.path / "state" / "automation.json"
            state_path.write_text(
                json.dumps(
                    {
                        "v": 5,
                        "actors": {
                            "peer-1": {
                                "mind_context_touched_at": touched_at,
                                "hot_only_updates_since_mind_touch": 3,
                            }
                        },
                        "rules": {},
                    }
                ),
                encoding="utf-8",
            )

            with patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group.group_id, "CCCC_ACTOR_ID": "peer-1"},
                clear=False,
            ), patch(
                "cccc.ports.mcp.handlers.cccc_core._call_daemon_or_raise",
                return_value={},
            ), patch(
                "cccc.ports.mcp.server._call_daemon_or_raise",
                return_value={
                    "agent_states": [
                        {
                            "id": "peer-1",
                            "hot": {
                                "focus": "verify hygiene",
                                "next_action": "read current status",
                                "blockers": [],
                            },
                            "warm": {
                                "what_changed": "recently updated execution state",
                                "environment_summary": "single bugfix branch in progress",
                                "user_model": "prefers direct evidence",
                                "persona_notes": "do not overbuild the fix",
                            },
                            "updated_at": updated_at,
                        }
                    ]
                },
            ):
                out = handle_tool_call("cccc_help", {})

            hygiene = out.get("context_hygiene") if isinstance(out, dict) else None
            self.assertIsInstance(hygiene, dict)
            assert isinstance(hygiene, dict)
            self.assertEqual(str((hygiene.get("execution_health") or {}).get("status") or ""), "ready")
            self.assertEqual(str((hygiene.get("mind_context_health") or {}).get("status") or ""), "stale")
            self.assertEqual(int((hygiene.get("mind_context_health") or {}).get("hot_only_updates_since_touch") or 0), 3)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
