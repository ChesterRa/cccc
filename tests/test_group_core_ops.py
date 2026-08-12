import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import errno
import shutil


class _FakePresentationRuntime:
    strategy = "fake_cdp"

    def __init__(self) -> None:
        self.closed = False

    def current_url(self) -> str:
        return "http://127.0.0.1:3000"

    def capture_frame(self) -> bytes:
        return b"frame"

    def close(self) -> None:
        self.closed = True


class TestGroupCoreOps(unittest.TestCase):
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

    def test_group_update_and_detach_scope_behaviors(self) -> None:
        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "g1", "topic": "old", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            update_resp, _ = self._call(
                "group_update",
                {"group_id": group_id, "by": "user", "patch": {"title": "new-title", "topic": "new-topic"}},
            )
            self.assertTrue(update_resp.ok, getattr(update_resp, "error", None))
            group_doc = (update_resp.result or {}).get("group") if isinstance(update_resp.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertEqual(str(group_doc.get("title") or ""), "new-title")
            self.assertEqual(str(group_doc.get("topic") or ""), "new-topic")

            bad_update_resp, _ = self._call(
                "group_update",
                {"group_id": group_id, "by": "user", "patch": {"unknown_key": 1}},
            )
            self.assertFalse(bad_update_resp.ok)
            self.assertEqual((bad_update_resp.error.code if bad_update_resp.error else ""), "invalid_patch")

            with tempfile.TemporaryDirectory(prefix="cccc_scope_") as scope_dir_raw:
                scope_dir = Path(scope_dir_raw)
                attach_resp, _ = self._call(
                    "attach",
                    {"group_id": group_id, "path": str(scope_dir), "by": "user"},
                )
                self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))
                scope_key = str((attach_resp.result or {}).get("scope_key") or "").strip()
                self.assertTrue(scope_key)

                actor_resp, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": "scope-peer",
                        "default_scope_key": scope_key,
                        "enabled": False,
                        "by": "user",
                    },
                )
                self.assertTrue(actor_resp.ok, getattr(actor_resp, "error", None))

                use_resp, _ = self._call(
                    "group_use",
                    {"group_id": group_id, "path": str(scope_dir), "by": "user"},
                )
                self.assertTrue(use_resp.ok, getattr(use_resp, "error", None))
                self.assertEqual(str((use_resp.result or {}).get("active_scope_key") or ""), scope_key)

                detach_resp, _ = self._call(
                    "group_detach_scope",
                    {"group_id": group_id, "scope_key": scope_key, "by": "user"},
                )
                self.assertTrue(detach_resp.ok, getattr(detach_resp, "error", None))
                self.assertEqual(str((detach_resp.result or {}).get("group_id") or ""), group_id)

                show_resp, _ = self._call("group_show", {"group_id": group_id})
                self.assertTrue(show_resp.ok, getattr(show_resp, "error", None))
                show_group = (show_resp.result or {}).get("group") if isinstance(show_resp.result, dict) else {}
                self.assertIsInstance(show_group, dict)
                assert isinstance(show_group, dict)
                scopes = show_group.get("scopes") if isinstance(show_group.get("scopes"), list) else []
                self.assertEqual(len(scopes), 0)
                actors = show_group.get("actors") if isinstance(show_group.get("actors"), list) else []
                scope_peer = next(
                    item
                    for item in actors
                    if isinstance(item, dict) and item.get("id") == "scope-peer"
                )
                self.assertEqual(scope_peer.get("default_scope_key"), "")
        finally:
            cleanup()

    def test_attach_rolls_back_group_scope_and_registry_when_registry_save_fails(self) -> None:
        from cccc.kernel.group import attach_scope_to_group, create_group, load_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.scope import detect_scope

        home_raw, cleanup = self._with_home()
        with tempfile.TemporaryDirectory(prefix="cccc_attach_rollback_") as scope_raw:
            try:
                home = Path(home_raw)
                registry = load_registry()
                group = create_group(
                    registry,
                    title="attach rollback",
                    publish=False,
                )
                scope = detect_scope(Path(scope_raw))
                group_text_before = (group.path / "group.yaml").read_text(encoding="utf-8")
                registry_text_before = (home / "registry.json").read_text(encoding="utf-8")

                with patch.object(
                    registry,
                    "save",
                    side_effect=PermissionError("injected registry failure"),
                ):
                    with self.assertRaisesRegex(PermissionError, "injected registry failure"):
                        attach_scope_to_group(registry, group, scope)

                stored = load_group(group.group_id)
                self.assertIsNotNone(stored)
                assert stored is not None
                self.assertEqual(stored.doc, group.doc)
                self.assertEqual(stored.doc.get("scopes"), [])
                self.assertEqual(stored.doc.get("active_scope_key"), "")
                self.assertEqual(
                    (group.path / "group.yaml").read_text(encoding="utf-8"),
                    group_text_before,
                )
                self.assertEqual(
                    (home / "registry.json").read_text(encoding="utf-8"),
                    registry_text_before,
                )
                self.assertFalse((group.path / "scopes" / scope.scope_key).exists())

                retried = attach_scope_to_group(registry, group, scope)
                self.assertEqual(retried.doc.get("active_scope_key"), scope.scope_key)
                self.assertEqual(load_registry().defaults.get(scope.scope_key), group.group_id)
            finally:
                cleanup()

    def test_group_preamble_get_set_and_reset(self) -> None:
        from cccc.kernel.prompt_files import DEFAULT_PREAMBLE_BODY

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call(
                "group_create", {"title": "preamble", "topic": "", "by": "user"}
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            initial, _ = self._call("group_preamble_get", {"group_id": group_id})
            self.assertTrue(initial.ok, getattr(initial, "error", None))
            self.assertEqual((initial.result or {}).get("source"), "builtin")
            self.assertFalse(bool((initial.result or {}).get("overridden")))
            self.assertEqual(
                (initial.result or {}).get("content"),
                str(DEFAULT_PREAMBLE_BODY or "").strip(),
            )

            custom = "Showrunner startup boundary.\nWait for the targeted mission.\n"
            updated, _ = self._call(
                "group_preamble_set",
                {"group_id": group_id, "content": custom, "by": "user"},
            )
            self.assertTrue(updated.ok, getattr(updated, "error", None))
            self.assertEqual((updated.result or {}).get("source"), "home")
            self.assertTrue(bool((updated.result or {}).get("overridden")))
            self.assertTrue(bool((updated.result or {}).get("changed")))
            self.assertEqual((updated.result or {}).get("content"), custom)

            with patch("cccc.daemon.group.group_ops.write_group_prompt_file") as write_prompt:
                unchanged, _ = self._call(
                    "group_preamble_set",
                    {"group_id": group_id, "content": custom, "by": "user"},
                )
            self.assertTrue(unchanged.ok, getattr(unchanged, "error", None))
            self.assertFalse(bool((unchanged.result or {}).get("changed")))
            write_prompt.assert_not_called()

            oversized, _ = self._call(
                "group_preamble_set",
                {"group_id": group_id, "content": "x" * (512 * 1024 + 1), "by": "user"},
            )
            self.assertFalse(oversized.ok)
            self.assertEqual(
                getattr(oversized.error, "code", ""), "group_preamble_set_failed"
            )
            self.assertIn("524288 UTF-8 bytes", getattr(oversized.error, "message", ""))

            after_rejected, _ = self._call("group_preamble_get", {"group_id": group_id})
            self.assertTrue(after_rejected.ok, getattr(after_rejected, "error", None))
            self.assertEqual((after_rejected.result or {}).get("content"), custom)

            invalid, _ = self._call(
                "group_preamble_set",
                {"group_id": group_id, "content": "  ", "by": "user"},
            )
            self.assertFalse(invalid.ok)
            self.assertEqual(getattr(invalid.error, "code", ""), "invalid_content")

            unconfirmed, _ = self._call(
                "group_preamble_reset",
                {"group_id": group_id, "confirm": "wrong", "by": "user"},
            )
            self.assertFalse(unconfirmed.ok)
            self.assertEqual(getattr(unconfirmed.error, "code", ""), "confirm_required")

            reset, _ = self._call(
                "group_preamble_reset",
                {"group_id": group_id, "confirm": "preamble", "by": "user"},
            )
            self.assertTrue(reset.ok, getattr(reset, "error", None))
            self.assertEqual((reset.result or {}).get("source"), "builtin")
            self.assertFalse(bool((reset.result or {}).get("overridden")))
            self.assertTrue(bool((reset.result or {}).get("changed")))
            self.assertEqual(
                (reset.result or {}).get("content"),
                str(DEFAULT_PREAMBLE_BODY or "").strip(),
            )
        finally:
            cleanup()

    def test_group_help_and_actor_notes_use_one_permissioned_file(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.prompt_files import HELP_FILENAME, read_group_prompt_file

        _, cleanup = self._with_home()
        try:
            missing_group_id, _ = self._call("group_help_get", {"by": "user"})
            self.assertFalse(missing_group_id.ok)
            self.assertEqual(
                getattr(missing_group_id.error, "code", ""), "missing_group_id"
            )
            unknown_group, _ = self._call(
                "actor_notes_get", {"group_id": "missing", "by": "user"}
            )
            self.assertFalse(unknown_group.ok)
            self.assertEqual(
                getattr(unknown_group.error, "code", ""), "group_not_found"
            )

            created, _ = self._call(
                "group_create", {"title": "help", "topic": "", "by": "user"}
            )
            group_id = str((created.result or {}).get("group_id") or "")
            for actor_id in ("lead", "peer"):
                added, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "runtime": "custom",
                        "runner": "pty",
                        "command": ["sh", "-c", "exit 0"],
                        "by": "user",
                    },
                )
                self.assertTrue(added.ok, getattr(added, "error", None))

            updated, _ = self._call(
                "actor_notes_set",
                {
                    "group_id": group_id,
                    "target_actor_id": "peer",
                    "content": "Keep receipts.",
                    "by": "lead",
                },
            )
            self.assertTrue(updated.ok, getattr(updated, "error", None))
            self.assertTrue(bool((updated.result or {}).get("changed")))

            own, _ = self._call(
                "actor_notes_get",
                {"group_id": group_id, "target_actor_id": "peer", "by": "peer"},
            )
            self.assertTrue(own.ok, getattr(own, "error", None))
            self.assertEqual((own.result or {}).get("content"), "Keep receipts.")

            denied_read, _ = self._call(
                "actor_notes_get",
                {"group_id": group_id, "target_actor_id": "lead", "by": "peer"},
            )
            self.assertFalse(denied_read.ok)
            self.assertEqual(getattr(denied_read.error, "code", ""), "permission_denied")

            denied_write, _ = self._call(
                "actor_notes_set",
                {
                    "group_id": group_id,
                    "target_actor_id": "peer",
                    "content": "self-authored",
                    "by": "peer",
                },
            )
            self.assertFalse(denied_write.ok)
            self.assertEqual(getattr(denied_write.error, "code", ""), "permission_denied")

            effective, _ = self._call(
                "group_help_get",
                {"group_id": group_id, "actor_id": "peer", "by": "peer"},
            )
            self.assertTrue(effective.ok, getattr(effective, "error", None))
            markdown = str((effective.result or {}).get("markdown") or "")
            self.assertIn("## Notes for you", markdown)
            self.assertIn("Keep receipts.", markdown)
            self.assertNotIn("## Foreman", markdown)

            cleared, _ = self._call(
                "actor_notes_clear",
                {"group_id": group_id, "target_actor_id": "peer", "by": "user"},
            )
            self.assertTrue(cleared.ok, getattr(cleared, "error", None))
            self.assertTrue(bool((cleared.result or {}).get("changed")))
            self.assertEqual((cleared.result or {}).get("content"), "")
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            self.assertFalse(read_group_prompt_file(group, HELP_FILENAME).found)
        finally:
            cleanup()

    def test_group_use_rejects_exact_cccc_home_as_workspace_scope(self) -> None:
        home, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "g1", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            use_resp, _ = self._call("group_use", {"group_id": group_id, "path": home, "by": "user"})
            self.assertFalse(use_resp.ok)
            self.assertEqual(getattr(use_resp.error, "code", ""), "invalid_scope_path")
        finally:
            cleanup()

    def test_group_delete_clears_active_and_removes_group(self) -> None:
        from cccc.kernel.active import load_active, set_active_group_id
        from cccc.kernel.group import load_group
        from cccc.kernel.web_model_connectors import (
            create_web_model_connector,
            verify_web_model_connector_secret,
        )

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "delete-me", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            actor_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "web1",
                    "runtime": "web_model",
                    "runner": "headless",
                    "enabled": False,
                    "by": "user",
                },
            )
            self.assertTrue(actor_resp.ok, getattr(actor_resp, "error", None))
            connector = create_web_model_connector(
                group_id=group_id,
                actor_id="web1",
                provider="chatgpt",
            )

            set_active_group_id(group_id)
            self.assertEqual(str(load_active().get("active_group_id") or ""), group_id)

            delete_resp, _ = self._call("group_delete", {"group_id": group_id, "by": "user"})
            self.assertTrue(delete_resp.ok, getattr(delete_resp, "error", None))
            self.assertEqual(str((delete_resp.result or {}).get("group_id") or ""), group_id)

            self.assertIsNone(load_group(group_id))
            self.assertEqual(str(load_active().get("active_group_id") or ""), "")
            self.assertIsNone(
                verify_web_model_connector_secret(
                    str(connector.get("connector_id") or ""),
                    str(connector.get("secret") or ""),
                )
            )

            show_resp, _ = self._call("group_show", {"group_id": group_id})
            self.assertFalse(show_resp.ok)
            self.assertEqual((show_resp.error.code if show_resp.error else ""), "group_not_found")
        finally:
            cleanup()

    def test_group_delete_closes_its_presentation_browser_sessions(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as browser_runtime
        from cccc.daemon.group import presentation_browser_runtime as presentation

        _, cleanup = self._with_home()
        fake = _FakePresentationRuntime()
        try:
            create_resp, _ = self._call(
                "group_create",
                {"title": "delete-browser", "topic": "", "by": "user"},
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "")
            with patch.object(
                browser_runtime,
                "launch_projected_browser_runtime",
                return_value=fake,
            ):
                opened = presentation.open_browser_surface_session(
                    group_id=group_id,
                    slot_id="slot-1",
                    url="http://127.0.0.1:3000",
                    width=1280,
                    height=800,
                )
            self.assertEqual(opened.get("state"), "ready")

            deleted, _ = self._call(
                "group_delete", {"group_id": group_id, "by": "user"}
            )

            self.assertTrue(deleted.ok, getattr(deleted, "error", None))
            self.assertTrue(fake.closed)
        finally:
            presentation.close_all_browser_surface_sessions()
            cleanup()

    def test_group_reset_closes_source_presentation_browser_sessions(self) -> None:
        from cccc.daemon.browser import projected_browser_runtime as browser_runtime
        from cccc.daemon.group import presentation_browser_runtime as presentation

        _, cleanup = self._with_home()
        fake = _FakePresentationRuntime()
        try:
            create_resp, _ = self._call(
                "group_create",
                {"title": "reset-browser", "topic": "", "by": "user"},
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "")
            with patch.object(
                browser_runtime,
                "launch_projected_browser_runtime",
                return_value=fake,
            ):
                presentation.open_browser_surface_session(
                    group_id=group_id,
                    slot_id="slot-1",
                    url="http://127.0.0.1:3000",
                    width=1280,
                    height=800,
                )

            reset, _ = self._call(
                "group_reset",
                {"group_id": group_id, "confirm": group_id, "by": "user"},
            )

            self.assertTrue(reset.ok, getattr(reset, "error", None))
            self.assertTrue(fake.closed)
        finally:
            presentation.close_all_browser_surface_sessions()
            cleanup()

    def test_group_delete_tolerates_transient_directory_not_empty(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "delete-race", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            real_rmtree = shutil.rmtree
            injected = {"raised": False}

            def _flaky_rmtree(path, *args, **kwargs):
                name = Path(path).name
                if group_id in name and not injected["raised"]:
                    injected["raised"] = True
                    raise OSError(errno.ENOTEMPTY, "Directory not empty")
                return real_rmtree(path, *args, **kwargs)

            with patch("cccc.kernel.group.shutil.rmtree", side_effect=_flaky_rmtree):
                delete_resp, _ = self._call("group_delete", {"group_id": group_id, "by": "user"})

            self.assertTrue(injected["raised"])
            self.assertTrue(delete_resp.ok, getattr(delete_resp, "error", None))
            self.assertIsNone(load_group(group_id))
        finally:
            cleanup()

    def test_group_delete_removes_canonical_path_recreated_after_rename(self) -> None:
        from cccc.kernel.group import load_group

        home_raw, cleanup = self._with_home()
        try:
            created, _ = self._call(
                "group_create",
                {"title": "delete-recreated", "topic": "", "by": "user"},
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "")
            group_path = Path(home_raw) / "groups" / group_id
            real_rmtree = shutil.rmtree
            recreated = {"done": False}

            def _recreating_rmtree(path, *args, **kwargs):
                candidate = Path(path)
                if (
                    candidate.name.startswith(f".{group_id}.deleting-")
                    and not recreated["done"]
                ):
                    recreated["done"] = True
                    (group_path / "state").mkdir(parents=True)
                    (group_path / "state" / "late-write").write_text(
                        "stale background write",
                        encoding="utf-8",
                    )
                return real_rmtree(candidate, *args, **kwargs)

            with patch(
                "cccc.kernel.group.shutil.rmtree",
                side_effect=_recreating_rmtree,
            ):
                deleted, _ = self._call(
                    "group_delete", {"group_id": group_id, "by": "user"}
                )

            self.assertTrue(recreated["done"])
            self.assertTrue(deleted.ok, getattr(deleted, "error", None))
            self.assertFalse(group_path.exists())
            self.assertIsNone(load_group(group_id))
        finally:
            cleanup()

    def test_group_delete_restores_group_routing_and_secrets_when_registry_save_fails(self) -> None:
        from cccc.daemon.actors.private_env_ops import load_actor_private_env
        from cccc.daemon.space.group_space_store import (
            enqueue_space_job,
            get_space_binding,
            list_space_jobs,
            upsert_space_binding,
        )
        from cccc.kernel.active import load_active, set_active_group_id
        from cccc.kernel.group import load_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.web_model_connectors import (
            create_web_model_connector,
            verify_web_model_connector_secret,
        )

        home_raw, cleanup = self._with_home()
        try:
            home = Path(home_raw)
            created, _ = self._call(
                "group_create",
                {"title": "delete rollback", "topic": "", "by": "user"},
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "")
            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "rollback-peer",
                    "runtime": "custom",
                    "runner": "pty",
                    "command": ["sh"],
                    "enabled": False,
                    "env_private": {"ROLLBACK_SECRET": "preserve-me"},
                    "by": "user",
                },
            )
            self.assertTrue(added.ok, getattr(added, "error", None))
            connector = create_web_model_connector(
                group_id=group_id,
                actor_id="rollback-peer",
                provider="chatgpt",
            )
            upsert_space_binding(
                group_id,
                provider="notebooklm",
                lane="work",
                remote_space_id="nb-rollback",
                by="user",
            )
            job, deduped = enqueue_space_job(
                group_id=group_id,
                provider="notebooklm",
                lane="work",
                remote_space_id="nb-rollback",
                kind="context_sync",
                payload={"title": "rollback"},
                idempotency_key="group-delete-rollback",
            )
            self.assertFalse(deduped)
            payload_ref = str(job.get("payload_ref") or "")
            self.assertTrue(payload_ref)
            set_active_group_id(group_id)
            registry_text_before = (home / "registry.json").read_text(encoding="utf-8")

            with patch(
                "cccc.kernel.registry.Registry.save",
                side_effect=PermissionError("injected registry failure"),
            ):
                failed, _ = self._call(
                    "group_delete", {"group_id": group_id, "by": "user"}
                )

            self.assertFalse(failed.ok)
            self.assertEqual(getattr(failed.error, "code", ""), "group_delete_failed")
            self.assertIsNotNone(load_group(group_id))
            self.assertIn(group_id, load_registry().groups)
            self.assertEqual(
                (home / "registry.json").read_text(encoding="utf-8"),
                registry_text_before,
            )
            self.assertEqual(str(load_active().get("active_group_id") or ""), group_id)
            self.assertEqual(
                load_actor_private_env(group_id, "rollback-peer").get("ROLLBACK_SECRET"),
                "preserve-me",
            )
            self.assertIsNotNone(
                verify_web_model_connector_secret(
                    str(connector.get("connector_id") or ""),
                    str(connector.get("secret") or ""),
                )
            )
            self.assertEqual(
                str(
                    (
                        get_space_binding(
                            group_id,
                            provider="notebooklm",
                            lane="work",
                        )
                        or {}
                    ).get("remote_space_id")
                    or ""
                ),
                "nb-rollback",
            )
            self.assertEqual(len(list_space_jobs(group_id=group_id)), 1)
            self.assertTrue((home / "state" / "space" / "job_payloads" / payload_ref).is_file())

            retried, _ = self._call(
                "group_delete", {"group_id": group_id, "by": "user"}
            )
            self.assertTrue(retried.ok, getattr(retried, "error", None))
            self.assertIsNone(load_group(group_id))
            self.assertNotIn(group_id, load_registry().groups)
            self.assertEqual(str(load_active().get("active_group_id") or ""), "")
            self.assertEqual(load_actor_private_env(group_id, "rollback-peer"), {})
            self.assertIsNone(
                verify_web_model_connector_secret(
                    str(connector.get("connector_id") or ""),
                    str(connector.get("secret") or ""),
                )
            )
            self.assertIsNone(
                get_space_binding(
                    group_id,
                    provider="notebooklm",
                    lane="work",
                )
            )
            self.assertEqual(list_space_jobs(group_id=group_id), [])
            self.assertFalse((home / "state" / "space" / "job_payloads" / payload_ref).exists())
        finally:
            cleanup()

    def test_group_reset_creates_clean_replacement_and_deletes_old(self) -> None:
        from cccc.daemon.actors.private_env_ops import load_actor_private_env, update_actor_private_env
        from cccc.kernel.active import load_active, set_active_group_id
        from cccc.kernel.group import load_group
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        with tempfile.TemporaryDirectory(prefix="cccc_scope_") as scope_dir_raw:
            try:
                create_resp, _ = self._call("group_create", {"title": "reset-me", "topic": "topic-a", "by": "user"})
                self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
                group_id = str((create_resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

                attach_resp, _ = self._call("attach", {"group_id": group_id, "path": scope_dir_raw, "by": "user"})
                self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))
                scope_key = str((attach_resp.result or {}).get("scope_key") or "").strip()
                self.assertTrue(scope_key)

                custom_rule = {
                    "id": "daily_check",
                    "enabled": True,
                    "scope": "group",
                    "owner_actor_id": None,
                    "to": ["@foreman"],
                    "trigger": {"kind": "interval", "every_seconds": 60},
                    "action": {
                        "kind": "notify",
                        "priority": "normal",
                        "requires_ack": False,
                        "title": "Daily check",
                        "message": "check progress",
                    },
                }
                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                group.doc["actors"] = [
                    {
                        "id": "peer1",
                        "title": "Peer One",
                        "command": ["codex"],
                        "env": {"PUBLIC_FLAG": "1"},
                        "default_scope_key": scope_key,
                        "runner": "pty",
                        "runtime": "codex",
                        "enabled": True,
                        "avatar_asset_path": str(group.path / "blobs" / "avatars" / "peer1.png"),
                        "created_at": "2026-01-01T00:00:00Z",
                    }
                ]
                group.doc["messaging"] = {"default_send_to": "broadcast"}
                group.doc["delivery"] = {"min_interval_seconds": 42, "auto_mark_on_delivery": "read"}
                group.doc["terminal_transcript"] = {
                    "visibility": "all",
                    "notify_tail": True,
                    "notify_lines": 12,
                }
                group.doc["features"] = {"legacy_flag": True, "panorama_enabled": True}
                group.doc["automation"] = {
                    "version": 7,
                    "rules": [custom_rule],
                    "snippets": {"custom_note": "custom automation note"},
                    "snippet_overrides": {"standup": "custom standup"},
                    "nudge_after_seconds": 123,
                    "keepalive_delay_seconds": 456,
                    "runtime_last_tick": "should not be copied",
                }
                group.doc["settings"] = {
                    "nudge_after_seconds": 999,
                    "help_nudge_interval_seconds": 777,
                    "default_send_to": "broadcast",
                }
                group.doc["runtime_states"] = {"peer1": {"status": "working"}}
                group.doc["assistants"] = {"active_document_id": "old-document"}
                group.doc["im"] = {"enabled": True}
                group.doc["im_bridge"] = {"running": True}
                group.doc["group_bridge"] = {"status": "connected"}
                group.doc["web_model_delivery_preferences"] = {
                    "peer1": {"mode": "image_compat"}
                }
                group.save()
                state_path = group.path / "state" / "automation.json"
                state_path.write_text('{"runtime_marker": true}\n', encoding="utf-8")
                update_actor_private_env(
                    group_id,
                    "peer1",
                    set_vars={"API_KEY": "secret-value"},
                    unset_keys=[],
                    clear=False,
                )
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group_id,
                    scope_key=scope_key,
                    by="user",
                    data={"text": "old history should not be copied"},
                )
                set_active_group_id(group_id)

                reset_resp, _ = self._call(
                    "group_reset",
                    {"group_id": group_id, "confirm": group_id, "by": "user"},
                )
                self.assertTrue(reset_resp.ok, getattr(reset_resp, "error", None))
                result = reset_resp.result if isinstance(reset_resp.result, dict) else {}
                new_group_id = str(result.get("new_group_id") or "").strip()
                self.assertTrue(new_group_id)
                self.assertNotEqual(new_group_id, group_id)
                self.assertTrue(bool(result.get("deleted_old")))

                self.assertIsNone(load_group(group_id))
                replacement = load_group(new_group_id)
                self.assertIsNotNone(replacement)
                assert replacement is not None
                self.assertEqual(replacement.doc.get("title"), "reset-me")
                self.assertEqual(replacement.doc.get("topic"), "topic-a")
                self.assertEqual(str(replacement.doc.get("active_scope_key") or ""), scope_key)
                scopes = replacement.doc.get("scopes") if isinstance(replacement.doc.get("scopes"), list) else []
                self.assertEqual(len(scopes), 1)
                self.assertEqual(str(scopes[0].get("scope_key") or ""), scope_key)

                actors = replacement.doc.get("actors") if isinstance(replacement.doc.get("actors"), list) else []
                self.assertEqual(len(actors), 1)
                self.assertEqual(str(actors[0].get("id") or ""), "peer1")
                self.assertEqual(str(actors[0].get("runtime") or ""), "codex")
                self.assertEqual(str(actors[0].get("avatar_asset_path") or ""), "")
                self.assertEqual(load_actor_private_env(new_group_id, "peer1"), {"API_KEY": "secret-value"})
                self.assertEqual(load_actor_private_env(group_id, "peer1"), {})
                automation = (
                    replacement.doc.get("automation") if isinstance(replacement.doc.get("automation"), dict) else {}
                )
                self.assertEqual(int(automation.get("version") or 0), 7)
                self.assertEqual(automation.get("rules"), [custom_rule])
                self.assertEqual(automation.get("snippets"), {"custom_note": "custom automation note"})
                self.assertEqual(automation.get("snippet_overrides"), {"standup": "custom standup"})
                self.assertEqual(int(automation.get("nudge_after_seconds") or 0), 123)
                self.assertEqual(int(automation.get("keepalive_delay_seconds") or 0), 456)
                self.assertEqual(int(automation.get("help_nudge_interval_seconds") or 0), 777)
                self.assertNotIn("runtime_last_tick", automation)
                self.assertNotIn("settings", replacement.doc)
                self.assertNotIn("messaging", replacement.doc)
                self.assertNotIn("delivery", replacement.doc)
                self.assertNotIn("terminal_transcript", replacement.doc)
                self.assertNotIn("features", replacement.doc)
                for key in (
                    "runtime_states",
                    "assistants",
                    "im",
                    "im_bridge",
                    "group_bridge",
                    "web_model_delivery_preferences",
                ):
                    self.assertNotIn(key, replacement.doc)
                self.assertFalse((replacement.path / "state" / "automation.json").exists())

                ledger_text = replacement.ledger_path.read_text(encoding="utf-8")
                self.assertIn("group.create", ledger_text)
                self.assertNotIn("old history should not be copied", ledger_text)
                self.assertEqual(str(load_active().get("active_group_id") or ""), new_group_id)
                self.assertEqual(load_registry().defaults.get(scope_key), new_group_id)
            finally:
                cleanup()

    def test_group_reset_non_active_group_does_not_switch_active_group(self) -> None:
        from cccc.kernel.active import load_active, set_active_group_id
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            target_resp, _ = self._call("group_create", {"title": "reset-target", "topic": "", "by": "user"})
            self.assertTrue(target_resp.ok, getattr(target_resp, "error", None))
            target_group_id = str((target_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(target_group_id)

            active_resp, _ = self._call("group_create", {"title": "keep-active", "topic": "", "by": "user"})
            self.assertTrue(active_resp.ok, getattr(active_resp, "error", None))
            active_group_id = str((active_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(active_group_id)
            self.assertNotEqual(active_group_id, target_group_id)
            set_active_group_id(active_group_id)

            reset_resp, _ = self._call(
                "group_reset",
                {"group_id": target_group_id, "confirm": target_group_id, "by": "user"},
            )
            self.assertTrue(reset_resp.ok, getattr(reset_resp, "error", None))
            result = reset_resp.result if isinstance(reset_resp.result, dict) else {}
            new_group_id = str(result.get("new_group_id") or "").strip()
            self.assertTrue(new_group_id)
            self.assertNotIn("active_group_id", result)
            self.assertIsNone(load_group(target_group_id))
            self.assertIsNotNone(load_group(new_group_id))
            self.assertEqual(str(load_active().get("active_group_id") or ""), active_group_id)
        finally:
            cleanup()

    def test_group_reset_preparation_failure_rolls_back_replacement_state(self) -> None:
        from cccc.kernel.active import load_active, set_active_group_id
        from cccc.kernel.group import load_group
        from cccc.kernel.registry import load_registry

        home_raw, cleanup = self._with_home()
        with tempfile.TemporaryDirectory(prefix="cccc_scope_") as scope_dir_raw:
            try:
                create_resp, _ = self._call(
                    "group_create",
                    {"title": "reset-rollback", "topic": "", "by": "user"},
                )
                self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
                group_id = str((create_resp.result or {}).get("group_id") or "").strip()
                attach_resp, _ = self._call(
                    "attach",
                    {"group_id": group_id, "path": scope_dir_raw, "by": "user"},
                )
                self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))
                scope_key = str((attach_resp.result or {}).get("scope_key") or "").strip()
                set_active_group_id(group_id)
                replacement_ids: list[str] = []

                def _fail_copy(_: str, replacement_group_id: str) -> int:
                    replacement_ids.append(replacement_group_id)
                    target = Path(home_raw) / "state" / "secrets" / "actors" / replacement_group_id
                    target.mkdir(parents=True)
                    (target / "partial.json").write_text("{}\n", encoding="utf-8")
                    raise OSError("injected secret-copy failure")

                with patch("cccc.daemon.group.group_ops.copy_group_private_env", side_effect=_fail_copy):
                    reset_resp, _ = self._call(
                        "group_reset",
                        {"group_id": group_id, "confirm": group_id, "by": "user"},
                    )

                self.assertFalse(reset_resp.ok)
                self.assertEqual((reset_resp.error.code if reset_resp.error else ""), "group_reset_failed")
                self.assertEqual(len(replacement_ids), 1)
                replacement_group_id = replacement_ids[0]
                self.assertIsNotNone(load_group(group_id))
                self.assertIsNone(load_group(replacement_group_id))
                registry = load_registry()
                self.assertEqual(set(registry.groups), {group_id})
                self.assertEqual(registry.defaults.get(scope_key), group_id)
                self.assertEqual(str(load_active().get("active_group_id") or ""), group_id)
                self.assertFalse(
                    (Path(home_raw) / "state" / "secrets" / "actors" / replacement_group_id).exists()
                )
            finally:
                cleanup()

    def test_group_reset_requires_matching_confirm(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "reset-confirm", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            reset_resp, _ = self._call("group_reset", {"group_id": group_id, "confirm": "wrong", "by": "user"})
            self.assertFalse(reset_resp.ok)
            self.assertEqual((reset_resp.error.code if reset_resp.error else ""), "confirm_required")
            self.assertIsNotNone(load_group(group_id))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
