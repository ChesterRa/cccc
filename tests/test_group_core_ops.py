import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import errno
import shutil


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

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "delete-me", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            set_active_group_id(group_id)
            self.assertEqual(str(load_active().get("active_group_id") or ""), group_id)

            delete_resp, _ = self._call("group_delete", {"group_id": group_id, "by": "user"})
            self.assertTrue(delete_resp.ok, getattr(delete_resp, "error", None))
            self.assertEqual(str((delete_resp.result or {}).get("group_id") or ""), group_id)

            self.assertIsNone(load_group(group_id))
            self.assertEqual(str(load_active().get("active_group_id") or ""), "")

            show_resp, _ = self._call("group_show", {"group_id": group_id})
            self.assertFalse(show_resp.ok)
            self.assertEqual((show_resp.error.code if show_resp.error else ""), "group_not_found")
        finally:
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
                if name == group_id and not injected["raised"]:
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
                group.doc["features"] = {"desktop_pet_enabled": True, "panorama_enabled": True}
                group.doc["automation"] = {
                    "version": 7,
                    "rules": [custom_rule],
                    "snippets": {"custom_note": "custom automation note"},
                    "snippet_overrides": {"standup": "custom standup"},
                    "nudge_after_seconds": 123,
                    "keepalive_delay_seconds": 456,
                    "runtime_last_tick": "should not be copied",
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
                self.assertNotIn("runtime_last_tick", automation)
                self.assertNotIn("messaging", replacement.doc)
                self.assertNotIn("delivery", replacement.doc)
                self.assertNotIn("terminal_transcript", replacement.doc)
                self.assertNotIn("features", replacement.doc)
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
