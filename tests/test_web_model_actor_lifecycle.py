import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestWebModelActorLifecycle(unittest.TestCase):
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

    def _create_attached_group(self, root: Path) -> str:
        create, _ = self._call("group_create", {"title": "web-model-lifecycle", "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        group_id = str((create.result or {}).get("group_id") or "").strip()
        attach, _ = self._call("attach", {"group_id": group_id, "path": str(root), "by": "user"})
        self.assertTrue(attach.ok, getattr(attach, "error", None))
        return group_id

    def test_web_model_actor_add_start_stop_uses_marker_not_local_process(self) -> None:
        from cccc.daemon.runner_state_ops import headless_state_running

        home, cleanup = self._with_home()
        try:
            root = Path(home) / "repo"
            root.mkdir(parents=True, exist_ok=True)
            group_id = self._create_attached_group(root)

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "webpeer",
                    "title": "Web Peer",
                    "runtime": "web_model",
                    "runner": "pty",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))
            actor = (add.result or {}).get("actor") or {}
            self.assertEqual(actor.get("runtime"), "web_model")
            self.assertEqual(actor.get("runner"), "headless")
            self.assertEqual(actor.get("command"), [])
            self.assertTrue(bool((add.result or {}).get("running")))
            self.assertTrue(headless_state_running(group_id, "webpeer"))

            actors, _ = self._call("actor_list", {"group_id": group_id, "include_unread": False})
            self.assertTrue(actors.ok, getattr(actors, "error", None))
            listed = ((actors.result or {}).get("actors") or [])[0]
            self.assertTrue(bool(listed.get("running")))
            self.assertEqual(listed.get("runner_effective"), "headless")
            self.assertEqual(listed.get("effective_working_state"), "waiting")

            stop, _ = self._call("actor_stop", {"group_id": group_id, "actor_id": "webpeer", "by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))
            self.assertFalse(headless_state_running(group_id, "webpeer"))
        finally:
            cleanup()

    def test_chatgpt_web_model_actor_is_singleton_per_home(self) -> None:
        home, cleanup = self._with_home()
        try:
            root_one = Path(home) / "repo-one"
            root_two = Path(home) / "repo-two"
            root_one.mkdir(parents=True, exist_ok=True)
            root_two.mkdir(parents=True, exist_ok=True)
            group_one = self._create_attached_group(root_one)
            group_two = self._create_attached_group(root_two)

            first, _ = self._call(
                "actor_add",
                {
                    "group_id": group_one,
                    "actor_id": "chatgpt-web",
                    "title": "ChatGPT Web Model",
                    "runtime": "web_model",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))

            second, _ = self._call(
                "actor_add",
                {
                    "group_id": group_two,
                    "actor_id": "chatgpt-web-2",
                    "title": "Second ChatGPT Web Model",
                    "runtime": "web_model",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertFalse(second.ok)
            self.assertEqual(second.error.code, "actor_add_failed")
            self.assertIn("limited to one actor", second.error.message)
            self.assertIn(group_one, second.error.message)
        finally:
            cleanup()

    def test_chatgpt_web_model_singleton_ignores_deleting_quarantine_group(self) -> None:
        home, cleanup = self._with_home()
        try:
            quarantine = Path(home) / "groups" / ".deleting-g_old-abcd1234"
            quarantine.mkdir(parents=True, exist_ok=True)
            (quarantine / "group.yaml").write_text(
                "\n".join(
                    [
                        "v: 1",
                        "group_id: g_old",
                        "title: deleted",
                        "actors:",
                        "  - id: chatgpt-web",
                        "    title: Deleted ChatGPT Web Model",
                        "    runtime: web_model",
                        "    runner: headless",
                    ]
                ),
                encoding="utf-8",
            )
            root = Path(home) / "repo"
            root.mkdir(parents=True, exist_ok=True)
            group_id = self._create_attached_group(root)

            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "chatgpt-web",
                    "title": "ChatGPT Web Model",
                    "runtime": "web_model",
                    "runner": "headless",
                    "by": "user",
                },
            )

            self.assertTrue(added.ok, getattr(added, "error", None))
        finally:
            cleanup()

    def test_chatgpt_web_model_singleton_ignores_internal_actor_runtime_residue(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.kernel.actors import INTERNAL_KIND_VOICE_SECRETARY
            from cccc.kernel.group import load_group

            root = Path(home) / "repo"
            root.mkdir(parents=True, exist_ok=True)
            group_id = self._create_attached_group(root)
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc.setdefault("actors", []).append(
                {
                    "id": "voice-secretary",
                    "title": "Voice Secretary",
                    "runtime": "web_model",
                    "runner": "headless",
                    "internal_kind": INTERNAL_KIND_VOICE_SECRETARY,
                    "enabled": True,
                }
            )
            group.save()

            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "chatgpt-web",
                    "title": "ChatGPT Web Model",
                    "runtime": "web_model",
                    "runner": "headless",
                    "by": "user",
                },
            )

            self.assertTrue(added.ok, getattr(added, "error", None))
        finally:
            cleanup()

    def test_internal_actor_cannot_use_chatgpt_web_model_runtime_or_profile(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.kernel.actors import INTERNAL_KIND_VOICE_SECRETARY, add_actor
            from cccc.kernel.group import load_group

            root = Path(home) / "repo"
            root.mkdir(parents=True, exist_ok=True)
            group_id = self._create_attached_group(root)
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(
                group,
                actor_id="voice-secretary",
                title="Voice Secretary",
                runtime="codex",
                runner="headless",
                internal_kind=INTERNAL_KIND_VOICE_SECRETARY,
            )

            direct, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "voice-secretary",
                    "patch": {"runtime": "web_model", "runner": "headless"},
                    "by": "user",
                },
            )
            self.assertFalse(direct.ok)
            self.assertIn("standard actors", direct.error.message)

            profile, _ = self._call(
                "actor_profile_upsert",
                {
                    "by": "user",
                    "profile": {
                        "id": "web-profile",
                        "name": "ChatGPT Web",
                        "runtime": "web_model",
                        "runner": "headless",
                    },
                },
            )
            self.assertTrue(profile.ok, getattr(profile, "error", None))

            apply_profile, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "voice-secretary",
                    "profile_id": "web-profile",
                    "by": "user",
                },
            )
            self.assertFalse(apply_profile.ok)
            self.assertIn("standard actors", apply_profile.error.message)
        finally:
            cleanup()

    def test_voice_secretary_seed_does_not_inherit_web_model_runtime(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.voice_secretary_actor import build_voice_secretary_actor_seed

            root = Path(home) / "repo"
            root.mkdir(parents=True, exist_ok=True)
            group_id = self._create_attached_group(root)
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            seed = build_voice_secretary_actor_seed(
                group,
                runtime="web_model",
                runner="headless",
                command=[],
                env={},
                default_scope_key="",
                submit="enter",
            )

            self.assertEqual(seed.get("runtime"), "codex")
            self.assertEqual(seed.get("runner"), "headless")
            self.assertTrue(seed.get("command"))
        finally:
            cleanup()

    def test_actor_update_to_chatgpt_web_model_is_singleton_guarded(self) -> None:
        home, cleanup = self._with_home()
        try:
            root_one = Path(home) / "repo-one"
            root_two = Path(home) / "repo-two"
            root_one.mkdir(parents=True, exist_ok=True)
            root_two.mkdir(parents=True, exist_ok=True)
            group_one = self._create_attached_group(root_one)
            group_two = self._create_attached_group(root_two)

            first, _ = self._call(
                "actor_add",
                {
                    "group_id": group_one,
                    "actor_id": "chatgpt-web",
                    "title": "ChatGPT Web Model",
                    "runtime": "web_model",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))

            codex_actor, _ = self._call(
                "actor_add",
                {
                    "group_id": group_two,
                    "actor_id": "peer1",
                    "title": "Peer",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(codex_actor.ok, getattr(codex_actor, "error", None))

            update, _ = self._call(
                "actor_update",
                {
                    "group_id": group_two,
                    "actor_id": "peer1",
                    "patch": {"runtime": "web_model", "runner": "headless"},
                    "by": "user",
                },
            )
            self.assertFalse(update.ok)
            self.assertEqual(update.error.code, "actor_update_failed")
            self.assertIn("limited to one actor", update.error.message)
            self.assertIn(group_one, update.error.message)
        finally:
            cleanup()

    def test_group_start_for_web_model_does_not_spawn_headless_supervisor(self) -> None:
        from cccc.daemon.runner_state_ops import headless_state_running

        home, cleanup = self._with_home()
        try:
            root = Path(home) / "repo"
            root.mkdir(parents=True, exist_ok=True)
            group_id = self._create_attached_group(root)
            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "webpeer",
                    "title": "Web Peer",
                    "runtime": "web_model",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))
            stop, _ = self._call("actor_stop", {"group_id": group_id, "actor_id": "webpeer", "by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))

            with patch(
                "cccc.daemon.group.group_lifecycle_ops.headless_runner.SUPERVISOR.start_actor",
                side_effect=AssertionError("web_model must not spawn generic headless supervisor"),
            ), patch(
                "cccc.daemon.group.group_lifecycle_ops.pty_runner.SUPERVISOR.start_actor",
                side_effect=AssertionError("web_model must not spawn PTY supervisor"),
            ):
                started, _ = self._call("group_start", {"group_id": group_id, "by": "user"})

            self.assertTrue(started.ok, getattr(started, "error", None))
            self.assertEqual((started.result or {}).get("started"), ["webpeer"])
            self.assertTrue(headless_state_running(group_id, "webpeer"))
        finally:
            cleanup()

    def test_runner_group_running_includes_web_model_marker(self) -> None:
        from cccc.daemon.actors.runner_ops import is_group_running
        from cccc.daemon.runner_state_ops import write_headless_state

        home, cleanup = self._with_home()
        try:
            root = Path(home) / "repo"
            root.mkdir(parents=True, exist_ok=True)
            group_id = self._create_attached_group(root)
            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "webpeer",
                    "title": "Web Peer",
                    "runtime": "web_model",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))
            write_headless_state(group_id, "webpeer")

            self.assertTrue(is_group_running(group_id))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
