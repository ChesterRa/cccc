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

            self.assertIn("CCCC Protocol:", prompt)
            self.assertNotIn("Working Style:", prompt)
            self.assertNotIn("Platform Invariants:", prompt)
            self.assertIn("Use cccc_message_reply for replies; use cccc_message_send for new messages.", prompt)
            self.assertNotIn("Visible replies must go through MCP: cccc_message_send / cccc_message_reply.", prompt)
            self.assertNotIn("your final answer streams to Chat automatically", prompt)
            self.assertIn("On cold start or resume, use MCP tool `cccc_bootstrap`.", prompt)
            self.assertIn("Call `cccc_help` only when you need a CCCC-specific route or a missing capability.", prompt)
            self.assertNotIn("A status message, plan, or promise is not task progress", prompt)
            self.assertNotIn("sync shared control-plane state", prompt)
            self.assertNotIn("finish it end-to-end", prompt)
            self.assertNotIn("implement only after explicit action intent", prompt)
            self.assertNotIn("Execution default:", prompt)
            self.assertEqual(prompt.count("cold start or resume"), 1)

            self.assertNotIn("Memory:", prompt)
            self.assertNotIn("state/memory/MEMORY.md + state/memory/daily/*.md", prompt)
            self.assertNotIn("cccc_memory(action=search)", prompt)
            self.assertNotIn("Planning gate (6D)", prompt)
            self.assertNotIn("Todo discipline:", prompt)
            self.assertNotIn("Gap policy:", prompt)
        finally:
            cleanup()

    def test_team_seed_is_one_small_non_solo_activation_not_full_insight_doctrine(self) -> None:
        from cccc.kernel.actors import add_actor, find_actor
        from cccc.kernel.group import load_group
        from cccc.kernel.peer_insight import SUPERVISOR_MAGIC_KERNEL, TEAM_MODE_SEED
        from cccc.kernel.system_prompt import render_system_prompt

        _, cleanup = self._with_home()
        try:
            gid, aid = self._create_group_with_actor(title="team-seed")
            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            actor = find_actor(group, aid)
            self.assertIsNotNone(actor)
            solo_prompt = render_system_prompt(group=group, actor=actor or {})
            self.assertNotIn(TEAM_MODE_SEED, solo_prompt)

            add_actor(group, actor_id="agent2", runner="headless", runtime="codex")
            actor = find_actor(group, aid)
            team_prompt = render_system_prompt(group=group, actor=actor or {})
            self.assertEqual(team_prompt.count(TEAM_MODE_SEED), 1)
            self.assertNotIn(SUPERVISOR_MAGIC_KERNEL, team_prompt)
            self.assertNotIn("Peer Insight Contract", team_prompt)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
