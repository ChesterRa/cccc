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

    def test_group_start_uses_disabled_foreman_as_pet_config_source(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start-stale-pet", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            self._add_actor(group_id, actor_id="lead", enabled=False)

            group = load_group(group_id)
            assert group is not None
            group.doc["features"] = {"desktop_pet_enabled": True}
            actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
            actors.append(
                {
                    "id": "pet-peer",
                    "title": "Pet Peer",
                    "runtime": "codex",
                    "runner": "headless",
                    "command": [],
                    "env": {},
                    "enabled": True,
                    "internal_kind": "pet",
                }
            )
            group.save()

            with self._fake_codex_headless_start():
                start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))

            reloaded = load_group(group_id)
            assert reloaded is not None
            actor_ids = [
                str(actor.get("id") or "").strip()
                for actor in (reloaded.doc.get("actors") if isinstance(reloaded.doc.get("actors"), list) else [])
                if isinstance(actor, dict)
            ]
            self.assertIn("lead", actor_ids)
            self.assertIn("pet-peer", actor_ids)
        finally:
            cleanup()

    def test_group_start_removes_stale_pet_actor_when_no_foreman_exists(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start-stale-pet-no-foreman", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            group = load_group(group_id)
            assert group is not None
            group.doc["features"] = {"desktop_pet_enabled": True}
            actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
            actors.append(
                {
                    "id": "pet-peer",
                    "title": "Pet Peer",
                    "runtime": "codex",
                    "runner": "headless",
                    "command": [],
                    "env": {},
                    "enabled": True,
                    "internal_kind": "pet",
                }
            )
            group.save()

            with self._fake_codex_headless_start():
                start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))

            reloaded = load_group(group_id)
            assert reloaded is not None
            actor_ids = [
                str(actor.get("id") or "").strip()
                for actor in (reloaded.doc.get("actors") if isinstance(reloaded.doc.get("actors"), list) else [])
                if isinstance(actor, dict)
            ]
            self.assertNotIn("pet-peer", actor_ids)
        finally:
            cleanup()

    def test_group_start_pet_uses_foreman_profile_private_env(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group
        from cccc.kernel.pet_actor import PET_ACTOR_ID, get_pet_actor

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start-pet-profile", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            profile_upsert, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "actor_profile_upsert",
                        "args": {
                            "by": "user",
                            "caller_id": "user-a",
                            "is_admin": False,
                            "profile": {
                                "id": "pet-profile",
                                "name": "Pet Profile",
                                "scope": "user",
                                "owner_id": "user-a",
                                "runtime": "custom",
                                "runner": "headless",
                                "command": [],
                                "submit": "newline",
                            },
                        },
                    }
                )
            )
            self.assertTrue(profile_upsert.ok, getattr(profile_upsert, "error", None))

            secret_update, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "actor_profile_secret_update",
                        "args": {
                            "by": "user",
                            "profile_id": "pet-profile",
                            "profile_scope": "user",
                            "profile_owner": "user-a",
                            "caller_id": "user-a",
                            "is_admin": False,
                            "set": {"API_KEY": "pet-secret"},
                        },
                    }
                )
            )
            self.assertTrue(secret_update.ok, getattr(secret_update, "error", None))

            add_foreman, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "lead",
                    "runtime": "codex",
                    "runner": "headless",
                    "profile_id": "pet-profile",
                    "profile_scope": "user",
                    "profile_owner": "user-a",
                    "caller_id": "user-a",
                    "is_admin": False,
                    "by": "user",
                },
            )
            self.assertTrue(add_foreman.ok, getattr(add_foreman, "error", None))

            group = load_group(group_id)
            assert group is not None
            group.doc["running"] = False
            group.save()

            enable_pet, _ = self._call(
                "group_settings_update",
                {"group_id": group_id, "by": "user", "patch": {"desktop_pet_enabled": True}},
            )
            self.assertTrue(enable_pet.ok, getattr(enable_pet, "error", None))

            captured: list[dict[str, object]] = []

            def _fake_headless_start_actor(*, group_id: str, actor_id: str, cwd: Path, env: dict[str, str]):
                captured.append(
                    {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "cwd": cwd,
                        "env": dict(env),
                    }
                )

                class _Session:
                    pass

                return _Session()

            with patch("cccc.daemon.group.group_lifecycle_ops.headless_runner.SUPERVISOR.start_actor", side_effect=_fake_headless_start_actor):
                start, _ = self._call(
                    "group_start",
                    {"group_id": group_id, "by": "user", "caller_id": "user-a", "is_admin": False},
                )

            self.assertTrue(start.ok, getattr(start, "error", None))
            pet_launches = [item for item in captured if item.get("actor_id") == PET_ACTOR_ID]
            self.assertEqual(len(pet_launches), 1)
            pet_env = pet_launches[0].get("env")
            self.assertIsInstance(pet_env, dict)
            assert isinstance(pet_env, dict)
            self.assertEqual(pet_env.get("API_KEY"), "pet-secret")

            private_keys, _ = self._call(
                "actor_env_private_keys",
                {"group_id": group_id, "actor_id": PET_ACTOR_ID, "by": "user"},
            )
            self.assertTrue(private_keys.ok, getattr(private_keys, "error", None))
            self.assertEqual(set(private_keys.result.get("keys") or []), {"API_KEY"})

            group = load_group(group_id)
            assert group is not None
            pet_actor = get_pet_actor(group)
            self.assertIsNotNone(pet_actor)
            assert pet_actor is not None
            self.assertEqual(str(pet_actor.get("runtime") or ""), "custom")
            self.assertEqual(str(pet_actor.get("submit") or ""), "newline")
        finally:
            cleanup()

    def test_group_start_preserves_existing_pet_runtime_config(self) -> None:
        from cccc.daemon.actors.private_env_ops import load_actor_private_env, update_actor_private_env
        from cccc.kernel.actors import update_actor
        from cccc.kernel.group import load_group
        from cccc.kernel.pet_actor import PET_ACTOR_ID, get_pet_actor

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start-pet-preserve", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            self._add_actor(group_id, actor_id="lead")
            enable_pet, _ = self._call(
                "group_settings_update",
                {"group_id": group_id, "by": "user", "patch": {"desktop_pet_enabled": True}},
            )
            self.assertTrue(enable_pet.ok, getattr(enable_pet, "error", None))

            group = load_group(group_id)
            assert group is not None
            update_actor(
                group,
                PET_ACTOR_ID,
                {
                    "runtime": "codex",
                    "runner": "headless",
                    "command": ["codex", "-m", "pet-manual"],
                    "env": {"PET_PUBLIC": "manual"},
                    "submit": "newline",
                    "avatar_asset_path": "avatars/pet.png",
                },
            )
            update_actor_private_env(group_id, PET_ACTOR_ID, set_vars={"PET_SECRET": "pet-secret"}, unset_keys=[], clear=True)
            update_actor_private_env(group_id, "lead", set_vars={"FOREMAN_SECRET": "foreman-secret"}, unset_keys=[], clear=True)

            captured: list[dict[str, object]] = []

            def _fake_codex_start_actor(*, group_id: str, actor_id: str, cwd: Path, env: dict[str, str], model: str = ""):
                captured.append({"group_id": group_id, "actor_id": actor_id, "cwd": cwd, "env": dict(env), "model": model})

                class _Session:
                    pass

                return _Session()

            with patch("cccc.daemon.group.group_lifecycle_ops.codex_app_supervisor.start_actor", side_effect=_fake_codex_start_actor):
                start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})

            self.assertTrue(start.ok, getattr(start, "error", None))
            group = load_group(group_id)
            assert group is not None
            pet_actor = get_pet_actor(group)
            self.assertIsNotNone(pet_actor)
            assert pet_actor is not None
            self.assertEqual(pet_actor.get("command"), ["codex", "-m", "pet-manual"])
            self.assertEqual(pet_actor.get("env"), {"PET_PUBLIC": "manual"})
            self.assertEqual(str(pet_actor.get("submit") or ""), "newline")
            self.assertEqual(str(pet_actor.get("avatar_asset_path") or ""), "avatars/pet.png")
            self.assertEqual(load_actor_private_env(group_id, PET_ACTOR_ID), {"PET_SECRET": "pet-secret"})
            pet_launches = [item for item in captured if item.get("actor_id") == PET_ACTOR_ID]
            self.assertEqual(len(pet_launches), 1)
            pet_env = pet_launches[0].get("env")
            self.assertIsInstance(pet_env, dict)
            assert isinstance(pet_env, dict)
            self.assertEqual(pet_env.get("PET_SECRET"), "pet-secret")
            self.assertNotIn("FOREMAN_SECRET", pet_env)
        finally:
            cleanup()

    def test_group_start_pet_falls_back_when_foreman_is_web_model(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.pet_actor import PET_ACTOR_ID, get_pet_actor

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start-web-model-pet", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add_foreman, _ = self._call(
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
            self.assertTrue(add_foreman.ok, getattr(add_foreman, "error", None))

            group = load_group(group_id)
            assert group is not None
            group.doc["features"] = {"desktop_pet_enabled": True}
            group.doc["running"] = False
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
            pet_actor = get_pet_actor(reloaded)
            self.assertIsNotNone(pet_actor)
            assert pet_actor is not None
            self.assertEqual(str(pet_actor.get("runtime") or ""), "codex")
            self.assertEqual(str(pet_actor.get("runner") or ""), "headless")
            self.assertTrue(pet_actor.get("command"))
            pet_launches = [item for item in captured if item.get("actor_id") == PET_ACTOR_ID]
            self.assertEqual(len(pet_launches), 1)
        finally:
            cleanup()

if __name__ == "__main__":
    unittest.main()
