from __future__ import annotations

import os
import tempfile
import unittest


class TestSystemPromptGroupSpace(unittest.TestCase):
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

    def test_prompt_excludes_group_space_block_when_unbound(self) -> None:
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group
        from cccc.kernel.system_prompt import render_system_prompt

        _, cleanup = self._with_home()
        try:
            gid, aid = self._create_group_with_actor(title="prompt-space-unbound")
            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            actor = find_actor(group, aid)
            self.assertIsNotNone(actor)
            prompt = render_system_prompt(group=group, actor=actor or {})
            self.assertNotIn("Group Space:", prompt)
            self.assertNotIn("cccc_space(action=query)", prompt)
        finally:
            cleanup()

    def test_prompt_includes_group_space_block_when_bound(self) -> None:
        from cccc.daemon.space.group_space_store import set_space_provider_state, upsert_space_binding
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group
        from cccc.kernel.system_prompt import render_system_prompt

        _, cleanup = self._with_home()
        try:
            gid, aid = self._create_group_with_actor(title="prompt-space-bound")
            upsert_space_binding(
                gid,
                provider="notebooklm",
                remote_space_id="nb_prompt_1",
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
            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            actor = find_actor(group, aid)
            self.assertIsNotNone(actor)
            prompt = render_system_prompt(group=group, actor=actor or {})
            self.assertIn("Group Space:", prompt)
            self.assertIn("cccc_space(action=query)", prompt)
            self.assertIn("cccc_space(action=ingest)", prompt)
            self.assertIn("cccc_space(action=artifact)", prompt)
            self.assertIn("source_type", prompt)
            self.assertIn("*.conflict.remote.*", prompt)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
