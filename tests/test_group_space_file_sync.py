import json
import os
import tempfile
import unittest
import hashlib
import shutil
from pathlib import Path
from unittest.mock import patch


class TestGroupSpaceFileSync(unittest.TestCase):
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

    def _with_env(self, key: str, value: str | None):
        old = os.environ.get(key)
        if value is None:
            os.environ.pop(key, None)
        else:
            os.environ[key] = value

        def cleanup() -> None:
            if old is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = old

        return cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group(self, title: str = "space-sync") -> str:
        resp, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        gid = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _attach(self, group_id: str, project_dir: Path) -> None:
        resp, _ = self._call("attach", {"path": str(project_dir), "group_id": group_id, "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))

    def _bind(self, group_id: str, remote_space_id: str = "nb_sync_test") -> None:
        resp, _ = self._call(
            "group_space_bind",
            {
                "group_id": group_id,
                "provider": "notebooklm",
                "action": "bind",
                "remote_space_id": remote_space_id,
                "by": "user",
            },
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))

    def _add_actor(self, group_id: str, actor_id: str, *, runner: str = "headless") -> None:
        resp, _ = self._call(
            "actor_add",
            {
                "group_id": group_id,
                "actor_id": actor_id,
                "runtime": "codex",
                "runner": runner,
                "by": "user",
            },
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))

    def _find_descriptor_by_source_id(self, space_dir: Path, source_id: str) -> Path | None:
        sources_dir = space_dir / "sources"
        if not sources_dir.exists():
            return None
        for path in sorted(sources_dir.glob("*.source.json")):
            try:
                doc = json.loads(path.read_text(encoding="utf-8"))
            except Exception:
                continue
            if str(doc.get("source_id") or "").strip() == str(source_id or "").strip():
                return path
        return None

    def _find_preview_by_descriptor(self, space_dir: Path, descriptor_path: Path) -> Path | None:
        name = descriptor_path.name
        if not name.endswith(".source.json"):
            return None
        stem = name[: -len(".source.json")]
        preview_dir = space_dir / ".sync" / "source-text"
        if not preview_dir.exists():
            return None
        for candidate in sorted(preview_dir.glob(f"{stem}.*")):
            if candidate.is_file():
                return candidate
        return None

    def test_sync_uploads_local_space_files_and_writes_index(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-upload")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_upload")

            space_dir = project_dir / "space"
            space_dir.mkdir(parents=True, exist_ok=True)
            (space_dir / "a.txt").write_text("alpha", encoding="utf-8")
            (space_dir / "b.txt").write_text("beta", encoding="utf-8")

            remote: dict[str, dict] = {}
            counter = {"n": 0}

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": list(remote.values())}

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id
                counter["n"] += 1
                sid = f"src_{counter['n']}"
                remote[sid] = {
                    "source_id": sid,
                    "title": Path(file_path).name,
                    "kind": "pasted_text",
                    "status": 2,
                    "url": "",
                }
                return dict(remote[sid])

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id
                item = remote.get(source_id) or {"source_id": source_id}
                item["title"] = new_title
                remote[source_id] = item
                return dict(item)

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id
                remote.pop(source_id, None)
                return {"deleted": True}

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}):
                result = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(result.get("ok")))
            self.assertEqual(bool(result.get("converged")), True)
            self.assertEqual(int(result.get("uploaded") or 0), 2)
            self.assertEqual(int(result.get("unsynced_count") or 0), 0)

            index_path = space_dir / ".space-index.json"
            self.assertTrue(index_path.exists())
            doc = json.loads(index_path.read_text(encoding="utf-8"))
            entries = doc.get("entries") if isinstance(doc.get("entries"), dict) else {}
            self.assertEqual(set(entries.keys()), {"a.txt", "b.txt"})

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}):
                second = sync_group_space_files(gid, provider="notebooklm", force=False)
            self.assertTrue(bool(second.get("ok")))
            self.assertEqual(bool(second.get("skipped")), True)
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_sync_unsupported_local_format_fails_fast_with_clear_error(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-unsupported-format")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_unsupported")

            space_dir = project_dir / "space"
            space_dir.mkdir(parents=True, exist_ok=True)
            (space_dir / "script.py").write_text("print('x')\n", encoding="utf-8")

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": []}

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id, file_path
                raise AssertionError("unsupported local file should not be uploaded")

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id, source_id, new_title
                return {"source_id": source_id, "title": new_title}

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {"deleted": True}

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}):
                result = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(result.get("ok")))
            self.assertEqual(bool(result.get("converged")), False)
            self.assertEqual(int(result.get("unsynced_count") or 0), 1)
            self.assertEqual(int(result.get("failed_count") or 0), 1)
            failed_items = result.get("failed_items") if isinstance(result.get("failed_items"), list) else []
            self.assertEqual(len(failed_items), 1)
            row = failed_items[0] if failed_items else {}
            self.assertEqual(str(row.get("rel_path") or ""), "script.py")
            self.assertEqual(str(row.get("code") or ""), "space_source_unsupported_format")

            state_doc = json.loads((space_dir / ".space-sync-state.json").read_text(encoding="utf-8"))
            state_failed = state_doc.get("failed_items") if isinstance(state_doc.get("failed_items"), list) else []
            self.assertEqual(len(state_failed), 1)
            self.assertEqual(str((state_failed[0] or {}).get("code") or ""), "space_source_unsupported_format")
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_sync_oversize_local_file_fails_fast_with_clear_error(self) -> None:
        _, cleanup = self._with_home()
        cleanup_limit = self._with_env("CCCC_SPACE_LOCAL_FILE_MAX_BYTES", "8")
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-oversize-format")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_oversize")

            space_dir = project_dir / "space"
            space_dir.mkdir(parents=True, exist_ok=True)
            (space_dir / "big.md").write_text("0123456789abcdef", encoding="utf-8")

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": []}

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id, file_path
                raise AssertionError("oversized local file should not be uploaded")

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id, source_id, new_title
                return {"source_id": source_id, "title": new_title}

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {"deleted": True}

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}):
                result = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(result.get("ok")))
            self.assertEqual(bool(result.get("converged")), False)
            failed_items = result.get("failed_items") if isinstance(result.get("failed_items"), list) else []
            self.assertEqual(len(failed_items), 1)
            row = failed_items[0] if failed_items else {}
            self.assertEqual(str(row.get("rel_path") or ""), "big.md")
            self.assertEqual(str(row.get("code") or ""), "space_source_file_too_large")
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup_limit()
            cleanup()

    def test_sync_deletes_remote_for_local_removed_file(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-delete")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_delete")

            space_dir = project_dir / "space"
            space_dir.mkdir(parents=True, exist_ok=True)
            target = space_dir / "remove_me.txt"
            target.write_text("to-delete", encoding="utf-8")

            remote: dict[str, dict] = {}
            counter = {"n": 0}

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": list(remote.values())}

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id
                counter["n"] += 1
                sid = f"src_{counter['n']}"
                remote[sid] = {"source_id": sid, "title": Path(file_path).name}
                return {"source_id": sid, "title": Path(file_path).name}

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id
                item = remote.get(source_id) or {"source_id": source_id}
                item["title"] = new_title
                remote[source_id] = item
                return dict(item)

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id
                remote.pop(source_id, None)
                return {"deleted": True}

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}):
                first = sync_group_space_files(gid, provider="notebooklm", force=True)
            self.assertTrue(bool(first.get("ok")))
            self.assertEqual(len(remote), 1)

            target.unlink(missing_ok=True)

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}):
                second = sync_group_space_files(gid, provider="notebooklm", force=True)
            self.assertTrue(bool(second.get("ok")))
            self.assertEqual(bool(second.get("converged")), True)
            self.assertGreaterEqual(int(second.get("deleted") or 0), 1)
            self.assertEqual(len(remote), 0)
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_sync_rehydrate_after_space_root_removed_does_not_delete_remote(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-rehydrate-after-root-reset")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_rehydrate")

            space_dir = project_dir / "space"
            space_dir.mkdir(parents=True, exist_ok=True)
            (space_dir / "keep.txt").write_text("keep-remote", encoding="utf-8")

            remote: dict[str, dict] = {}
            counter = {"n": 0}

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": list(remote.values())}

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id
                counter["n"] += 1
                sid = f"src_{counter['n']}"
                remote[sid] = {"source_id": sid, "title": Path(file_path).name}
                return {"source_id": sid, "title": Path(file_path).name}

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id
                item = remote.get(source_id) or {"source_id": source_id}
                item["title"] = new_title
                remote[source_id] = item
                return dict(item)

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id
                remote.pop(source_id, None)
                return {"deleted": True}

            def _get_source_fulltext(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {
                    "source_id": source_id,
                    "title": "keep.txt",
                    "kind": "pasted_text",
                    "url": "",
                    "content": "keep-remote",
                    "char_count": 11,
                }

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_get_source_fulltext", side_effect=_get_source_fulltext), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}):
                first = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(first.get("ok")))
            self.assertEqual(len(remote), 1)
            source_id = next(iter(remote.keys()))

            shutil.rmtree(space_dir)
            self.assertFalse(space_dir.exists())

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_get_source_fulltext", side_effect=_get_source_fulltext), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}):
                second = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(second.get("ok")))
            self.assertEqual(bool(second.get("converged")), True)
            self.assertEqual(int(second.get("deleted") or 0), 0)
            self.assertEqual(len(remote), 1)
            desc = self._find_descriptor_by_source_id(project_dir / "space", source_id)
            self.assertIsNotNone(desc)
            desc = desc or ((project_dir / "space") / "sources" / "missing.source.json")
            self.assertTrue(desc.exists())
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_sync_materializes_remote_sources_and_artifacts(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-materialize-remote")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_remote")

            space_dir = project_dir / "space"
            space_dir.mkdir(parents=True, exist_ok=True)

            remote_sources = [
                {
                    "source_id": "src_remote_1",
                    "title": "Remote Source One",
                    "kind": "web_page",
                    "status": 2,
                    "url": "https://example.com/a",
                }
            ]
            remote_artifacts = [
                {
                    "artifact_id": "art_remote_1",
                    "title": "Remote Infographic",
                    "kind": "ArtifactType.INFOGRAPHIC",
                    "status": "completed",
                    "created_at": "2026-02-23T00:00:00Z",
                    "url": "https://example.com/artifact",
                }
            ]

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": list(remote_sources)}

            def _list_artifacts(provider: str, *, remote_space_id: str, kind: str = ""):
                _ = provider, remote_space_id, kind
                return {"artifacts": list(remote_artifacts)}

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id, file_path
                return {"source_id": "src_added", "title": "added"}

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id, source_id, new_title
                return {"source_id": source_id, "title": new_title}

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {"deleted": True}

            def _download_artifact(
                provider: str,
                *,
                remote_space_id: str,
                kind: str,
                output_path: str,
                artifact_id: str = "",
                output_format: str = "",
            ):
                _ = provider, remote_space_id, kind, artifact_id, output_format
                Path(output_path).parent.mkdir(parents=True, exist_ok=True)
                Path(output_path).write_text("artifact-bytes", encoding="utf-8")
                return {"output_path": output_path, "downloaded": True}

            def _get_source_fulltext(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {
                    "source_id": "src_remote_1",
                    "title": "Remote Source One",
                    "kind": "web_page",
                    "url": "https://example.com/a",
                    "content": "Remote source body",
                    "char_count": 18,
                }

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_get_source_fulltext", side_effect=_get_source_fulltext), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", side_effect=_list_artifacts), \
                 patch("cccc.daemon.space.group_space_sync.provider_download_artifact", side_effect=_download_artifact):
                result = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(result.get("ok")))
            self.assertEqual(int(result.get("remote_sources") or 0), 1)
            self.assertGreaterEqual(int(result.get("materialized_sources") or 0), 1)
            self.assertEqual(int(result.get("remote_artifacts") or 0), 1)
            self.assertEqual(int(result.get("downloaded_artifacts") or 0), 1)

            remote_source_snapshot = space_dir / ".sync" / "remote-sources" / "src_remote_1.json"
            self.assertTrue(remote_source_snapshot.exists())
            remote_source_descriptor = self._find_descriptor_by_source_id(space_dir, "src_remote_1")
            self.assertIsNotNone(remote_source_descriptor)
            remote_source_descriptor = remote_source_descriptor or (space_dir / "sources" / "missing.source.json")
            self.assertTrue(remote_source_descriptor.exists())
            desc = json.loads(remote_source_descriptor.read_text(encoding="utf-8"))
            self.assertEqual(str(desc.get("source_id") or ""), "src_remote_1")
            self.assertEqual(str(desc.get("type") or ""), "web_page")
            self.assertEqual(str(desc.get("url") or ""), "https://example.com/a")
            remote_source_preview = self._find_preview_by_descriptor(space_dir, remote_source_descriptor)
            self.assertIsNotNone(remote_source_preview)
            remote_source_preview = remote_source_preview or (space_dir / ".sync" / "source-text" / "missing.txt")
            self.assertTrue(remote_source_preview.exists())
            self.assertIn("Remote source body", remote_source_preview.read_text(encoding="utf-8"))

            index_doc = json.loads((space_dir / ".space-index.json").read_text(encoding="utf-8"))
            entries = index_doc.get("entries") if isinstance(index_doc, dict) else {}
            rel_descriptor = f"sources/{remote_source_descriptor.name}"
            source_entry = entries.get(rel_descriptor) if isinstance(entries, dict) else None
            self.assertTrue(isinstance(source_entry, dict))
            self.assertEqual(str((source_entry or {}).get("source_id") or ""), "src_remote_1")

            artifact_path = space_dir / "artifacts" / "notebooklm" / "infographic" / "art_remote_1.png"
            self.assertTrue(artifact_path.exists())
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_sync_reports_materialize_fulltext_failures(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-materialize-fulltext-failure")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_remote_fail")

            space_dir = project_dir / "space"
            space_dir.mkdir(parents=True, exist_ok=True)

            remote_sources = [
                {
                    "source_id": "src_remote_fail_1",
                    "title": "Remote Source Failure",
                    "kind": "web_page",
                    "status": 2,
                    "url": "https://example.com/fail",
                }
            ]

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": list(remote_sources)}

            def _list_artifacts(provider: str, *, remote_space_id: str, kind: str = ""):
                _ = provider, remote_space_id, kind
                return {"artifacts": []}

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id, file_path
                return {"source_id": "src_added", "title": "added"}

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id, source_id, new_title
                return {"source_id": source_id, "title": new_title}

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {"deleted": True}

            from cccc.daemon.space.group_space_provider import SpaceProviderError
            from cccc.daemon.space.group_space_sync import sync_group_space_files

            def _get_source_fulltext(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                raise SpaceProviderError("space_provider_upstream_error", "fulltext fetch failed")

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_get_source_fulltext", side_effect=_get_source_fulltext), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", side_effect=_list_artifacts):
                result = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(result.get("ok")))
            self.assertEqual(bool(result.get("converged")), False)
            failed_items = result.get("failed_items") if isinstance(result.get("failed_items"), list) else []
            self.assertTrue(any(str(item.get("code") or "") == "space_provider_upstream_error" for item in failed_items if isinstance(item, dict)))
            desc = self._find_descriptor_by_source_id(space_dir, "src_remote_fail_1")
            self.assertIsNotNone(desc)
            desc = desc or (space_dir / "sources" / "missing.source.json")
            self.assertTrue(desc.exists())
            preview = self._find_preview_by_descriptor(space_dir, desc)
            self.assertIsNotNone(preview)
            preview = preview or (space_dir / ".sync" / "source-text" / "missing.txt")
            self.assertTrue(preview.exists())
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_sync_materializes_remote_image_as_descriptor_not_txt_source(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-materialize-image")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_image")

            space_dir = project_dir / "space"
            space_dir.mkdir(parents=True, exist_ok=True)

            remote_sources = [
                {
                    "source_id": "src_img_1",
                    "title": "Remote Image",
                    "kind": "image",
                    "status": 2,
                    "url": "https://example.com/image.png",
                }
            ]

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": list(remote_sources)}

            def _list_artifacts(provider: str, *, remote_space_id: str, kind: str = ""):
                _ = provider, remote_space_id, kind
                return {"artifacts": []}

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id, file_path
                raise AssertionError("unexpected add_file for remote-only image materialization")

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id, source_id, new_title
                return {"source_id": source_id, "title": new_title}

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {"deleted": True}

            def _get_source_fulltext(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {
                    "source_id": "src_img_1",
                    "title": "Remote Image",
                    "kind": "image",
                    "url": "https://example.com/image.png",
                    "content": "",
                    "char_count": 0,
                }

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_get_source_fulltext", side_effect=_get_source_fulltext), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", side_effect=_list_artifacts):
                result = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(result.get("ok")))
            desc = self._find_descriptor_by_source_id(space_dir, "src_img_1")
            self.assertIsNotNone(desc)
            desc = desc or (space_dir / "sources" / "missing.source.json")
            self.assertTrue(desc.exists())
            self.assertFalse((space_dir / "sources" / "src_img_1.txt").exists())
            preview = self._find_preview_by_descriptor(space_dir, desc)
            self.assertIsNotNone(preview)
            preview = preview or (space_dir / ".sync" / "source-text" / "missing.txt")
            self.assertTrue(preview.exists())
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_sync_remote_managed_local_edit_is_not_reuploaded(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-remote-edit-guard")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_remote_guard")

            space_dir = project_dir / "space"
            (space_dir / "sources").mkdir(parents=True, exist_ok=True)
            local_path = space_dir / "sources" / "src_remote_guard.source.json"
            local_path.write_text(
                json.dumps(
                    {
                        "v": 1,
                        "provider": "notebooklm",
                        "remote_space_id": "nb_sync_remote_guard",
                        "source_id": "src_remote_guard",
                        "type": "web_page",
                        "title": "Local Edited",
                        "url": "https://example.com/local-edited",
                        "status": "2",
                        "mode": "remote_mirror",
                        "read_only": True,
                        "synced_at": "2026-02-22T00:00:00Z",
                    }
                ),
                encoding="utf-8",
            )
            st = local_path.stat()
            old_content = json.dumps(
                {
                    "v": 1,
                    "provider": "notebooklm",
                    "remote_space_id": "nb_sync_remote_guard",
                    "source_id": "src_remote_guard",
                    "type": "web_page",
                    "title": "Remote Guard Source",
                    "url": "https://example.com/guard",
                    "status": "2",
                    "mode": "remote_mirror",
                    "read_only": True,
                    "synced_at": "2026-02-20T00:00:00Z",
                },
                indent=2,
            )
            old_sha = hashlib.sha256(old_content.encode("utf-8")).hexdigest()

            (space_dir / ".space-index.json").write_text(
                json.dumps(
                    {
                        "v": 1,
                        "group_id": gid,
                        "provider": "notebooklm",
                        "remote_space_id": "nb_sync_remote_guard",
                        "entries": {
                            "sources/src_remote_guard.source.json": {
                                "rel_path": "sources/src_remote_guard.source.json",
                                "path_hash": hashlib.sha256("sources/src_remote_guard.source.json".encode("utf-8")).hexdigest()[:24],
                                "sha256": old_sha,
                                "size": len(old_content.encode("utf-8")),
                                "mtime_ns": int(getattr(st, "st_mtime_ns", int(st.st_mtime * 1_000_000_000))),
                                "source_id": "src_remote_guard",
                                "remote_title": "Remote Guard Source",
                                "last_synced_at": "2026-02-22T00:00:00Z",
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )

            remote_sources = [
                {
                    "source_id": "src_remote_guard",
                    "title": "Remote Guard Source",
                    "kind": "web_page",
                    "status": 2,
                    "url": "https://example.com/guard",
                }
            ]

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": list(remote_sources)}

            def _list_artifacts(provider: str, *, remote_space_id: str, kind: str = ""):
                _ = provider, remote_space_id, kind
                return {"artifacts": []}

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id, file_path
                raise AssertionError("remote-managed edited mirror must not be re-uploaded as file source")

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id, source_id, new_title
                return {"source_id": source_id, "title": new_title}

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {"deleted": True}

            def _get_source_fulltext(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {
                    "source_id": "src_remote_guard",
                    "title": "Remote Guard Source",
                    "kind": "web_page",
                    "url": "https://example.com/guard",
                    "content": "remote-canonical\n",
                    "char_count": 16,
                }

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_get_source_fulltext", side_effect=_get_source_fulltext), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", side_effect=_list_artifacts):
                result = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(result.get("ok")))
            migrated_descriptor = self._find_descriptor_by_source_id(space_dir, "src_remote_guard")
            self.assertIsNotNone(migrated_descriptor)
            migrated_descriptor = migrated_descriptor or local_path
            local_doc = json.loads(migrated_descriptor.read_text(encoding="utf-8"))
            self.assertEqual(str(local_doc.get("source_id") or ""), "src_remote_guard")
            self.assertEqual(str(local_doc.get("title") or ""), "Remote Guard Source")
            self.assertEqual(str(local_doc.get("url") or ""), "https://example.com/guard")
            self.assertEqual(str(local_doc.get("type") or ""), "web_page")
            preview_path = self._find_preview_by_descriptor(space_dir, migrated_descriptor)
            self.assertIsNotNone(preview_path)
            preview_path = preview_path or (space_dir / ".sync" / "source-text" / "missing.txt")
            self.assertTrue(preview_path.exists())
            self.assertEqual(preview_path.read_text(encoding="utf-8"), "remote-canonical\n")
            index_doc = json.loads((space_dir / ".space-index.json").read_text(encoding="utf-8"))
            entries = index_doc.get("entries") if isinstance(index_doc, dict) else {}
            row = entries.get(f"sources/{migrated_descriptor.name}") if isinstance(entries, dict) else {}
            self.assertEqual(str((row or {}).get("source_id") or ""), "src_remote_guard")
            failed_items = result.get("failed_items") if isinstance(result.get("failed_items"), list) else []
            self.assertTrue(any(str(item.get("code") or "") == "space_remote_source_read_only" for item in failed_items if isinstance(item, dict)))
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_sync_failure_state_and_edge_notifications(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-notify")
            self._attach(gid, project_dir)
            self._add_actor(gid, "lead")
            self._add_actor(gid, "peer1")
            self._bind(gid, "nb_sync_notify")

            space_dir = project_dir / "space"
            space_dir.mkdir(parents=True, exist_ok=True)
            (space_dir / "a.txt").write_text("alpha", encoding="utf-8")

            remote: dict[str, dict] = {}

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {"sources": list(remote.values())}

            def _add_file_fail(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id, file_path
                from cccc.daemon.space.group_space_provider import SpaceProviderError

                raise SpaceProviderError("space_provider_upstream_error", "upstream add failed")

            def _add_file_ok(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id
                sid = "src_ok_1"
                remote[sid] = {"source_id": sid, "title": Path(file_path).name}
                return {"source_id": sid, "title": Path(file_path).name}

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id
                row = remote.get(source_id) or {"source_id": source_id}
                row["title"] = new_title
                remote[source_id] = row
                return dict(row)

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                remote.pop(source_id, None)
                return {"deleted": True}

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file_fail), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}), \
                 patch("cccc.daemon.space.group_space_sync.append_event") as append_event_mock:
                failed = sync_group_space_files(gid, provider="notebooklm", force=True, by="peer1")

            self.assertTrue(bool(failed.get("ok")))
            self.assertEqual(bool(failed.get("converged")), False)
            self.assertGreaterEqual(int(failed.get("failed_count") or 0), 1)
            failed_items = failed.get("failed_items") if isinstance(failed.get("failed_items"), list) else []
            self.assertTrue(any(str(item.get("rel_path") or "") == "a.txt" for item in failed_items if isinstance(item, dict)))

            targets = {
                str((call.kwargs.get("data") or {}).get("target_actor_id") or "")
                for call in append_event_mock.call_args_list
            }
            self.assertEqual(targets, {"lead", "peer1"})
            self.assertTrue(
                all(
                    str((call.kwargs.get("data") or {}).get("kind") or "") == "error"
                    for call in append_event_mock.call_args_list
                )
            )

            state_path = space_dir / ".space-sync-state.json"
            self.assertTrue(state_path.exists())
            state_doc = json.loads(state_path.read_text(encoding="utf-8"))
            self.assertEqual(str(state_doc.get("state") or ""), "error")
            self.assertGreaterEqual(int(state_doc.get("failed_count") or 0), 1)

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file_ok), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}), \
                 patch("cccc.daemon.space.group_space_sync.append_event") as append_event_mock:
                recovered = sync_group_space_files(gid, provider="notebooklm", force=True, by="peer1")

            self.assertTrue(bool(recovered.get("ok")))
            self.assertEqual(bool(recovered.get("converged")), True)
            self.assertEqual(int(recovered.get("failed_count") or 0), 0)
            targets = {
                str((call.kwargs.get("data") or {}).get("target_actor_id") or "")
                for call in append_event_mock.call_args_list
            }
            self.assertEqual(targets, {"lead", "peer1"})
            self.assertTrue(
                all(
                    str((call.kwargs.get("data") or {}).get("kind") or "") == "status_change"
                    for call in append_event_mock.call_args_list
                )
            )
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_sync_migrates_legacy_source_layout(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-sync-legacy-migrate")
            self._attach(gid, project_dir)
            self._bind(gid, "nb_sync_legacy")

            space_dir = project_dir / "space"
            legacy_dir = space_dir / "sources" / "notebooklm"
            legacy_dir.mkdir(parents=True, exist_ok=True)
            legacy_file = legacy_dir / "src_legacy_1.txt"
            legacy_file.write_text("legacy-body\n", encoding="utf-8")
            st = legacy_file.stat()
            rel_legacy = "sources/notebooklm/src_legacy_1.txt"
            sha = hashlib.sha256(legacy_file.read_bytes()).hexdigest()
            (space_dir / ".space-index.json").write_text(
                json.dumps(
                    {
                        "v": 1,
                        "group_id": gid,
                        "provider": "notebooklm",
                        "remote_space_id": "nb_sync_legacy",
                        "entries": {
                            rel_legacy: {
                                "rel_path": rel_legacy,
                                "path_hash": "legacy_hash",
                                "sha256": sha,
                                "size": int(st.st_size),
                                "mtime_ns": int(getattr(st, "st_mtime_ns", int(st.st_mtime * 1_000_000_000))),
                                "source_id": "src_legacy_1",
                                "remote_title": "Legacy remote source",
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )

            def _list_sources(provider: str, *, remote_space_id: str):
                _ = provider, remote_space_id
                return {
                    "sources": [
                        {
                            "source_id": "src_legacy_1",
                            "title": "Legacy remote source",
                            "kind": "web_page",
                            "status": 1,
                        }
                    ]
                }

            def _add_file(provider: str, *, remote_space_id: str, file_path: str):
                _ = provider, remote_space_id, file_path
                raise AssertionError("unexpected add_file during legacy migration")

            def _rename(provider: str, *, remote_space_id: str, source_id: str, new_title: str):
                _ = provider, remote_space_id, source_id, new_title
                return {"source_id": source_id, "title": new_title}

            def _delete(provider: str, *, remote_space_id: str, source_id: str):
                _ = provider, remote_space_id, source_id
                return {"deleted": True}

            from cccc.daemon.space.group_space_sync import sync_group_space_files

            with patch("cccc.daemon.space.group_space_sync.provider_list_sources", side_effect=_list_sources), \
                 patch("cccc.daemon.space.group_space_sync.provider_add_file_source", side_effect=_add_file), \
                 patch("cccc.daemon.space.group_space_sync.provider_rename_source", side_effect=_rename), \
                 patch("cccc.daemon.space.group_space_sync.provider_delete_source", side_effect=_delete), \
                 patch("cccc.daemon.space.group_space_sync.provider_list_artifacts", return_value={"artifacts": []}):
                result = sync_group_space_files(gid, provider="notebooklm", force=True)

            self.assertTrue(bool(result.get("ok")))
            self.assertEqual(bool(result.get("converged")), True)
            self.assertFalse((space_dir / "sources" / "notebooklm" / "src_legacy_1.txt").exists())
            migrated_path = self._find_descriptor_by_source_id(space_dir, "src_legacy_1")
            self.assertIsNotNone(migrated_path)
            migrated_path = migrated_path or (space_dir / "sources" / "missing.source.json")
            self.assertTrue(migrated_path.exists())

            doc = json.loads((space_dir / ".space-index.json").read_text(encoding="utf-8"))
            entries = doc.get("entries") if isinstance(doc, dict) else {}
            self.assertTrue(isinstance(entries, dict))
            self.assertNotIn("sources/notebooklm/src_legacy_1.txt", entries)
            row = entries.get(f"sources/{migrated_path.name}") if isinstance(entries, dict) else None
            self.assertTrue(isinstance(row, dict))
            self.assertEqual(str((row or {}).get("source_id") or ""), "src_legacy_1")
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()


if __name__ == "__main__":
    unittest.main()
