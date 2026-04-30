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
            self.assertIn("Cold start or resume: run `cccc_bootstrap`, then `cccc_help`.", prompt)
            self.assertIn("At key transitions, sync shared control-plane state and your cccc_agent_state.", prompt)
            self.assertIn("Once scope is approved, finish it end-to-end; do not ask to continue on obvious next steps.", prompt)
            self.assertIn("For strategy or scope discussion, align first; implement only after explicit action intent.", prompt)
            self.assertIn("Execution default:", prompt)
            self.assertIn("Reuse working paths first.", prompt)
            self.assertNotIn("Working stance:", prompt)
            self.assertNotIn("Cold start or resume: call cccc_bootstrap first", prompt)
            self.assertEqual(prompt.count("Prefer silence over low-signal chatter"), 1)
            self.assertEqual(prompt.count("finish it end-to-end"), 1)
            self.assertEqual(prompt.count("Cold start or resume:"), 1)

            self.assertNotIn("Memory:", prompt)
            self.assertNotIn("state/memory/MEMORY.md + state/memory/daily/*.md", prompt)
            self.assertNotIn("cccc_memory(action=search)", prompt)
            self.assertNotIn("Planning gate (6D)", prompt)
            self.assertNotIn("Todo discipline:", prompt)
            self.assertNotIn("Gap policy:", prompt)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
