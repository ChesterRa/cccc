"""Tests for the slimmed system prompt surface."""

import os
import tempfile
import unittest


class TestSystemPromptMemory(unittest.TestCase):
    """System prompt should stay lean and route rich guidance elsewhere."""

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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group_with_actor(self, *, title: str) -> tuple[str, str]:
        create, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        gid = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        add, _ = self._call(
            "actor_add",
            {
                "group_id": gid,
                "actor_id": "agent1",
                "runtime": "codex",
                "runner": "headless",
                "by": "user",
            },
        )
        self.assertTrue(add.ok, getattr(add, "error", None))
        return gid, "agent1"

    def test_prompt_routes_to_bootstrap_and_help(self) -> None:
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group
        from cccc.kernel.system_prompt import render_system_prompt

        _, cleanup = self._with_home()
        try:
            gid, aid = self._create_group_with_actor(title="prompt-memory")
            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            actor = find_actor(group, aid)
            self.assertIsNotNone(actor)
            prompt = render_system_prompt(group=group, actor=actor or {})

            self.assertIn("Working Style:", prompt)
            self.assertIn("Platform Invariants:", prompt)
            self.assertIn("Work like a sharp teammate, not a customer-service script.", prompt)
            self.assertIn("Prefer silence over low-signal chatter; speak for real changes, not filler or routine @all updates.", prompt)
            self.assertIn("No fabrication. Verify before claiming done.", prompt)
            self.assertIn("Visible replies must go through MCP: cccc_message_send / cccc_message_reply.", prompt)
            self.assertNotIn("your final answer streams to Chat automatically", prompt)
            self.assertIn("A status message, plan, or promise is not task progress", prompt)
            self.assertIn("Cold start or resume: call cccc_bootstrap first, then cccc_help.", prompt)
            self.assertIn("At key transitions, sync shared control-plane state and your cccc_agent_state.", prompt)
            self.assertIn("Once scope is approved, finish it end-to-end; do not ask to continue on obvious next steps.", prompt)
            self.assertIn("For strategy or scope discussion, align first; implement only after explicit action intent.", prompt)

            self.assertNotIn("Memory:", prompt)
            self.assertNotIn("state/memory/MEMORY.md + state/memory/daily/*.md", prompt)
            self.assertNotIn("cccc_memory(action=search)", prompt)
            self.assertNotIn("Planning gate (6D)", prompt)
            self.assertNotIn("Todo discipline:", prompt)
            self.assertNotIn("Gap policy:", prompt)
            self.assertNotIn("Active Experience Assets:", prompt)
        finally:
            cleanup()

    def test_prompt_injects_experience_asset_cards(self) -> None:
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group
        from cccc.kernel.system_prompt import render_system_prompt

        _, cleanup = self._with_home()
        try:
            gid, aid = self._create_group_with_actor(title="prompt-memory-assets")
            self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "task.create", "title": "Phase 1", "outcome": "stabilize memory lane"}],
                },
            )
            task_list, _ = self._call("task_list", {"group_id": gid})
            tasks = (task_list.result or {}).get("tasks") if isinstance(task_list.result, dict) else []
            task_id = str((tasks[0] if tasks else {}).get("id") or "")
            self.assertTrue(task_id)
            self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "task.move", "task_id": task_id, "status": "done"}],
                },
            )

            from cccc.kernel.group import load_group as reload_group
            from cccc.util.fs import read_json

            group = reload_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            candidates_path = group.path / "state" / "experience_candidates.json"
            candidate_doc = read_json(candidates_path)
            candidates = candidate_doc.get("candidates") if isinstance(candidate_doc.get("candidates"), list) else []
            candidate_id = str((candidates[0] if candidates else {}).get("id") or "")
            self.assertTrue(candidate_id)

            promote, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": gid, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote.ok, getattr(promote, "error", None))

            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            actor = find_actor(group, aid)
            self.assertIsNotNone(actor)
            prompt = render_system_prompt(group=group, actor=actor or {})

            self.assertIn("Active Procedural Skills:", prompt)
            self.assertIn(f"procskill_{candidate_id}", prompt)
            self.assertIn("Active Experience Assets:", prompt)
            self.assertIn(candidate_id, prompt)
            self.assertIn("stabilize memory lane", prompt)
            self.assertNotIn("Promote this outcome", prompt)
        finally:
            cleanup()

    def test_prompt_consumption_policy_normalizes_and_caps_actor_assets(self) -> None:
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group
        from cccc.kernel.system_prompt import render_system_prompt

        _, cleanup = self._with_home()
        try:
            gid, aid = self._create_group_with_actor(title="prompt-memory-policy")
            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            actor = find_actor(group, aid)
            self.assertIsNotNone(actor)
            actor = dict(actor or {})
            actor["experience_assets"] = [
                {"candidate_id": "exp_old", "title": "Old asset", "summary": "older", "recommended_action": "ignore", "promoted_at": "2026-03-01T00:00:00Z"},
                {"candidate_id": "exp_new", "title": "Newest asset", "summary": "newest", "recommended_action": "use newest", "promoted_at": "2026-03-04T00:00:00Z"},
                {"candidate_id": "exp_mid", "title": "Middle asset", "summary": "middle", "recommended_action": "use middle", "promoted_at": "2026-03-03T00:00:00Z"},
                {"candidate_id": "", "title": "Broken asset", "summary": "invalid", "recommended_action": "skip", "promoted_at": "2026-03-05T00:00:00Z"},
                {"candidate_id": "exp_low", "title": "Low asset", "summary": "low", "recommended_action": "use low", "promoted_at": "2026-03-02T00:00:00Z"},
            ]

            prompt = render_system_prompt(group=group, actor=actor)

            self.assertIn("exp_new", prompt)
            self.assertIn("exp_mid", prompt)
            self.assertIn("exp_low", prompt)
            self.assertNotIn("exp_old", prompt)
            self.assertNotIn("Broken asset", prompt)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
