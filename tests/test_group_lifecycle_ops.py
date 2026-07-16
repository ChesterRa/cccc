import os
import tempfile
import unittest
from contextlib import contextmanager
from pathlib import Path
from unittest.mock import patch


class TestGroupLifecycleOps(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            try:
                from cccc.daemon.claude_app_sessions import SUPERVISOR as claude_app_supervisor
                from cccc.daemon.codex_app_sessions import SUPERVISOR as codex_app_supervisor
                from cccc.runners import headless as headless_runner
                from cccc.runners import pty as pty_runner

                codex_app_supervisor.stop_all()
                claude_app_supervisor.stop_all()
                headless_runner.SUPERVISOR.stop_all()
                pty_runner.SUPERVISOR.stop_all()
            except Exception:
                pass
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

    @contextmanager
    def _fake_codex_headless_start(self):
        def _fake_start_actor(*, group_id: str, actor_id: str, cwd: Path, env: dict[str, str], model: str = ""):
            class _Session:
                def __init__(self) -> None:
                    self.group_id = group_id
                    self.actor_id = actor_id
                    self.cwd = cwd
                    self.env = dict(env)
                    self.model = model

            return _Session()

        with patch(
            "cccc.daemon.group.group_lifecycle_ops.codex_app_supervisor.start_actor",
            side_effect=_fake_start_actor,
        ):
            yield

    def _add_actor(self, group_id: str, *, actor_id: str = "peer1", enabled: bool | None = None):
        add, _ = self._call(
            "actor_add",
            {
                "group_id": group_id,
                "actor_id": actor_id,
                "title": actor_id,
                "runtime": "codex",
                "runner": "headless",
                "by": "user",
            },
        )
        self.assertTrue(add.ok, getattr(add, "error", None))
        if enabled is None:
            return
        from cccc.kernel.group import load_group

        group = load_group(group_id)
        assert group is not None
        actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
        for actor in actors:
            if isinstance(actor, dict) and str(actor.get("id") or "").strip() == actor_id:
                actor["enabled"] = bool(enabled)
        group.save()

    def _append_legacy_internal_actor(self, group_id: str, *, actor_id: str = "pet-peer", enabled: bool = False) -> None:
        from cccc.kernel.group import load_group

        group = load_group(group_id)
        assert group is not None
        actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
        actors.append(
            {
                "id": actor_id,
                "title": "Legacy PET",
                "runtime": "codex",
                "runner": "headless",
                "command": [],
                "env": {},
                "enabled": bool(enabled),
                "internal_kind": "pet",
            }
        )
        group.save()

    def test_group_start_requires_active_scope(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(getattr(start.error, "code", ""), "missing_project_root")
        finally:
            cleanup()

    def test_group_start_does_not_resume_paused_group(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start-paused", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            self._add_actor(group_id)

            set_state, _ = self._call("group_set_state", {"group_id": group_id, "state": "paused", "by": "user"})
            self.assertTrue(set_state.ok, getattr(set_state, "error", None))

            with self._fake_codex_headless_start():
                start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))

            show, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show.ok, getattr(show, "error", None))
            group_doc = (show.result or {}).get("group") if isinstance(show.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertEqual(str(group_doc.get("state") or ""), "paused")
            self.assertTrue(bool(group_doc.get("running")))
        finally:
            cleanup()

    def test_group_start_after_stop_clears_stale_paused_state(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-stop-start", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            self._add_actor(group_id)

            paused, _ = self._call("group_set_state", {"group_id": group_id, "state": "paused", "by": "user"})
            self.assertTrue(paused.ok, getattr(paused, "error", None))

            stop, _ = self._call("group_stop", {"group_id": group_id, "by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))

            stopped_show, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(stopped_show.ok, getattr(stopped_show, "error", None))
            stopped_doc = (stopped_show.result or {}).get("group") if isinstance(stopped_show.result, dict) else {}
            self.assertIsInstance(stopped_doc, dict)
            assert isinstance(stopped_doc, dict)
            self.assertEqual(str(stopped_doc.get("state") or ""), "stopped")
            self.assertFalse(bool(stopped_doc.get("running")))

            with self._fake_codex_headless_start():
                start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))

            show, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show.ok, getattr(show, "error", None))
            group_doc = (show.result or {}).get("group") if isinstance(show.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertEqual(str(group_doc.get("state") or ""), "active")
            self.assertTrue(bool(group_doc.get("running")))
        finally:
            cleanup()

    def test_single_actor_start_paths_reject_unsupported_internal_actor(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "legacy-pet-start", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)
            self._append_legacy_internal_actor(group_id, actor_id="pet-peer", enabled=False)

            start, _ = self._call("actor_start", {"group_id": group_id, "actor_id": "pet-peer", "by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(str(getattr(start.error, "code", "")), "unsupported_internal_actor")

            restart, _ = self._call("actor_restart", {"group_id": group_id, "actor_id": "pet-peer", "by": "user"})
            self.assertFalse(restart.ok)
            self.assertEqual(str(getattr(restart.error, "code", "")), "unsupported_internal_actor")

            update, _ = self._call(
                "actor_update",
                {"group_id": group_id, "actor_id": "pet-peer", "patch": {"enabled": True}, "by": "user"},
            )
            self.assertFalse(update.ok)
            self.assertEqual(str(getattr(update.error, "code", "")), "unsupported_internal_actor")

            group = load_group(group_id)
            assert group is not None
            actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
            legacy = next(
                actor
                for actor in actors
                if isinstance(actor, dict) and str(actor.get("id") or "").strip() == "pet-peer"
            )
            self.assertFalse(bool(legacy.get("enabled")))
        finally:
            cleanup()

    def test_group_start_skips_unsupported_internal_actor(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start-legacy-internal", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            self._add_actor(group_id, actor_id="peer1")

            group = load_group(group_id)
            assert group is not None
            actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
            actors.append(
                {
                    "id": "legacy-internal",
                    "title": "Legacy Internal",
                    "runtime": "codex",
                    "runner": "headless",
                    "command": [],
                    "env": {},
                    "enabled": True,
                    "internal_kind": "legacy",
                }
            )
            group.save()

            captured: list[dict[str, object]] = []

            def _fake_codex_start_actor(*, group_id: str, actor_id: str, cwd: Path, env: dict[str, str], model: str = ""):
                captured.append(
                    {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "cwd": cwd,
                        "env": dict(env),
                        "model": model,
                    }
                )

                class _Session:
                    pass

                return _Session()

            with patch("cccc.daemon.group.group_lifecycle_ops.codex_app_supervisor.start_actor", side_effect=_fake_codex_start_actor):
                start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})

            self.assertTrue(start.ok, getattr(start, "error", None))
            reloaded = load_group(group_id)
            assert reloaded is not None
            actor_ids = {
                str(actor.get("id") or "").strip()
                for actor in (reloaded.doc.get("actors") if isinstance(reloaded.doc.get("actors"), list) else [])
                if isinstance(actor, dict)
            }
            self.assertIn("legacy-internal", actor_ids)
            launched_ids = {str(item.get("actor_id") or "") for item in captured}
            self.assertIn("peer1", launched_ids)
            self.assertNotIn("legacy-internal", launched_ids)
        finally:
            cleanup()

if __name__ == "__main__":
    unittest.main()
