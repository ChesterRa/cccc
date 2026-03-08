import os
import tempfile
import unittest
from pathlib import Path


class TestActorExtraPrompt(unittest.TestCase):
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

    def test_render_system_prompt_appends_actor_specific_extra_prompt(self) -> None:
        from cccc.kernel.actors import add_actor, find_actor
        from cccc.kernel.group import attach_scope_to_group, create_group, load_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.scope import detect_scope
        from cccc.kernel.system_prompt import render_system_prompt

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="actor-extra-prompt", topic="")
            scope = detect_scope(Path("."))
            attach_scope_to_group(reg, group, scope, set_active=True)
            add_actor(
                group,
                actor_id="lead1",
                title="Foreman",
                runtime="codex",
                runner="headless",
                extra_prompt="Own final integration quality and give the final ship/no-ship call.",
            )
            add_actor(
                group,
                actor_id="peer-review",
                title="Peer Review",
                runtime="codex",
                runner="headless",
                extra_prompt="Review diffs only, focus on risks and missing tests.",
            )

            reloaded = load_group(group.group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None

            foreman = find_actor(reloaded, "lead1")
            reviewer = find_actor(reloaded, "peer-review")
            self.assertIsNotNone(foreman)
            self.assertIsNotNone(reviewer)

            foreman_prompt = render_system_prompt(group=reloaded, actor=foreman or {})
            reviewer_prompt = render_system_prompt(group=reloaded, actor=reviewer or {})

            self.assertIn("Actor-specific instructions:", foreman_prompt)
            self.assertIn("Own final integration quality", foreman_prompt)
            self.assertNotIn("Review diffs only", foreman_prompt)

            self.assertIn("Actor-specific instructions:", reviewer_prompt)
            self.assertIn("Review diffs only", reviewer_prompt)
            self.assertNotIn("Own final integration quality", reviewer_prompt)
        finally:
            cleanup()

    def test_actor_update_persists_extra_prompt(self) -> None:
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-extra-prompt-update", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            update, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "patch": {"extra_prompt": "Keep scope tight and surface blockers early."},
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))
            actor = (update.result or {}).get("actor") if isinstance(update.result, dict) else {}
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(str(actor.get("extra_prompt") or ""), "Keep scope tight and surface blockers early.")

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            stored = find_actor(group, "peer1")
            self.assertIsNotNone(stored)
            self.assertEqual(str((stored or {}).get("extra_prompt") or ""), "Keep scope tight and surface blockers early.")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
