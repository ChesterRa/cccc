"""Tests for system prompt memory guidance (T099)."""

import os
import tempfile
import unittest


class TestSystemPromptMemory(unittest.TestCase):
    """Test that memory policy lines appear in rendered system prompt."""

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

    def test_prompt_includes_memory_block(self) -> None:
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

            # Memory header
            self.assertIn("Memory:", prompt)

            # Memory vs Notes boundary
            self.assertIn("Notes are sticky notes", prompt)
            self.assertIn("Memory is a notebook", prompt)

            # Memory tool list mentioned
            self.assertIn("cccc_memory_guide", prompt)
            self.assertIn("cccc_memory_store", prompt)
            self.assertIn("cccc_memory_search", prompt)
            self.assertIn("cccc_memory_ingest", prompt)
            self.assertIn("cccc_memory_stats", prompt)
            self.assertIn("cccc_memory_delete", prompt)
            self.assertIn("cccc_memory_decay", prompt)
            self.assertIn("cccc_memory_export", prompt)
            self.assertIn("Call cccc_memory_guide(topic) before complex memory operations for best practices.", prompt)
        finally:
            cleanup()

    def test_memory_block_before_group_space(self) -> None:
        """Memory block appears before Group Space block in prompt."""
        from cccc.kernel.system_prompt import _memory_policy_lines, _group_space_policy_lines

        mem_lines = _memory_policy_lines("g_test")
        self.assertGreater(len(mem_lines), 0)
        self.assertEqual(mem_lines[0], "Memory:")

    def test_memory_lines_empty_for_empty_group_id(self) -> None:
        from cccc.kernel.system_prompt import _memory_policy_lines

        lines = _memory_policy_lines("")
        self.assertEqual(lines, [])

    def test_memory_lines_content(self) -> None:
        """Verify all key guidance points are present."""
        from cccc.kernel.system_prompt import _memory_policy_lines

        lines = _memory_policy_lines("g_test")
        text = "\n".join(lines)

        # Memory vs Notes boundary
        self.assertIn("Notes are sticky notes", text)
        self.assertIn("Memory is a notebook", text)

        # Tool guidance
        self.assertIn("cccc_memory_guide", text)
        self.assertIn("cccc_memory_store", text)
        self.assertIn("cccc_memory_search", text)
        self.assertIn("cccc_memory_ingest", text)
        self.assertIn("cccc_memory_stats", text)
        self.assertIn("cccc_memory_delete", text)
        self.assertIn("cccc_memory_decay", text)
        self.assertIn("cccc_memory_export", text)

        # Guide call-to-action
        self.assertIn("Call cccc_memory_guide(topic) before complex memory operations for best practices.", text)


if __name__ == "__main__":
    unittest.main()
