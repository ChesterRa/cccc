import base64
import io
import json
import os
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

import yaml  # type: ignore


class TestGroupCopyOps(unittest.TestCase):
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

    def _create_group_with_scope(self, workspace: Path) -> tuple[str, str]:
        create_resp, _ = self._call("group_create", {"title": "copy-src", "topic": "copy topic", "by": "user"})
        self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
        group_id = str((create_resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)
        attach_resp, _ = self._call("attach", {"group_id": group_id, "path": str(workspace), "by": "user"})
        self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))
        scope_key = str((attach_resp.result or {}).get("scope_key") or "").strip()
        self.assertTrue(scope_key)
        return group_id, scope_key

    def _mark_actor_with_secret_and_missing_profile(self, group_id: str, actor_id: str) -> None:
        from cccc.kernel.group import load_group

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        doc = yaml.safe_load((group.path / "group.yaml").read_text(encoding="utf-8")) or {}
        actors = doc.get("actors") if isinstance(doc.get("actors"), list) else []
        for actor in actors:
            if isinstance(actor, dict) and str(actor.get("id") or "") == actor_id:
                actor["env"] = {"API_TOKEN": "secret-value"}
                actor["profile_id"] = "missing-profile"
                actor["profile_scope"] = "global"
                actor["profile_owner"] = "user"
                actor["profile_revision_applied"] = "rev-missing"
                break
        (group.path / "group.yaml").write_text(yaml.safe_dump(doc, allow_unicode=True, sort_keys=False), encoding="utf-8")

    def _package_bytes(self, group_id: str) -> bytes:
        export_resp, _ = self._call("group_copy_export", {"group_id": group_id, "by": "user"})
        self.assertTrue(export_resp.ok, getattr(export_resp, "error", None))
        raw = str((export_resp.result or {}).get("package_b64") or "")
        self.assertTrue(raw)
        return base64.b64decode(raw)

    def test_export_excludes_runtime_state_and_secrets(self) -> None:
        from cccc.kernel.group import load_group

        _home, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace_raw:
                group_id, scope_key = self._create_group_with_scope(Path(workspace_raw))
                add_resp, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "runtime": "codex",
                        "runner": "pty",
                        "by": "user",
                    },
                )
                self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))
                self._mark_actor_with_secret_and_missing_profile(group_id, "peer1")

                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                event = {
                    "id": "evt1",
                    "kind": "chat.message",
                    "group_id": group_id,
                    "scope_key": scope_key,
                    "data": {"cross_group_id": group_id, "text": "hello"},
                }
                group.ledger_path.write_text(json.dumps(event, ensure_ascii=False) + "\n", encoding="utf-8")
                (group.path / "state" / "read_cursors.json").parent.mkdir(parents=True, exist_ok=True)
                (group.path / "state" / "read_cursors.json").write_text('{"peer1":"evt1"}', encoding="utf-8")
                (group.path / "state" / "assistants.json").write_text(
                    json.dumps(
                        {
                            "voice-secretary": {
                                "document_path": "/old/home/docs/voice-secretary/notes.md",
                                "document_workspace_root": "/old/home",
                            }
                        }
                    ),
                    encoding="utf-8",
                )
                (group.path / "state" / "unread_index.json").write_text('{"derived":true}', encoding="utf-8")
                (group.path / "state" / "preamble_sent.json").write_text('{"peer1":"yes"}', encoding="utf-8")
                (group.path / "state" / "runners" / "pty").mkdir(parents=True, exist_ok=True)
                (group.path / "state" / "runners" / "pty" / "peer1.json").write_text("{}", encoding="utf-8")
                (group.path / "state" / "runtime_sessions").mkdir(parents=True, exist_ok=True)
                (group.path / "state" / "runtime_sessions" / "peer1.json").write_text("{}", encoding="utf-8")
                (group.path / "state" / "ledger").mkdir(parents=True, exist_ok=True)
                (group.path / "state" / "ledger" / "index.sqlite3").write_text("cache", encoding="utf-8")
                (group.path / "state" / "ledger" / "snapshot.latest.json").write_text('{"snapshot":true}', encoding="utf-8")
                (group.path / "state" / "blobs").mkdir(parents=True, exist_ok=True)
                (group.path / "state" / "blobs" / "blob.txt").write_text("blob", encoding="utf-8")
                (group.path / "state" / "notebooklm_auth").mkdir(parents=True, exist_ok=True)
                (group.path / "state" / "notebooklm_auth" / "cookies.json").write_text("secret", encoding="utf-8")
                (group.path / "state" / "browser").mkdir(parents=True, exist_ok=True)
                (group.path / "state" / "browser" / "session.json").write_text("secret", encoding="utf-8")
                (group.path / "state" / "tokens").mkdir(parents=True, exist_ok=True)
                (group.path / "state" / "tokens" / "provider.json").write_text("secret", encoding="utf-8")
                (group.path / "runtime" / "codex" / "peer1" / ".tmp" / "plugins").mkdir(parents=True, exist_ok=True)
                (group.path / "runtime" / "codex" / "peer1" / ".tmp" / "plugins" / "cache.pack").write_text(
                    "runtime cache",
                    encoding="utf-8",
                )

                package = self._package_bytes(group_id)
                with zipfile.ZipFile(io.BytesIO(package), "r") as zf:
                    names = set(zf.namelist())
                    self.assertIn("manifest.json", names)
                    self.assertIn("group/group.yaml", names)
                    self.assertIn("group/ledger.jsonl", names)
                    self.assertIn("group/state/read_cursors.json", names)
                    self.assertIn("group/state/ledger/snapshot.latest.json", names)
                    self.assertIn("group/state/blobs/blob.txt", names)
                    self.assertNotIn("group/state/assistants.json", names)
                    self.assertNotIn("group/state/unread_index.json", names)
                    self.assertNotIn("group/state/preamble_sent.json", names)
                    self.assertNotIn("group/state/runners/pty/peer1.json", names)
                    self.assertNotIn("group/state/runtime_sessions/peer1.json", names)
                    self.assertNotIn("group/state/ledger/index.sqlite3", names)
                    self.assertNotIn("group/state/notebooklm_auth/cookies.json", names)
                    self.assertNotIn("group/state/browser/session.json", names)
                    self.assertNotIn("group/state/tokens/provider.json", names)
                    self.assertNotIn("group/runtime/codex/peer1/.tmp/plugins/cache.pack", names)
                    manifest = json.loads(zf.read("manifest.json").decode("utf-8"))
                    self.assertEqual(manifest.get("kind"), "cccc.group_copy")
                    self.assertFalse(bool(manifest.get("workspace_included")))
                    self.assertFalse(bool(manifest.get("contains_secrets")))
                    packaged_doc = yaml.safe_load(zf.read("group/group.yaml").decode("utf-8")) or {}
                    actors = packaged_doc.get("actors") if isinstance(packaged_doc.get("actors"), list) else []
                    self.assertEqual(actors[0].get("env"), {})
        finally:
            cleanup()

    def test_conflict_import_creates_inert_copy_without_stealing_workspace_default(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.registry import load_registry

        _home, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace_raw, tempfile.TemporaryDirectory() as remap_raw:
                group_id, scope_key = self._create_group_with_scope(Path(workspace_raw))
                add_resp, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "runtime": "codex",
                        "runner": "pty",
                        "by": "user",
                    },
                )
                self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))
                self._mark_actor_with_secret_and_missing_profile(group_id, "peer1")
                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                doc = dict(group.doc)
                actors = doc.get("actors") if isinstance(doc.get("actors"), list) else []
                self.assertTrue(actors)
                actors[0]["default_scope_key"] = scope_key
                (group.path / "group.yaml").write_text(
                    yaml.safe_dump(doc, allow_unicode=True, sort_keys=False),
                    encoding="utf-8",
                )
                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                event = {
                    "id": "evt1",
                    "kind": "chat.message",
                    "group_id": group_id,
                    "scope_key": scope_key,
                    "data": {"cross_group_id": group_id},
                }
                group.ledger_path.write_text(json.dumps(event, ensure_ascii=False) + "\n", encoding="utf-8")
                (group.path / "state").mkdir(parents=True, exist_ok=True)
                (group.path / "state" / "read_cursors.json").write_text('{"peer1":"evt1"}', encoding="utf-8")
                (group.path / "state" / "assistants.json").write_text(
                    json.dumps(
                        {
                            "voice-secretary": {
                                "document_path": "/old/home/docs/voice-secretary/notes.md",
                                "document_workspace_root": "/old/home",
                            }
                        }
                    ),
                    encoding="utf-8",
                )
                (group.path / "state" / "preamble_sent.json").write_text('{"peer1":"yes"}', encoding="utf-8")

                package = self._package_bytes(group_id)
                package_b64 = base64.b64encode(package).decode("ascii")
                import_resp, _ = self._call(
                    "group_copy_import",
                    {"package_b64": package_b64, "workspace_root": remap_raw, "by": "user"},
                )
                self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))
                new_group_id = str((import_resp.result or {}).get("group_id") or "")
                self.assertTrue(new_group_id)
                self.assertNotEqual(new_group_id, group_id)

                imported = load_group(new_group_id)
                self.assertIsNotNone(imported)
                assert imported is not None
                self.assertFalse(bool(imported.doc.get("running")))
                self.assertEqual(str(imported.doc.get("state") or ""), "idle")
                self.assertNotEqual(str(imported.doc.get("active_scope_key") or ""), scope_key)
                self.assertEqual(str((imported.doc.get("scopes") or [{}])[0].get("url") or ""), str(Path(remap_raw).resolve()))
                actors = imported.doc.get("actors") if isinstance(imported.doc.get("actors"), list) else []
                self.assertEqual(actors[0].get("env"), {})
                self.assertNotIn("profile_id", actors[0])
                self.assertEqual(str(actors[0].get("default_scope_key") or ""), str(imported.doc.get("active_scope_key") or ""))
                self.assertTrue((imported.path / "state" / "read_cursors.json").exists())
                self.assertFalse((imported.path / "state" / "assistants.json").exists())
                self.assertFalse((imported.path / "state" / "preamble_sent.json").exists())

                imported_events = imported.ledger_path.read_text(encoding="utf-8").splitlines()
                imported_event = json.loads(imported_events[0])
                self.assertEqual(imported_event.get("group_id"), new_group_id)
                self.assertEqual(imported_event.get("data", {}).get("cross_group_id"), group_id)

                reg = load_registry()
                self.assertEqual(reg.defaults.get(scope_key), group_id)
        finally:
            cleanup()

    def test_export_file_and_path_import_avoid_package_b64(self) -> None:
        from cccc.kernel.group import load_group

        _home, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace_raw, tempfile.TemporaryDirectory() as remap_raw:
                group_id, _scope_key = self._create_group_with_scope(Path(workspace_raw))
                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                (group.path / "runtime" / "codex" / "peer1").mkdir(parents=True, exist_ok=True)
                (group.path / "runtime" / "codex" / "peer1" / "cache.pack").write_text("runtime cache", encoding="utf-8")

                export_resp, _ = self._call("group_copy_export_file", {"group_id": group_id, "by": "user"})
                self.assertTrue(export_resp.ok, getattr(export_resp, "error", None))
                result = export_resp.result or {}
                self.assertNotIn("package_b64", result)
                package_path = Path(str(result.get("package_path") or ""))
                self.assertTrue(package_path.is_file(), result)

                with zipfile.ZipFile(package_path, "r") as zf:
                    self.assertNotIn("group/runtime/codex/peer1/cache.pack", set(zf.namelist()))

                preview_resp, _ = self._call("group_copy_preview_import", {"package_path": str(package_path)})
                self.assertTrue(preview_resp.ok, getattr(preview_resp, "error", None))
                preview = (preview_resp.result or {}).get("preview") or {}
                self.assertEqual(str(preview.get("source_group_id") or ""), group_id)

                import_resp, _ = self._call(
                    "group_copy_import",
                    {"package_path": str(package_path), "workspace_root": remap_raw, "by": "user"},
                )
                self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))
                imported_id = str((import_resp.result or {}).get("group_id") or "")
                self.assertTrue(imported_id)
                self.assertNotEqual(imported_id, group_id)
        finally:
            cleanup()

    def test_export_file_package_bytes_can_import_through_package_b64(self) -> None:
        from cccc.kernel.group import load_group

        _home, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace_raw, tempfile.TemporaryDirectory() as remap_raw:
                group_id, _scope_key = self._create_group_with_scope(Path(workspace_raw))
                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                group.ledger_path.write_text("hello\n", encoding="utf-8")

                export_resp, _ = self._call("group_copy_export_file", {"group_id": group_id, "by": "user"})
                self.assertTrue(export_resp.ok, getattr(export_resp, "error", None))
                package_path = Path(str((export_resp.result or {}).get("package_path") or ""))
                package_b64 = base64.b64encode(package_path.read_bytes()).decode("ascii")

                import_resp, _ = self._call(
                    "group_copy_import",
                    {"package_b64": package_b64, "workspace_root": remap_raw, "by": "user"},
                )

                self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))
                imported_id = str((import_resp.result or {}).get("group_id") or "")
                self.assertTrue(imported_id)
                self.assertNotEqual(imported_id, group_id)
        finally:
            cleanup()

    def test_export_file_digest_matches_bytes_written_when_source_changes_during_export(self) -> None:
        from cccc.daemon.ops import group_copy_files
        from cccc.daemon.ops import group_copy_ops
        from cccc.kernel.group import load_group

        _home, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace_raw:
                group_id, _scope_key = self._create_group_with_scope(Path(workspace_raw))
                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                group.ledger_path.write_text("before\n", encoding="utf-8")
                output_path = Path(os.environ["CCCC_HOME"]) / "tmp" / "race-export.zip"

                original_write_source_to_zip = group_copy_files._write_source_to_zip

                def append_after_ledger_write(zf, arcname, *, path, data):
                    digest = original_write_source_to_zip(zf, arcname, path=path, data=data)
                    if arcname == "group/ledger.jsonl":
                        group.ledger_path.write_text("before\nafter\n", encoding="utf-8")
                    return digest

                with patch.object(group_copy_files, "_write_source_to_zip", side_effect=append_after_ledger_write):
                    manifest, _filename = group_copy_files.build_package_file(
                        group_id,
                        output_path,
                        group_copy_ops._group_copy_file_deps(),
                    )

                with zipfile.ZipFile(output_path, "r") as zf:
                    entries = {
                        name.removeprefix("group/"): zf.read(name)
                        for name in zf.namelist()
                        if name.startswith("group/") and not name.endswith("/")
                    }
                self.assertEqual(entries["ledger.jsonl"], b"before\n")
                self.assertEqual(manifest["content_digest"], group_copy_ops._content_digest(entries.items()))
        finally:
            cleanup()

    def test_export_file_rejects_package_over_limit_and_removes_zip(self) -> None:
        from cccc.daemon.ops import group_copy_ops

        _home, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace_raw:
                group_id, _scope_key = self._create_group_with_scope(Path(workspace_raw))
                caller_path = Path(os.environ["CCCC_HOME"]) / "tmp" / "caller-owned-export.zip"
                caller_path.parent.mkdir(parents=True, exist_ok=True)
                caller_path.write_text("keep me", encoding="utf-8")

                with patch.object(group_copy_ops, "MAX_PACKAGE_BYTES", 8):
                    export_resp, _ = self._call(
                        "group_copy_export_file",
                        {"group_id": group_id, "output_path": str(caller_path), "by": "user"},
                    )

                self.assertFalse(export_resp.ok)
                self.assertEqual(getattr(export_resp.error, "code", ""), "copy_package_too_large")
                details = getattr(export_resp.error, "details", {}) or {}
                self.assertEqual(details.get("max_bytes"), 8)
                self.assertGreater(int(details.get("actual_bytes") or 0), 8)
                self.assertEqual(caller_path.read_text(encoding="utf-8"), "keep me")
        finally:
            cleanup()

    def test_export_file_does_not_delete_caller_path_when_build_fails(self) -> None:
        from cccc.daemon.ops import group_copy_files
        from cccc.daemon.ops import group_copy_ops

        _home, cleanup = self._with_home()
        try:
            caller_path = Path(os.environ["CCCC_HOME"]) / "tmp" / "caller-owned-partial-export.zip"
            caller_path.parent.mkdir(parents=True, exist_ok=True)
            caller_path.write_text("keep me", encoding="utf-8")
            captured_output_paths: list[Path] = []

            def _write_partial_then_fail(_group_id: str, path: Path, _deps):
                captured_output_paths.append(path)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"partial zip")
                raise RuntimeError("simulated build failure")

            with patch.object(group_copy_ops, "build_package_file", side_effect=_write_partial_then_fail):
                export_resp, _ = self._call(
                    "group_copy_export_file",
                    {"group_id": "g_test", "output_path": str(caller_path), "by": "user"},
                )

            self.assertFalse(export_resp.ok)
            self.assertEqual(getattr(export_resp.error, "code", ""), "group_copy_export_failed")
            self.assertEqual(caller_path.read_text(encoding="utf-8"), "keep me")
            self.assertTrue(captured_output_paths)
            self.assertFalse(captured_output_paths[0].exists())
        finally:
            cleanup()

    def test_import_rejects_cccc_home_as_workspace_root(self) -> None:
        _home, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace_raw:
                group_id, _scope_key = self._create_group_with_scope(Path(workspace_raw))
                package_b64 = base64.b64encode(self._package_bytes(group_id)).decode("ascii")
                import_resp, _ = self._call(
                    "group_copy_import",
                    {
                        "package_b64": package_b64,
                        "workspace_root": os.environ["CCCC_HOME"],
                        "by": "user",
                    },
                )
                self.assertFalse(import_resp.ok)
                self.assertEqual(getattr(import_resp.error, "code", ""), "group_copy_import_failed")
                self.assertIn("not CCCC_HOME", getattr(import_resp.error, "message", ""))
        finally:
            cleanup()

    def test_import_without_conflict_preserves_group_id_in_new_home(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as source_home, tempfile.TemporaryDirectory() as target_home, tempfile.TemporaryDirectory() as workspace_raw:
                os.environ["CCCC_HOME"] = source_home
                group_id, _scope_key = self._create_group_with_scope(Path(workspace_raw))
                package = self._package_bytes(group_id)
                package_b64 = base64.b64encode(package).decode("ascii")

                os.environ["CCCC_HOME"] = target_home
                import_resp, _ = self._call(
                    "group_copy_import",
                    {"package_b64": package_b64, "workspace_root": workspace_raw, "by": "user"},
                )
                self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))
                self.assertEqual(str((import_resp.result or {}).get("group_id") or ""), group_id)
                imported = load_group(group_id)
                self.assertIsNotNone(imported)
                assert imported is not None
                self.assertEqual(str(imported.doc.get("group_id") or ""), group_id)
                self.assertFalse(bool(imported.doc.get("running")))
                self.assertEqual(str(imported.doc.get("state") or ""), "idle")
                reg = load_registry()
                active_scope = str(imported.doc.get("active_scope_key") or "")
                self.assertEqual(reg.defaults.get(active_scope), group_id)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_import_publish_event_failure_does_not_rollback_group_or_registry(self) -> None:
        from unittest.mock import patch

        from cccc.kernel.group import load_group
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as source_home, tempfile.TemporaryDirectory() as target_home, tempfile.TemporaryDirectory() as workspace_raw:
                os.environ["CCCC_HOME"] = source_home
                group_id, _scope_key = self._create_group_with_scope(Path(workspace_raw))
                package = self._package_bytes(group_id)
                package_b64 = base64.b64encode(package).decode("ascii")

                os.environ["CCCC_HOME"] = target_home
                with patch("cccc.kernel.events.publish_event", side_effect=RuntimeError("event bus unavailable")):
                    import_resp, _ = self._call(
                        "group_copy_import",
                        {"package_b64": package_b64, "workspace_root": workspace_raw, "by": "user"},
                    )

                self.assertTrue(import_resp.ok, getattr(import_resp, "error", None))
                imported_group_id = str((import_resp.result or {}).get("group_id") or "")
                self.assertEqual(imported_group_id, group_id)
                self.assertIsNotNone(load_group(imported_group_id))
                self.assertIn(imported_group_id, load_registry().groups)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_preview_actor_count_excludes_internal_assistants(self) -> None:
        from cccc.kernel.group import load_group

        _home, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace_raw:
                group_id, _scope_key = self._create_group_with_scope(Path(workspace_raw))
                add_resp, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "runtime": "codex",
                        "runner": "pty",
                        "by": "user",
                    },
                )
                self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))
                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                doc = yaml.safe_load((group.path / "group.yaml").read_text(encoding="utf-8")) or {}
                actors = doc.get("actors") if isinstance(doc.get("actors"), list) else []
                actors.extend(
                    [
                        {
                            "id": "internal-helper",
                            "runtime": "codex",
                            "runner": "headless",
                            "internal_kind": "legacy",
                            "enabled": True,
                        },
                        {
                            "id": "voice-secretary",
                            "runtime": "codex",
                            "runner": "headless",
                            "internal_kind": "voice_secretary",
                            "enabled": True,
                        },
                    ]
                )
                doc["actors"] = actors
                (group.path / "group.yaml").write_text(yaml.safe_dump(doc, allow_unicode=True, sort_keys=False), encoding="utf-8")

                package_b64 = base64.b64encode(self._package_bytes(group_id)).decode("ascii")
                preview_resp, _ = self._call("group_copy_preview_import", {"package_b64": package_b64})
                self.assertTrue(preview_resp.ok, getattr(preview_resp, "error", None))
                preview = (preview_resp.result or {}).get("preview") or {}
                self.assertEqual(preview.get("actor_count"), 1)
                self.assertEqual([item.get("id") for item in preview.get("actors") or []], ["peer1"])
        finally:
            cleanup()

    def test_rejects_unsafe_zip_paths(self) -> None:
        _home, cleanup = self._with_home()
        try:
            buf = io.BytesIO()
            with zipfile.ZipFile(buf, "w") as zf:
                zf.writestr("manifest.json", json.dumps({
                    "kind": "cccc.group_copy",
                    "version": 1,
                    "export_mode": "group_state_only",
                    "workspace_included": False,
                    "contains_secrets": False,
                }))
                zf.writestr("../evil.txt", "bad")
            package_b64 = base64.b64encode(buf.getvalue()).decode("ascii")
            preview_resp, _ = self._call("group_copy_preview_import", {"package_b64": package_b64})
            self.assertFalse(preview_resp.ok)
            self.assertEqual(preview_resp.error.code if preview_resp.error else "", "invalid_group_copy")
        finally:
            cleanup()

    def test_rejects_symlink_case_collision_and_windows_reserved_zip_paths(self) -> None:
        _home, cleanup = self._with_home()
        try:
            manifest = {
                "kind": "cccc.group_copy",
                "version": 1,
                "export_mode": "group_state_only",
                "workspace_included": False,
                "contains_secrets": False,
            }

            def preview_for(entries: list[tuple[str, bytes, int]]) -> tuple[bool, str]:
                buf = io.BytesIO()
                with zipfile.ZipFile(buf, "w") as zf:
                    zf.writestr("manifest.json", json.dumps(manifest))
                    zf.writestr("group/group.yaml", "group_id: g_test\n")
                    for name, data, external_attr in entries:
                        info = zipfile.ZipInfo(name)
                        info.external_attr = external_attr
                        zf.writestr(info, data)
                package_b64 = base64.b64encode(buf.getvalue()).decode("ascii")
                resp, _ = self._call("group_copy_preview_import", {"package_b64": package_b64})
                return bool(resp.ok), resp.error.message if resp.error else ""

            symlink_attr = (0o120777 << 16)
            ok, message = preview_for([("group/link", b"target", symlink_attr)])
            self.assertFalse(ok)
            self.assertIn("symlink", message)

            ok, message = preview_for([("group/State/file.txt", b"a", 0), ("group/state/file.txt", b"b", 0)])
            self.assertFalse(ok)
            self.assertIn("case-insensitive", message)

            ok, message = preview_for([("group/CON", b"bad", 0)])
            self.assertFalse(ok)
            self.assertIn("windows reserved", message)
        finally:
            cleanup()
