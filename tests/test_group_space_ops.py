from __future__ import annotations

import asyncio
import json
import os
import shutil
import sys
import tempfile
import threading
import time
import types
import unittest
from pathlib import Path
from typing import Any, Dict
from unittest.mock import patch


class TestGroupSpaceOps(unittest.TestCase):
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

    def _create_group(self, title: str = "space-test") -> str:
        create, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        gid = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _add_actor(self, group_id: str, actor_id: str, *, by: str = "user") -> None:
        add, _ = self._call(
            "actor_add",
            {
                "group_id": group_id,
                "actor_id": actor_id,
                "runtime": "codex",
                "runner": "headless",
                "by": by,
            },
        )
        self.assertTrue(add.ok, getattr(add, "error", None))

    def _attach_scope(self, group_id: str, path: str) -> None:
        attach, _ = self._call("attach", {"group_id": group_id, "path": path, "by": "user"})
        self.assertTrue(attach.ok, getattr(attach, "error", None))

    def test_status_defaults_to_unbound(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            status, _ = self._call("group_space_status", {"group_id": gid})
            self.assertTrue(status.ok, getattr(status, "error", None))
            result = status.result if isinstance(status.result, dict) else {}
            provider = result.get("provider") if isinstance(result.get("provider"), dict) else {}
            binding = (((result.get("bindings") or {}).get("work")) if isinstance(result.get("bindings"), dict) else {})
            summary = (((result.get("queue_summary") or {}).get("work")) if isinstance(result.get("queue_summary"), dict) else {})
            self.assertEqual(str(provider.get("provider") or ""), "notebooklm")
            self.assertEqual(str(provider.get("mode") or ""), "disabled")
            self.assertEqual(bool(provider.get("enabled")), False)
            self.assertEqual(bool(provider.get("write_ready")), False)
            self.assertEqual(str(provider.get("readiness_reason") or ""), "real_disabled_and_stub_disabled")
            self.assertEqual(str(binding.get("status") or ""), "unbound")
            self.assertEqual(int(summary.get("pending") or 0), 0)
        finally:
            cleanup()

    def test_status_reports_stub_readiness(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-status-ready")
            status, _ = self._call("group_space_status", {"group_id": gid})
            self.assertTrue(status.ok, getattr(status, "error", None))
            result = status.result if isinstance(status.result, dict) else {}
            provider = result.get("provider") if isinstance(result.get("provider"), dict) else {}
            self.assertEqual(bool(provider.get("stub_adapter_enabled")), True)
            self.assertEqual(bool(provider.get("real_adapter_enabled")), False)
            self.assertEqual(bool(provider.get("write_ready")), True)
            self.assertEqual(str(provider.get("readiness_reason") or ""), "ok")
        finally:
            cleanup_stub()
            cleanup()

    def test_work_sync_status_hides_stale_remote_after_unbind(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            from cccc.daemon.space.group_space_paths import resolve_space_root, space_state_path

            gid = self._create_group("space-work-unbind-status")
            self._attach_scope(gid, str(project_dir))

            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_work_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            space_root = resolve_space_root(gid, create=False)
            self.assertIsNotNone(space_root)
            state_path = space_state_path(space_root or project_dir)
            state_path.write_text(
                json.dumps(
                    {
                        "v": 1,
                        "group_id": gid,
                        "provider": "notebooklm",
                        "remote_space_id": "nb_work_1",
                        "last_run_at": "2026-03-13T00:00:00Z",
                        "converged": True,
                        "unsynced_count": 0,
                        "failed_count": 0,
                        "uploaded": 2,
                        "updated": 1,
                        "deleted": 0,
                        "reused": 4,
                        "remote_sources": 7,
                        "materialized_sources": 7,
                        "last_error": "",
                    },
                    ensure_ascii=False,
                ),
                encoding="utf-8",
            )

            unbind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "unbind",
                    "by": "user",
                },
            )
            self.assertTrue(unbind.ok, getattr(unbind, "error", None))
            unbind_result = unbind.result if isinstance(unbind.result, dict) else {}
            unbind_sync = unbind_result.get("sync") if isinstance(unbind_result.get("sync"), dict) else {}
            self.assertEqual(str(unbind_sync.get("remote_space_id") or ""), "")
            self.assertEqual(str(unbind_sync.get("reason") or ""), "work_lane_unbound")
            self.assertEqual(int(unbind_sync.get("remote_sources", -1)), 0)
            self.assertEqual(str(unbind_sync.get("last_run_at") or ""), "")

            status, _ = self._call("group_space_status", {"group_id": gid})
            self.assertTrue(status.ok, getattr(status, "error", None))
            status_result = status.result if isinstance(status.result, dict) else {}
            status_sync = status_result.get("sync") if isinstance(status_result.get("sync"), dict) else {}
            self.assertEqual(str(status_sync.get("remote_space_id") or ""), "")
            self.assertEqual(str(status_sync.get("reason") or ""), "work_lane_unbound")
            self.assertEqual(int(status_sync.get("remote_sources", -1)), 0)
            self.assertEqual(str(status_sync.get("last_run_at") or ""), "")

            sync_status, _ = self._call(
                "group_space_sync",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "status",
                },
            )
            self.assertTrue(sync_status.ok, getattr(sync_status, "error", None))
            sync_result = sync_status.result if isinstance(sync_status.result, dict) else {}
            sync_payload = sync_result.get("sync") if isinstance(sync_result.get("sync"), dict) else {}
            self.assertEqual(str(sync_payload.get("remote_space_id") or ""), "")
            self.assertEqual(str(sync_payload.get("reason") or ""), "work_lane_unbound")
            self.assertEqual(int(sync_payload.get("remote_sources", -1)), 0)
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_status_hides_stale_work_queue_after_rebind(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.space.group_space_store import enqueue_space_job, mark_space_job_failed

            gid = self._create_group("space-status-rebind-queue")
            bind_old, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_status_old",
                    "by": "user",
                },
            )
            self.assertTrue(bind_old.ok, getattr(bind_old, "error", None))

            failed_job, _ = enqueue_space_job(
                group_id=gid,
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_status_old",
                kind="context_sync",
                payload={"summary": {"tasks": []}},
                idempotency_key="status-old-failed",
            )
            mark_space_job_failed(str(failed_job.get("job_id") or ""), code="space_failed", message="old notebook failed")

            bind_new, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_status_new",
                    "by": "user",
                },
            )
            self.assertTrue(bind_new.ok, getattr(bind_new, "error", None))

            status, _ = self._call("group_space_status", {"group_id": gid})
            self.assertTrue(status.ok, getattr(status, "error", None))
            result = status.result if isinstance(status.result, dict) else {}
            summary = (((result.get("queue_summary") or {}).get("work")) if isinstance(result.get("queue_summary"), dict) else {})
            self.assertEqual(int(summary.get("pending") or 0), 0)
            self.assertEqual(int(summary.get("running") or 0), 0)
            self.assertEqual(int(summary.get("failed") or 0), 0)
        finally:
            cleanup()

    def test_memory_sync_status_hides_stale_summary_after_unbind(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.memory_reme.layout import resolve_memory_layout

            gid = self._create_group("space-memory-unbind-status")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "bind",
                    "remote_space_id": "nb_memory_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            layout = resolve_memory_layout(gid, ensure_files=True)
            manifest_path = layout.memory_root / "notebooklm_sync.json"
            manifest_path.write_text(
                json.dumps(
                    {
                        "v": 1,
                        "provider": "notebooklm",
                        "lane": "memory",
                        "group_id": gid,
                        "group_label": layout.group_label,
                        "remote_space_id": "nb_memory_1",
                        "last_scan_at": "2026-03-13T00:00:00Z",
                        "last_success_at": "2026-03-13T00:05:00Z",
                        "files": {
                            "2026-03-12": {
                                "date": "2026-03-12",
                                "file_path": str(layout.daily_dir / f"2026-03-12__{layout.group_label}.md"),
                                "relative_path": f"daily/2026-03-12__{layout.group_label}.md",
                                "content_hash": "abc",
                                "entry_count": 1,
                                "word_count": 20,
                                "source_strategy": "single",
                                "source_ids": ["src_1"],
                                "part_count": 1,
                                "state": "succeeded",
                                "attempt": 1,
                                "job_id": "job_1",
                                "synced_at": "2026-03-13T00:05:00Z",
                            }
                        },
                    },
                    ensure_ascii=False,
                ),
                encoding="utf-8",
            )

            unbind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "unbind",
                    "by": "user",
                },
            )
            self.assertTrue(unbind.ok, getattr(unbind, "error", None))
            unbind_result = unbind.result if isinstance(unbind.result, dict) else {}
            unbind_summary = unbind_result.get("memory_sync") if isinstance(unbind_result.get("memory_sync"), dict) else {}
            self.assertEqual(str(unbind_summary.get("last_success_at") or ""), "")
            self.assertEqual(int(unbind_summary.get("synced_daily_files", -1)), 0)

            status, _ = self._call("group_space_status", {"group_id": gid})
            self.assertTrue(status.ok, getattr(status, "error", None))
            status_result = status.result if isinstance(status.result, dict) else {}
            status_summary = status_result.get("memory_sync") if isinstance(status_result.get("memory_sync"), dict) else {}
            self.assertEqual(str(status_summary.get("last_success_at") or ""), "")
            self.assertEqual(int(status_summary.get("synced_daily_files", -1)), 0)

            sync_status, _ = self._call(
                "group_space_sync",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "status",
                },
            )
            self.assertTrue(sync_status.ok, getattr(sync_status, "error", None))
            sync_result = sync_status.result if isinstance(sync_status.result, dict) else {}
            sync_payload = sync_result.get("sync") if isinstance(sync_result.get("sync"), dict) else {}
            sync_summary = sync_result.get("summary") if isinstance(sync_result.get("summary"), dict) else {}
            self.assertEqual(str(sync_payload.get("remote_space_id") or ""), "")
            self.assertEqual(str(sync_payload.get("last_success_at") or ""), "")
            self.assertEqual(str(sync_summary.get("last_success_at") or ""), "")
            self.assertEqual(int(sync_summary.get("synced_daily_files", -1)), 0)
        finally:
            cleanup()

    def test_group_space_capabilities_reports_local_policy_and_ingest_schema(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("space-capabilities")
            resp, _ = self._call(
                "group_space_capabilities",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("group_id") or ""), gid)
            self.assertEqual(str(result.get("provider") or ""), "notebooklm")

            local_policy = result.get("local_file_policy") if isinstance(result.get("local_file_policy"), dict) else {}
            allowed = local_policy.get("allowed_extensions") if isinstance(local_policy.get("allowed_extensions"), list) else []
            self.assertIn(".md", allowed)
            self.assertIn(".pdf", allowed)
            self.assertNotIn(".py", allowed)
            self.assertEqual(int(local_policy.get("max_file_size_bytes") or 0), 200 * 1024 * 1024)
            self.assertEqual(
                str(local_policy.get("unsupported_error_code") or ""),
                "space_source_unsupported_format",
            )
            self.assertEqual(
                str(local_policy.get("oversize_error_code") or ""),
                "space_source_file_too_large",
            )

            ingest = result.get("ingest") if isinstance(result.get("ingest"), dict) else {}
            resource_ingest = ingest.get("resource_ingest") if isinstance(ingest.get("resource_ingest"), dict) else {}
            source_types = resource_ingest.get("source_types") if isinstance(resource_ingest.get("source_types"), list) else []
            self.assertIn("file", source_types)
            self.assertIn("web_page", source_types)
            self.assertIn("youtube", source_types)
            self.assertIn("google_docs", source_types)
            required_fields = (
                resource_ingest.get("required_fields")
                if isinstance(resource_ingest.get("required_fields"), dict)
                else {}
            )
            self.assertEqual(required_fields.get("google_docs"), ["source_type", "file_id"])
            self.assertEqual(required_fields.get("file"), ["source_type", "file_path"])

            artifacts = result.get("artifacts") if isinstance(result.get("artifacts"), dict) else {}
            kinds = artifacts.get("kinds") if isinstance(artifacts.get("kinds"), list) else []
            aliases = artifacts.get("aliases") if isinstance(artifacts.get("aliases"), dict) else {}
            self.assertIn("slide_deck", kinds)
            self.assertEqual(str(aliases.get("slide") or ""), "slide_deck")

            query_caps = result.get("query") if isinstance(result.get("query"), dict) else {}
            query_options = query_caps.get("options") if isinstance(query_caps.get("options"), dict) else {}
            query_unsupported = (
                query_caps.get("unsupported_options") if isinstance(query_caps.get("unsupported_options"), dict) else {}
            )
            self.assertIn("source_ids", query_options)
            self.assertIn("language", query_unsupported)
            self.assertIn("lang", query_unsupported)
        finally:
            cleanup()

    def test_group_space_spaces_lists_remote_notebooks(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("space-list-notebooks")
            with patch(
                "cccc.daemon.space.group_space_ops.provider_list_spaces",
                return_value={
                    "provider": "notebooklm",
                    "spaces": [
                        {"remote_space_id": "nb_1", "title": "Team Space"},
                        {"remote_space_id": "nb_2", "title": "Archive Space"},
                    ],
                },
            ):
                spaces_resp, _ = self._call(
                    "group_space_spaces",
                    {"group_id": gid, "provider": "notebooklm"},
                )
            self.assertTrue(spaces_resp.ok, getattr(spaces_resp, "error", None))
            result = spaces_resp.result if isinstance(spaces_resp.result, dict) else {}
            spaces = result.get("spaces") if isinstance(result.get("spaces"), list) else []
            self.assertEqual(len(spaces), 2)
            self.assertEqual(str(spaces[0].get("remote_space_id") or ""), "nb_1")
        finally:
            cleanup()

    def test_bind_ingest_query_jobs_with_stub(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group()
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_123",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            ingest_1, _ = self._call(
                "group_space_ingest",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "kind": "context_sync",
                    "payload": {"vision": "shipping v1"},
                    "idempotency_key": "sync-1",
                    "by": "user",
                },
            )
            self.assertTrue(ingest_1.ok, getattr(ingest_1, "error", None))
            r1 = ingest_1.result if isinstance(ingest_1.result, dict) else {}
            self.assertEqual(bool(r1.get("deduped")), False)
            self.assertEqual(bool(r1.get("accepted")), False)
            self.assertEqual(bool(r1.get("completed")), True)
            job_1 = r1.get("job") if isinstance(r1.get("job"), dict) else {}
            self.assertEqual(str(job_1.get("state") or ""), "succeeded")
            self.assertIsInstance(job_1.get("result"), dict)
            self.assertEqual((r1.get("ingest_result") or {}), job_1.get("result"))
            job_id = str(r1.get("job_id") or "")
            self.assertTrue(job_id)

            ingest_2, _ = self._call(
                "group_space_ingest",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "kind": "context_sync",
                    "payload": {"vision": "shipping v1"},
                    "idempotency_key": "sync-1",
                    "by": "user",
                },
            )
            self.assertTrue(ingest_2.ok, getattr(ingest_2, "error", None))
            r2 = ingest_2.result if isinstance(ingest_2.result, dict) else {}
            self.assertEqual(bool(r2.get("deduped")), True)
            self.assertEqual(bool(r2.get("accepted")), False)
            self.assertEqual(bool(r2.get("completed")), True)
            self.assertEqual(str(r2.get("job_id") or ""), job_id)

            query, _ = self._call(
                "group_space_query",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "query": "What is the current vision?",
                },
            )
            self.assertTrue(query.ok, getattr(query, "error", None))
            q = query.result if isinstance(query.result, dict) else {}
            self.assertEqual(bool(q.get("degraded")), False)
            self.assertIn("NotebookLM stub", str(q.get("answer") or ""))

            jobs, _ = self._call(
                "group_space_jobs",
                {"group_id": gid, "provider": "notebooklm", "lane": "work", "action": "list"},
            )
            self.assertTrue(jobs.ok, getattr(jobs, "error", None))
            jobs_list = (jobs.result or {}).get("jobs") if isinstance(jobs.result, dict) else []
            self.assertIsInstance(jobs_list, list)
            self.assertGreaterEqual(len(jobs_list), 1)
        finally:
            cleanup_stub()
            cleanup()

    def test_group_space_ingest_returns_source_id_when_provider_reports_it(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-ingest-source-id")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_ingest_src_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            with patch(
                "cccc.daemon.space.group_space_runtime.provider_ingest",
                return_value={
                    "provider": "notebooklm",
                    "remote_space_id": "nb_ingest_src_1",
                    "kind": "resource_ingest",
                    "source_type": "pasted_text",
                    "source_id": "src_intel_1",
                    "title": "Intel evidence pack",
                    "accepted": True,
                },
            ):
                ingest, _ = self._call(
                    "group_space_ingest",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "kind": "resource_ingest",
                        "payload": {"source_type": "pasted_text", "title": "Intel evidence pack", "content": "Revenue up."},
                        "idempotency_key": "ingest-src-1",
                        "by": "user",
                    },
                )
            self.assertTrue(ingest.ok, getattr(ingest, "error", None))
            result = ingest.result if isinstance(ingest.result, dict) else {}
            self.assertEqual(bool(result.get("accepted")), False)
            self.assertEqual(bool(result.get("completed")), True)
            self.assertEqual(str(result.get("source_id") or ""), "src_intel_1")
            self.assertEqual(result.get("source_ids"), ["src_intel_1"])
            ingest_result = result.get("ingest_result") if isinstance(result.get("ingest_result"), dict) else {}
            self.assertEqual(str(ingest_result.get("source_id") or ""), "src_intel_1")
            job = result.get("job") if isinstance(result.get("job"), dict) else {}
            job_result = job.get("result") if isinstance(job.get("result"), dict) else {}
            self.assertEqual(str(job_result.get("source_id") or ""), "src_intel_1")

            ingest_dedup, _ = self._call(
                "group_space_ingest",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "kind": "resource_ingest",
                    "payload": {"source_type": "pasted_text", "title": "Intel evidence pack", "content": "Revenue up."},
                    "idempotency_key": "ingest-src-1",
                    "by": "user",
                },
            )
            self.assertTrue(ingest_dedup.ok, getattr(ingest_dedup, "error", None))
            result_dedup = ingest_dedup.result if isinstance(ingest_dedup.result, dict) else {}
            self.assertEqual(bool(result_dedup.get("deduped")), True)
            self.assertEqual(str(result_dedup.get("source_id") or ""), "src_intel_1")
            self.assertEqual(result_dedup.get("source_ids"), ["src_intel_1"])
        finally:
            cleanup_stub()
            cleanup()

    def test_group_space_query_prefers_explicit_source_diagnostics(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-query-explicit-source-diagnostics")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_query_src_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            with patch(
                "cccc.daemon.space.group_space_ops.read_group_space_sync_state",
                return_value={"available": True, "remote_sources": 0, "materialized_sources": 0},
            ), patch(
                "cccc.daemon.space.group_space_ops.list_space_jobs",
                return_value=[
                    {
                        "kind": "context_sync",
                        "state": "succeeded",
                        "updated_at": "2026-03-08T10:00:00Z",
                    }
                ],
            ), patch(
                "cccc.daemon.space.group_space_ops.run_space_query",
                return_value={
                    "answer": "Intel remains cautious.",
                    "references": [{"source_id": "src_intel_1"}],
                    "degraded": False,
                    "error": None,
                },
            ):
                query, _ = self._call(
                    "group_space_query",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "query": "What does the Intel pack say?",
                        "options": {"source_ids": ["src_intel_1"]},
                    },
                )
            self.assertTrue(query.ok, getattr(query, "error", None))
            result = query.result if isinstance(query.result, dict) else {}
            self.assertEqual(str(result.get("source_basis_hint") or ""), "requested_sources_hit")
            self.assertEqual(result.get("requested_source_ids"), ["src_intel_1"])
            self.assertEqual(result.get("referenced_source_ids"), ["src_intel_1"])
            self.assertEqual(bool(result.get("references_match_requested")), True)
            self.assertEqual(str(result.get("latest_context_sync_at") or ""), "2026-03-08T10:00:00Z")
        finally:
            cleanup_stub()
            cleanup()

    def test_group_space_query_includes_work_diagnostics(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-query-work-diagnostics")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_diag_work_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            with patch(
                "cccc.daemon.space.group_space_ops.read_group_space_sync_state",
                return_value={"available": True, "remote_sources": 0, "materialized_sources": 0},
            ), patch(
                "cccc.daemon.space.group_space_ops.list_space_jobs",
                return_value=[
                    {
                        "kind": "context_sync",
                        "state": "succeeded",
                        "updated_at": "2026-03-08T10:00:00Z",
                    }
                ],
            ):
                query, _ = self._call(
                    "group_space_query",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "query": "What is the current focus?",
                    },
                )
            self.assertTrue(query.ok, getattr(query, "error", None))
            result = query.result if isinstance(query.result, dict) else {}
            self.assertEqual(int(result.get("reference_count", -1)), 0)
            self.assertEqual(str(result.get("binding_status") or ""), "bound")
            self.assertEqual(str(result.get("source_basis_hint") or ""), "context_sync_only")
            self.assertEqual(str(result.get("latest_context_sync_at") or ""), "2026-03-08T10:00:00Z")
            self.assertEqual(int(result.get("remote_sources", -1)), 0)
            self.assertEqual(int(result.get("materialized_sources", -1)), 0)
        finally:
            cleanup_stub()
            cleanup()

    def test_group_space_query_ignores_old_context_sync_after_rebind(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            from cccc.daemon.space.group_space_store import enqueue_space_job, mark_space_job_succeeded

            gid = self._create_group("space-query-rebind-diagnostics")
            bind_old, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_diag_old",
                    "by": "user",
                },
            )
            self.assertTrue(bind_old.ok, getattr(bind_old, "error", None))

            old_job, _ = enqueue_space_job(
                group_id=gid,
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_diag_old",
                kind="context_sync",
                payload={"summary": {"tasks": []}},
                idempotency_key="diag-old-success",
            )
            mark_space_job_succeeded(
                str(old_job.get("job_id") or ""),
                result={"ok": True},
            )

            bind_new, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_diag_new",
                    "by": "user",
                },
            )
            self.assertTrue(bind_new.ok, getattr(bind_new, "error", None))

            with patch(
                "cccc.daemon.space.group_space_ops.read_group_space_sync_state",
                return_value={
                    "available": True,
                    "remote_space_id": "nb_diag_new",
                    "remote_sources": 0,
                    "materialized_sources": 0,
                },
            ):
                query, _ = self._call(
                    "group_space_query",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "query": "What is the current focus?",
                    },
                )
            self.assertTrue(query.ok, getattr(query, "error", None))
            result = query.result if isinstance(query.result, dict) else {}
            self.assertEqual(str(result.get("source_basis_hint") or ""), "unknown")
            self.assertEqual(str(result.get("latest_context_sync_at") or ""), "")
        finally:
            cleanup_stub()
            cleanup()

    def test_group_space_query_includes_memory_diagnostics(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-query-memory-diagnostics")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "memory",
                    "action": "bind",
                    "remote_space_id": "nb_diag_mem_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            with patch(
                "cccc.daemon.space.group_space_ops.summarize_memory_notebooklm_sync",
                return_value={
                    "lane": "memory",
                    "last_success_at": "2026-03-08T11:00:00Z",
                    "pending_files": 1,
                    "failed_files": 0,
                    "running_files": 0,
                    "blocked_files": 0,
                },
            ):
                query, _ = self._call(
                    "group_space_query",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "memory",
                        "query": "What was decided yesterday?",
                    },
                )
            self.assertTrue(query.ok, getattr(query, "error", None))
            result = query.result if isinstance(query.result, dict) else {}
            self.assertEqual(int(result.get("reference_count", -1)), 0)
            self.assertEqual(str(result.get("binding_status") or ""), "bound")
            self.assertEqual(str(result.get("source_basis_hint") or ""), "memory_manifest_only")
            self.assertEqual(str(result.get("memory_last_success_at") or ""), "2026-03-08T11:00:00Z")
            self.assertEqual(int(result.get("memory_pending_files") or -1), 1)
            self.assertEqual(int(result.get("memory_failed_files", -1)), 0)
        finally:
            cleanup_stub()
            cleanup()

    def test_group_space_query_rejects_unsupported_language_option(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-query-options")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_query_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            bad_query, _ = self._call(
                "group_space_query",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "query": "Summarize this notebook",
                    "options": {"language": "zh-CN"},
                },
            )
            self.assertFalse(bad_query.ok)
            self.assertEqual(str(getattr(bad_query.error, "code", "")), "space_job_invalid")
            self.assertIn("language/lang", str(getattr(bad_query.error, "message", "")))
        finally:
            cleanup_stub()
            cleanup()

    def test_space_sources_list_refresh_delete_with_stub(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-sources")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_src_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            listed, _ = self._call(
                "group_space_sources",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "list",
                },
            )
            self.assertTrue(listed.ok, getattr(listed, "error", None))
            list_result = listed.result if isinstance(listed.result, dict) else {}
            self.assertEqual(str(list_result.get("action") or ""), "list")
            self.assertIsInstance(list_result.get("sources"), list)

            refreshed, _ = self._call(
                "group_space_sources",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "refresh",
                    "source_id": "src_abc",
                    "by": "user",
                },
            )
            self.assertTrue(refreshed.ok, getattr(refreshed, "error", None))
            refreshed_result = refreshed.result if isinstance(refreshed.result, dict) else {}
            self.assertEqual(str(refreshed_result.get("action") or ""), "refresh")

            deleted, _ = self._call(
                "group_space_sources",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "delete",
                    "source_id": "src_abc",
                    "by": "user",
                },
            )
            self.assertTrue(deleted.ok, getattr(deleted, "error", None))
            deleted_result = deleted.result if isinstance(deleted.result, dict) else {}
            self.assertEqual(str(deleted_result.get("action") or ""), "delete")
        finally:
            cleanup_stub()
            cleanup()

    def test_space_artifact_generate_auto_saves_to_space(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        scope_td = tempfile.TemporaryDirectory()
        try:
            gid = self._create_group("space-artifacts")
            self._attach_scope(gid, scope_td.name)
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_art_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            generated, _ = self._call(
                "group_space_artifact",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "generate",
                    "kind": "report",
                    "wait": True,
                    "save_to_space": True,
                    "by": "user",
                },
            )
            self.assertTrue(generated.ok, getattr(generated, "error", None))
            result = generated.result if isinstance(generated.result, dict) else {}
            self.assertEqual(str(result.get("action") or ""), "generate")
            self.assertEqual(str(result.get("kind") or ""), "report")
            self.assertEqual(bool(result.get("accepted")), False)
            self.assertEqual(bool(result.get("completed")), True)
            self.assertEqual(bool(result.get("saved_to_space")), True)
            output_path = str(result.get("output_path") or "")
            self.assertTrue(output_path)
            self.assertIn("/space/artifacts/notebooklm/report/", output_path)
            self.assertTrue(os.path.isfile(output_path))
        finally:
            scope_td.cleanup()
            cleanup_stub()
            cleanup()

    def test_space_artifact_generate_save_path_prefers_canonical_artifact_id(self) -> None:
        _, cleanup = self._with_home()
        scope_td = tempfile.TemporaryDirectory()
        try:
            gid = self._create_group("space-artifacts-canonical-id")
            self._attach_scope(gid, scope_td.name)
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_art_canonical",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            captured: dict[str, Any] = {}

            def _gen(*args: Any, **kwargs: Any) -> Dict[str, Any]:
                _ = args, kwargs
                return {
                    "provider": "notebooklm",
                    "remote_space_id": "nb_art_canonical",
                    "kind": "report",
                    "task_id": "tsk_123",
                    "status": "completed",
                }

            def _list(*args: Any, **kwargs: Any) -> Dict[str, Any]:
                _ = args, kwargs
                return {
                    "provider": "notebooklm",
                    "remote_space_id": "nb_art_canonical",
                    "kind": "report",
                    "artifacts": [
                        {
                            "artifact_id": "art_456",
                            "kind": "report",
                            "status": "completed",
                            "created_at": "2026-02-23T00:00:00Z",
                        }
                    ],
                }

            def _download(*args: Any, **kwargs: Any) -> Dict[str, Any]:
                output_path = str(kwargs.get("output_path") or "")
                Path(output_path).parent.mkdir(parents=True, exist_ok=True)
                Path(output_path).write_text("report-body", encoding="utf-8")
                captured["artifact_id"] = str(kwargs.get("artifact_id") or "")
                captured["output_path"] = output_path
                return {"output_path": output_path, "downloaded": True}

            with patch("cccc.daemon.space.group_space_ops.provider_generate_artifact", side_effect=_gen), \
                 patch("cccc.daemon.space.group_space_ops.provider_list_artifacts", side_effect=_list), \
                 patch("cccc.daemon.space.group_space_ops.provider_download_artifact", side_effect=_download):
                generated, _ = self._call(
                    "group_space_artifact",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "generate",
                        "kind": "report",
                        "wait": True,
                        "save_to_space": True,
                        "by": "user",
                    },
                )

            self.assertTrue(generated.ok, getattr(generated, "error", None))
            result = generated.result if isinstance(generated.result, dict) else {}
            output_path = str(result.get("output_path") or "")
            self.assertIn("/space/artifacts/notebooklm/report/", output_path)
            self.assertTrue(output_path.endswith("art_456.md"))
            self.assertEqual(str(captured.get("artifact_id") or ""), "art_456")
        finally:
            scope_td.cleanup()
            cleanup()

    def test_space_artifact_rejects_invalid_kind(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-artifacts-invalid")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_art_2",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            invalid, _ = self._call(
                "group_space_artifact",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "generate",
                    "kind": "comic",
                    "by": "user",
                },
            )
            self.assertFalse(invalid.ok)
            self.assertEqual(str(getattr(invalid.error, "code", "")), "space_job_invalid")
        finally:
            cleanup_stub()
            cleanup()

    def test_space_artifact_download_requires_output_path_when_not_saving_to_space(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-artifacts-download")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_art_3",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            bad_download, _ = self._call(
                "group_space_artifact",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "download",
                    "kind": "report",
                    "save_to_space": False,
                    "output_path": "",
                    "by": "user",
                },
            )
            self.assertFalse(bad_download.ok)
            self.assertEqual(str(getattr(bad_download.error, "code", "")), "space_job_invalid")
        finally:
            cleanup_stub()
            cleanup()

    def test_space_artifact_accepts_prefixed_kind_alias(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        temp_ctx = tempfile.TemporaryDirectory()
        try:
            gid = self._create_group("space-artifacts-prefixed-kind")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_art_4",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            output_path = os.path.join(temp_ctx.name, "artifact.png")
            downloaded, _ = self._call(
                "group_space_artifact",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "download",
                    "kind": "ArtifactType.INFOGRAPHIC",
                    "save_to_space": False,
                    "output_path": output_path,
                    "by": "user",
                },
            )
            self.assertTrue(downloaded.ok, getattr(downloaded, "error", None))
            result = downloaded.result if isinstance(downloaded.result, dict) else {}
            self.assertEqual(str(result.get("kind") or ""), "infographic")
            self.assertTrue(os.path.isfile(output_path))

            generated, _ = self._call(
                "group_space_artifact",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "generate",
                    "kind": "slide",
                    "wait": False,
                    "save_to_space": False,
                    "by": "user",
                },
            )
            self.assertTrue(generated.ok, getattr(generated, "error", None))
            gen_result = generated.result if isinstance(generated.result, dict) else {}
            self.assertEqual(str(gen_result.get("kind") or ""), "slide_deck")
        finally:
            temp_ctx.cleanup()
            cleanup_stub()
            cleanup()

    def test_space_query_backpressure_when_lane_busy(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-query-backpressure")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_query_backpressure",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            started = threading.Event()
            release = threading.Event()
            first_result: dict = {}

            def _slow_query(*, provider: str, remote_space_id: str, query: str, options: dict) -> dict:
                _ = provider
                _ = remote_space_id
                _ = query
                _ = options
                started.set()
                release.wait(2.0)
                return {"answer": "ok", "references": [], "degraded": False, "error": None}

            with patch("cccc.daemon.space.group_space_ops.run_space_query", side_effect=_slow_query):
                def _run_first() -> None:
                    resp, _ = self._call(
                        "group_space_query",
                        {
                            "group_id": gid,
                            "provider": "notebooklm",
                            "lane": "work",
                            "query": "first query",
                        },
                    )
                    first_result["resp"] = resp

                t = threading.Thread(target=_run_first, daemon=True)
                t.start()
                self.assertTrue(started.wait(1.5))

                blocked, _ = self._call(
                    "group_space_query",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "query": "second query",
                    },
                )
                self.assertFalse(blocked.ok)
                self.assertEqual(str(getattr(blocked.error, "code", "")), "space_backpressure")
                details = getattr(blocked.error, "details", {}) if blocked.error else {}
                self.assertEqual(str((details or {}).get("lane") or ""), "query")

                release.set()
                t.join(timeout=2.0)
                self.assertFalse(t.is_alive())

            first_resp = first_result.get("resp")
            self.assertIsNotNone(first_resp)
            assert first_resp is not None
            self.assertTrue(first_resp.ok, getattr(first_resp, "error", None))
        finally:
            cleanup_stub()
            cleanup()

    def test_space_artifact_generate_queue_backpressure(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-generate-queue")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_gen_queue",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            seq = {"n": 0}

            def _fake_generate(provider: str, *, remote_space_id: str, kind: str, options: dict) -> dict:
                _ = provider
                _ = remote_space_id
                _ = kind
                _ = options
                seq["n"] += 1
                return {"task_id": f"task_{seq['n']}", "status": "pending"}

            def _fake_wait(
                provider: str,
                *,
                remote_space_id: str,
                task_id: str,
                timeout_seconds: float,
                initial_interval: float,
                max_interval: float,
            ) -> dict:
                _ = provider
                _ = remote_space_id
                _ = timeout_seconds
                _ = initial_interval
                _ = max_interval
                time.sleep(1.0)
                return {"task_id": task_id, "status": "completed"}

            with patch("cccc.daemon.space.group_space_ops.provider_generate_artifact", side_effect=_fake_generate), patch(
                "cccc.daemon.space.group_space_ops.provider_wait_artifact",
                side_effect=_fake_wait,
            ):
                first, _ = self._call(
                    "group_space_artifact",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "generate",
                        "kind": "slide_deck",
                        "wait": False,
                        "save_to_space": False,
                        "by": "user",
                    },
                )
                self.assertTrue(first.ok, getattr(first, "error", None))
                first_result = first.result if isinstance(first.result, dict) else {}
                self.assertEqual(str(first_result.get("status") or ""), "pending")
                self.assertFalse(bool(first_result.get("queued")))
                self.assertEqual(str(first_result.get("recommended_next_action") or ""), "wait_for_notify")
                self.assertEqual(bool(first_result.get("polling_discouraged")), True)
                self.assertIn("Do not poll in a loop.", str(first_result.get("wait_guidance") or ""))

                second, _ = self._call(
                    "group_space_artifact",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "generate",
                        "kind": "slide_deck",
                        "wait": False,
                        "save_to_space": False,
                        "by": "user",
                    },
                )
                self.assertTrue(second.ok, getattr(second, "error", None))
                second_result = second.result if isinstance(second.result, dict) else {}
                self.assertEqual(str(second_result.get("status") or ""), "queued")
                self.assertTrue(bool(second_result.get("queued")))
                self.assertEqual(str(second_result.get("recommended_next_action") or ""), "wait_for_notify")
                self.assertEqual(bool(second_result.get("polling_discouraged")), True)
                self.assertIn("one-shot reminder", str(second_result.get("wait_guidance") or ""))

                third, _ = self._call(
                    "group_space_artifact",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "generate",
                        "kind": "slide_deck",
                        "wait": False,
                        "save_to_space": False,
                        "by": "user",
                    },
                )
                self.assertFalse(third.ok)
                self.assertEqual(str(getattr(third.error, "code", "")), "space_backpressure")
                details = getattr(third.error, "details", {}) if third.error else {}
                self.assertEqual(str((details or {}).get("lane") or ""), "generate")

                # Let async workers finish before patch exits.
                time.sleep(2.5)
        finally:
            cleanup_stub()
            cleanup()

    def test_space_artifact_async_generate_emits_system_notify(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-generate-notify")
            self._add_actor(gid, "peer1", by="user")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_gen_notify",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            with patch(
                "cccc.daemon.space.group_space_ops.provider_generate_artifact",
                return_value={"task_id": "task_notify_1", "status": "pending"},
            ), patch(
                "cccc.daemon.space.group_space_ops.provider_wait_artifact",
                return_value={"task_id": "task_notify_1", "status": "completed"},
            ):
                generated, _ = self._call(
                    "group_space_artifact",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "generate",
                        "kind": "report",
                        "wait": False,
                        "save_to_space": False,
                        "by": "user",
                    },
                )
                self.assertTrue(generated.ok, getattr(generated, "error", None))

            matched_notify = None
            for _ in range(30):
                inbox, _ = self._call(
                    "inbox_list",
                    {
                        "group_id": gid,
                        "actor_id": "peer1",
                        "by": "peer1",
                        "kind_filter": "notify",
                        "limit": 30,
                    },
                )
                self.assertTrue(inbox.ok, getattr(inbox, "error", None))
                messages = (inbox.result or {}).get("messages") if isinstance(inbox.result, dict) else []
                if isinstance(messages, list):
                    for ev in messages:
                        if not isinstance(ev, dict):
                            continue
                        if str(ev.get("kind") or "") != "system.notify":
                            continue
                        data = ev.get("data") if isinstance(ev.get("data"), dict) else {}
                        context = data.get("context") if isinstance(data.get("context"), dict) else {}
                        if str(context.get("task_id") or "") == "task_notify_1":
                            matched_notify = ev
                            break
                if matched_notify is not None:
                    break
                time.sleep(0.1)
            self.assertIsNotNone(matched_notify, "expected async generate completion notify in inbox")
            assert isinstance(matched_notify, dict)
            data = matched_notify.get("data") if isinstance(matched_notify.get("data"), dict) else {}
            context = data.get("context") if isinstance(data.get("context"), dict) else {}
            self.assertEqual(str(data.get("title") or ""), "Group Space artifact ready")
            self.assertIn("No extra polling is needed.", str(data.get("message") or ""))
            self.assertEqual(str(context.get("completion_signal") or ""), "system.notify")
            self.assertEqual(bool(context.get("polling_discouraged")), True)
            self.assertEqual(str(context.get("recommended_next_action") or ""), "download_or_list_artifact")
            self.assertIn("do not poll again", str(context.get("completion_guidance") or "").lower())
        finally:
            cleanup_stub()
            cleanup()

    def test_space_artifact_async_generate_skips_notify_when_group_has_no_actor_recipient(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import iter_events

            gid = self._create_group("space-generate-no-recipient")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_gen_no_recipient",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            with patch(
                "cccc.daemon.space.group_space_ops.provider_generate_artifact",
                return_value={"task_id": "task_notify_none", "status": "pending"},
            ), patch(
                "cccc.daemon.space.group_space_ops.provider_wait_artifact",
                return_value={"task_id": "task_notify_none", "status": "completed"},
            ):
                generated, _ = self._call(
                    "group_space_artifact",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "generate",
                        "kind": "report",
                        "wait": False,
                        "save_to_space": False,
                        "by": "user",
                    },
                )
                self.assertTrue(generated.ok, getattr(generated, "error", None))

            matched_notify = False
            for _ in range(20):
                group = load_group(gid)
                self.assertIsNotNone(group)
                assert group is not None
                matched_notify = any(
                    isinstance(ev, dict) and str(ev.get("kind") or "") == "system.notify"
                    for ev in iter_events(group.ledger_path)
                )
                if matched_notify:
                    break
                time.sleep(0.05)
            self.assertFalse(matched_notify, "did not expect async artifact notify without actor recipients")
        finally:
            cleanup_stub()
            cleanup()

    def test_space_artifact_wait_true_returns_not_completed_when_provider_stays_pending(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-generate-wait-pending")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_gen_wait_pending",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            with patch(
                "cccc.daemon.space.group_space_ops.provider_generate_artifact",
                return_value={"task_id": "task_wait_pending", "status": "pending"},
            ), patch(
                "cccc.daemon.space.group_space_ops.provider_wait_artifact",
                return_value={"task_id": "task_wait_pending", "status": "pending"},
            ):
                generated, _ = self._call(
                    "group_space_artifact",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "generate",
                        "kind": "report",
                        "wait": True,
                        "save_to_space": False,
                        "by": "user",
                    },
                )

            self.assertTrue(generated.ok, getattr(generated, "error", None))
            result = generated.result if isinstance(generated.result, dict) else {}
            self.assertEqual(str(result.get("status") or ""), "pending")
            self.assertEqual(bool(result.get("accepted")), True)
            self.assertEqual(bool(result.get("completed")), False)
            self.assertEqual(str(result.get("task_id") or ""), "task_wait_pending")
        finally:
            cleanup_stub()
            cleanup()

    def test_space_artifact_async_generate_returns_quickly_when_provider_generate_is_slow(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("space-generate-fast-return")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_gen_fast",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            def _slow_generate(provider: str, *, remote_space_id: str, kind: str, options: dict) -> dict:
                _ = provider, remote_space_id, kind, options
                time.sleep(2.0)
                return {"task_id": "task_slow_1", "status": "pending"}

            with patch(
                "cccc.daemon.space.group_space_ops.provider_generate_artifact",
                side_effect=_slow_generate,
            ), patch(
                "cccc.daemon.space.group_space_ops.provider_wait_artifact",
                return_value={"task_id": "task_slow_1", "status": "completed"},
            ):
                started = time.monotonic()
                generated, _ = self._call(
                    "group_space_artifact",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "generate",
                        "kind": "audio",
                        "wait": False,
                        "save_to_space": False,
                        "by": "user",
                    },
                )
                elapsed = time.monotonic() - started
                self.assertTrue(generated.ok, getattr(generated, "error", None))
                result = generated.result if isinstance(generated.result, dict) else {}
                self.assertEqual(str(result.get("status") or ""), "pending")
                self.assertFalse(bool(result.get("queued")))
                self.assertEqual(bool(result.get("accepted")), True)
                self.assertEqual(bool(result.get("completed")), False)
                self.assertEqual(bool(result.get("background")), True)
                self.assertEqual(str(result.get("completion_signal") or ""), "system.notify")
                self.assertEqual(str(result.get("recommended_next_action") or ""), "wait_for_notify")
                self.assertEqual(bool(result.get("polling_discouraged")), True)
                self.assertIn("Do not poll in a loop.", str(result.get("wait_guidance") or ""))
                self.assertLess(elapsed, 1.0, f"expected async return, elapsed={elapsed:.3f}s")
                # Let background worker consume patched provider fns before exiting patch scope.
                time.sleep(2.3)
        finally:
            cleanup_stub()
            cleanup()

    def test_bind_write_requires_permission(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("space-perm")
            self._add_actor(gid, "foreman1", by="user")
            self._add_actor(gid, "peer1", by="user")

            denied, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_perm",
                    "by": "peer1",
                },
            )
            self.assertFalse(denied.ok)
            self.assertEqual(str(getattr(denied.error, "code", "")), "space_permission_denied")
        finally:
            cleanup()

    def test_degraded_mode_does_not_crash_query_flow(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", None)
        try:
            gid = self._create_group("space-degraded")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_degraded",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            ingest, _ = self._call(
                "group_space_ingest",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "kind": "context_sync",
                    "payload": {"foo": "bar"},
                    "by": "user",
                },
            )
            self.assertTrue(ingest.ok, getattr(ingest, "error", None))
            job = (ingest.result or {}).get("job") if isinstance(ingest.result, dict) else {}
            self.assertEqual(str(job.get("state") or ""), "failed")
            self.assertEqual(str((job.get("last_error") or {}).get("code") or ""), "space_provider_disabled")

            query, _ = self._call(
                "group_space_query",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "query": "status?",
                },
            )
            self.assertTrue(query.ok, getattr(query, "error", None))
            result = query.result if isinstance(query.result, dict) else {}
            self.assertEqual(bool(result.get("degraded")), True)
            err = result.get("error") if isinstance(result.get("error"), dict) else {}
            self.assertEqual(str(err.get("code") or ""), "space_provider_disabled")
        finally:
            cleanup_stub()
            cleanup()

    def test_jobs_retry_and_cancel(self) -> None:
        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", None)
        try:
            gid = self._create_group("space-jobs")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_jobs",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            failed_ingest, _ = self._call(
                "group_space_ingest",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "kind": "context_sync",
                    "payload": {"a": 1},
                    "idempotency_key": "retry-1",
                    "by": "user",
                },
            )
            self.assertTrue(failed_ingest.ok, getattr(failed_ingest, "error", None))
            failed_job_id = str((failed_ingest.result or {}).get("job_id") or "")
            self.assertTrue(failed_job_id)

            retried, _ = self._call(
                "group_space_jobs",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "retry",
                    "job_id": failed_job_id,
                    "by": "user",
                },
            )
            self.assertTrue(retried.ok, getattr(retried, "error", None))
            retried_job = (retried.result or {}).get("job") if isinstance(retried.result, dict) else {}
            self.assertEqual(str(retried_job.get("state") or ""), "failed")

            from cccc.daemon.space.group_space_store import get_space_job

            with patch(
                "cccc.daemon.space.group_space_ops.execute_space_job",
                side_effect=lambda job_id: get_space_job(job_id),
            ):
                pending_ingest, _ = self._call(
                    "group_space_ingest",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "kind": "resource_ingest",
                        "payload": {"resource": "doc.md"},
                        "idempotency_key": "cancel-1",
                        "by": "user",
                    },
                )
            self.assertTrue(pending_ingest.ok, getattr(pending_ingest, "error", None))
            pending_result = pending_ingest.result if isinstance(pending_ingest.result, dict) else {}
            self.assertEqual(bool(pending_result.get("accepted")), True)
            self.assertEqual(bool(pending_result.get("completed")), False)
            pending_job = pending_result.get("job") if isinstance(pending_result.get("job"), dict) else {}
            self.assertEqual(str(pending_job.get("state") or ""), "pending")
            pending_job_id = str(pending_ingest.result.get("job_id") or "") if isinstance(pending_ingest.result, dict) else ""
            self.assertTrue(pending_job_id)

            canceled, _ = self._call(
                "group_space_jobs",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "cancel",
                    "job_id": pending_job_id,
                    "by": "user",
                },
            )
            self.assertTrue(canceled.ok, getattr(canceled, "error", None))
            canceled_job = (canceled.result or {}).get("job") if isinstance(canceled.result, dict) else {}
            self.assertEqual(str(canceled_job.get("state") or ""), "canceled")
        finally:
            cleanup_stub()
            cleanup()

    def test_provider_credential_status_update_and_clear(self) -> None:
        _, cleanup = self._with_home()
        try:
            initial, _ = self._call(
                "group_space_provider_credential_status",
                {"provider": "notebooklm", "by": "user"},
            )
            self.assertTrue(initial.ok, getattr(initial, "error", None))
            initial_cred = ((initial.result or {}).get("credential") or {}) if isinstance(initial.result, dict) else {}
            self.assertEqual(bool(initial_cred.get("configured")), False)
            self.assertEqual(str(initial_cred.get("source") or ""), "none")

            bad_update, _ = self._call(
                "group_space_provider_credential_update",
                {
                    "provider": "notebooklm",
                    "by": "user",
                    "auth_json": "{bad-json",
                    "clear": False,
                },
            )
            self.assertFalse(bad_update.ok)
            self.assertEqual(str(getattr(bad_update.error, "code", "")), "space_provider_auth_invalid")

            good_update, _ = self._call(
                "group_space_provider_credential_update",
                {
                    "provider": "notebooklm",
                    "by": "user",
                    "auth_json": '{"cookies":[{"name":"SID","value":"x","domain":".google.com"}]}',
                    "clear": False,
                },
            )
            self.assertTrue(good_update.ok, getattr(good_update, "error", None))
            cred = ((good_update.result or {}).get("credential") or {}) if isinstance(good_update.result, dict) else {}
            self.assertEqual(bool(cred.get("configured")), True)
            self.assertEqual(str(cred.get("source") or ""), "store")
            self.assertTrue(str(cred.get("masked_value") or "").strip())

            clear_resp, _ = self._call(
                "group_space_provider_credential_update",
                {
                    "provider": "notebooklm",
                    "by": "user",
                    "clear": True,
                },
            )
            self.assertTrue(clear_resp.ok, getattr(clear_resp, "error", None))
            cleared = ((clear_resp.result or {}).get("credential") or {}) if isinstance(clear_resp.result, dict) else {}
            self.assertEqual(bool(cleared.get("configured")), False)
        finally:
            cleanup()

    def test_provider_credential_status_env_overrides_store(self) -> None:
        _, cleanup = self._with_home()
        cleanup_env = self._with_env(
            "CCCC_NOTEBOOKLM_AUTH_JSON",
            '{"cookies":[{"name":"SID","value":"env","domain":".google.com"}]}',
        )
        try:
            stored_update, _ = self._call(
                "group_space_provider_credential_update",
                {
                    "provider": "notebooklm",
                    "by": "user",
                    "auth_json": '{"cookies":[{"name":"SID","value":"store","domain":".google.com"}]}',
                    "clear": False,
                },
            )
            self.assertTrue(stored_update.ok, getattr(stored_update, "error", None))

            status, _ = self._call(
                "group_space_provider_credential_status",
                {"provider": "notebooklm", "by": "user"},
            )
            self.assertTrue(status.ok, getattr(status, "error", None))
            cred = ((status.result or {}).get("credential") or {}) if isinstance(status.result, dict) else {}
            self.assertEqual(bool(cred.get("configured")), True)
            self.assertEqual(str(cred.get("source") or ""), "env")
            self.assertEqual(bool(cred.get("env_configured")), True)
            self.assertEqual(bool(cred.get("store_configured")), True)
        finally:
            cleanup_env()
            cleanup()

    def test_provider_health_check_returns_structured_result(self) -> None:
        from cccc.providers.notebooklm.errors import NotebookLMProviderError

        _, cleanup = self._with_home()
        try:
            with patch(
                "cccc.daemon.space.group_space_ops.notebooklm_health_check",
                return_value={"provider": "notebooklm", "enabled": True, "compatible": True, "reason": "ok"},
            ):
                ok_resp, _ = self._call(
                    "group_space_provider_health_check",
                    {"provider": "notebooklm", "by": "user"},
                )
            self.assertTrue(ok_resp.ok, getattr(ok_resp, "error", None))
            ok_result = ok_resp.result if isinstance(ok_resp.result, dict) else {}
            self.assertEqual(bool(ok_result.get("healthy")), True)

            with patch(
                "cccc.daemon.space.group_space_ops.notebooklm_health_check",
                side_effect=NotebookLMProviderError(
                    code="space_provider_auth_invalid",
                    message="auth invalid",
                    transient=False,
                    degrade_provider=True,
                ),
            ):
                fail_resp, _ = self._call(
                    "group_space_provider_health_check",
                    {"provider": "notebooklm", "by": "user"},
                )
            self.assertTrue(fail_resp.ok, getattr(fail_resp, "error", None))
            fail_result = fail_resp.result if isinstance(fail_resp.result, dict) else {}
            self.assertEqual(bool(fail_result.get("healthy")), False)
            err = fail_result.get("error") if isinstance(fail_result.get("error"), dict) else {}
            self.assertEqual(str(err.get("code") or ""), "space_provider_auth_invalid")
        finally:
            cleanup()

    def test_provider_health_check_compat_mismatch_marks_degraded_when_enabled(self) -> None:
        from cccc.daemon.space.group_space_store import set_space_provider_state
        from cccc.providers.notebooklm.errors import NotebookLMProviderError

        _, cleanup = self._with_home()
        try:
            set_space_provider_state("notebooklm", enabled=True, mode="active", last_error="", touch_health=True)
            with patch(
                "cccc.daemon.space.group_space_ops.notebooklm_health_check",
                side_effect=NotebookLMProviderError(
                    code="space_provider_compat_mismatch",
                    message="vendor package unavailable",
                    transient=False,
                    degrade_provider=True,
                ),
            ):
                resp, _ = self._call(
                    "group_space_provider_health_check",
                    {"provider": "notebooklm", "by": "user"},
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(bool(result.get("healthy")), False)
            provider_state = result.get("provider_state") if isinstance(result.get("provider_state"), dict) else {}
            self.assertEqual(str(provider_state.get("mode") or ""), "degraded")
            err = result.get("error") if isinstance(result.get("error"), dict) else {}
            self.assertEqual(str(err.get("code") or ""), "space_provider_compat_mismatch")
        finally:
            cleanup()

    def test_provider_auth_flow_start_status_cancel(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch(
                "cccc.daemon.space.group_space_ops.start_notebooklm_auth_flow",
                return_value={
                    "provider": "notebooklm",
                    "state": "running",
                    "phase": "waiting_user_login",
                    "session_id": "nbl_auth_1",
                },
            ) as start_mock, patch(
                "cccc.daemon.space.group_space_ops.get_notebooklm_auth_flow_status",
                return_value={
                    "provider": "notebooklm",
                    "state": "running",
                    "phase": "waiting_user_login",
                },
            ) as status_mock, patch(
                "cccc.daemon.space.group_space_ops.cancel_notebooklm_auth_flow",
                return_value={
                    "provider": "notebooklm",
                    "state": "running",
                    "phase": "canceling",
                },
            ) as cancel_mock:
                started, _ = self._call(
                    "group_space_provider_auth",
                    {
                        "provider": "notebooklm",
                        "by": "user",
                        "action": "start",
                        "timeout_seconds": 120,
                    },
                )
                self.assertTrue(started.ok, getattr(started, "error", None))
                start_mock.assert_called_once_with(timeout_seconds=120, force_reauth=False, projected=False)
                started_result = started.result if isinstance(started.result, dict) else {}
                started_auth = started_result.get("auth") if isinstance(started_result.get("auth"), dict) else {}
                self.assertEqual(str(started_auth.get("state") or ""), "running")
                provider_state = (
                    started_result.get("provider_state")
                    if isinstance(started_result.get("provider_state"), dict)
                    else {}
                )
                self.assertEqual(bool(provider_state.get("real_enabled")), False)

                status, _ = self._call(
                    "group_space_provider_auth",
                    {
                        "provider": "notebooklm",
                        "by": "user",
                        "action": "status",
                    },
                )
                self.assertTrue(status.ok, getattr(status, "error", None))
                status_mock.assert_called_once()
                status_auth = (status.result or {}).get("auth") if isinstance(status.result, dict) else {}
                self.assertEqual(str((status_auth or {}).get("state") or ""), "running")

                canceled, _ = self._call(
                    "group_space_provider_auth",
                    {
                        "provider": "notebooklm",
                        "by": "user",
                        "action": "cancel",
                    },
                )
                self.assertTrue(canceled.ok, getattr(canceled, "error", None))
                cancel_mock.assert_called_once()
                canceled_auth = (canceled.result or {}).get("auth") if isinstance(canceled.result, dict) else {}
                self.assertEqual(str((canceled_auth or {}).get("phase") or ""), "canceling")
        finally:
            cleanup()

    def test_provider_auth_flow_start_forwards_projected_flag(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch(
                "cccc.daemon.space.group_space_ops.start_notebooklm_auth_flow",
                return_value={
                    "provider": "notebooklm",
                    "state": "running",
                    "phase": "waiting_user_login",
                    "delivery": "projected_browser",
                    "session_id": "nbl_auth_projected",
                },
            ) as start_mock:
                started, _ = self._call(
                    "group_space_provider_auth",
                    {
                        "provider": "notebooklm",
                        "by": "user",
                        "action": "start",
                        "timeout_seconds": 120,
                        "projected": True,
                    },
                )
                self.assertTrue(started.ok, getattr(started, "error", None))
                start_mock.assert_called_once_with(timeout_seconds=120, force_reauth=False, projected=True)
        finally:
            cleanup()

    def test_provider_auth_flow_start_force_reauth_and_disconnect(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch(
                "cccc.daemon.space.group_space_ops.start_notebooklm_auth_flow",
                return_value={
                    "provider": "notebooklm",
                    "state": "running",
                    "phase": "starting",
                    "session_id": "nbl_auth_switch",
                },
            ) as start_mock, patch(
                "cccc.daemon.space.group_space_ops.disconnect_notebooklm_auth_flow",
                return_value={
                    "provider": "notebooklm",
                    "state": "idle",
                    "phase": "idle",
                    "session_id": "",
                },
            ) as disconnect_mock:
                started, _ = self._call(
                    "group_space_provider_auth",
                    {
                        "provider": "notebooklm",
                        "by": "user",
                        "action": "start",
                        "timeout_seconds": 180,
                        "force_reauth": True,
                    },
                )
                self.assertTrue(started.ok, getattr(started, "error", None))
                start_mock.assert_called_once_with(timeout_seconds=180, force_reauth=True, projected=False)

                disconnected, _ = self._call(
                    "group_space_provider_auth",
                    {
                        "provider": "notebooklm",
                        "by": "user",
                        "action": "disconnect",
                    },
                )
                self.assertTrue(disconnected.ok, getattr(disconnected, "error", None))
                disconnect_mock.assert_called_once_with()
                result = disconnected.result if isinstance(disconnected.result, dict) else {}
                provider_state = result.get("provider_state") if isinstance(result.get("provider_state"), dict) else {}
                self.assertEqual(bool(provider_state.get("enabled")), False)
                self.assertEqual(str(provider_state.get("mode") or ""), "disabled")
        finally:
            cleanup()

    def test_provider_auth_disconnect_rejects_running_flow(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch(
                "cccc.daemon.space.group_space_ops.disconnect_notebooklm_auth_flow",
                side_effect=RuntimeError("cannot disconnect while Google connect flow is running"),
            ):
                resp, _ = self._call(
                    "group_space_provider_auth",
                    {
                        "provider": "notebooklm",
                        "by": "user",
                        "action": "disconnect",
                    },
                )
                self.assertFalse(resp.ok)
                self.assertEqual(str(getattr(resp.error, "code", "")), "space_provider_auth_flow_running")
        finally:
            cleanup()

    def test_provider_credential_ops_require_user_identity(self) -> None:
        _, cleanup = self._with_home()
        try:
            for op, args in (
                ("group_space_provider_credential_status", {"provider": "notebooklm", "by": "foreman"}),
                (
                    "group_space_provider_credential_update",
                    {
                        "provider": "notebooklm",
                        "by": "foreman",
                        "auth_json": '{"cookies":[{"name":"SID","value":"x","domain":".google.com"}]}',
                        "clear": False,
                    },
                ),
                ("group_space_provider_health_check", {"provider": "notebooklm", "by": "foreman"}),
                ("group_space_provider_auth", {"provider": "notebooklm", "by": "foreman", "action": "status"}),
            ):
                resp, _ = self._call(op, args)
                self.assertFalse(resp.ok)
                self.assertEqual(str(getattr(resp.error, "code", "")), "space_permission_denied")
        finally:
            cleanup()

    def test_real_adapter_flag_off_rolls_back_without_blocking_group_space_flow(self) -> None:
        _, cleanup = self._with_home()
        cleanup_real = self._with_env("CCCC_NOTEBOOKLM_REAL", None)
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", None)
        try:
            gid = self._create_group("space-rollback")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_rb_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            query, _ = self._call(
                "group_space_query",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "query": "status?",
                },
            )
            self.assertTrue(query.ok, getattr(query, "error", None))
            result = query.result if isinstance(query.result, dict) else {}
            self.assertEqual(bool(result.get("degraded")), True)
            err = result.get("error") if isinstance(result.get("error"), dict) else {}
            self.assertEqual(str(err.get("code") or ""), "space_provider_disabled")

            ingest, _ = self._call(
                "group_space_ingest",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "kind": "context_sync",
                    "payload": {"k": "v"},
                    "idempotency_key": "rollback-1",
                    "by": "user",
                },
            )
            self.assertTrue(ingest.ok, getattr(ingest, "error", None))
            job = (ingest.result or {}).get("job") if isinstance(ingest.result, dict) else {}
            self.assertEqual(str(job.get("state") or ""), "failed")
            last_error = job.get("last_error") if isinstance(job.get("last_error"), dict) else {}
            self.assertEqual(str(last_error.get("code") or ""), "space_provider_disabled")
        finally:
            cleanup_stub()
            cleanup_real()
            cleanup()

    def test_bind_without_remote_id_can_auto_create_notebook(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("Space Auto Bind")
            with patch(
                "cccc.daemon.space.group_space_ops.provider_create_space",
                return_value={"provider": "notebooklm", "remote_space_id": "nb_auto_1", "created": True},
            ) as create_mock, patch(
                "cccc.daemon.space.group_space_ops.read_group_space_sync_state",
                return_value={"available": False, "reason": "no_local_scope"},
            ), patch(
                "cccc.daemon.space.group_space_ops.mark_group_space_sync_pending",
                return_value={"ok": True, "state": "pending", "remote_space_id": "nb_auto_1"},
            ) as pending_mock:
                bind, _ = self._call(
                    "group_space_bind",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "bind",
                        "remote_space_id": "",
                        "by": "user",
                    },
                )
            self.assertTrue(bind.ok, getattr(bind, "error", None))
            create_mock.assert_called_once_with("notebooklm", title="CCCC · Space Auto Bind")
            pending_mock.assert_called_once_with(gid, provider="notebooklm", remote_space_id="nb_auto_1")
            result = bind.result if isinstance(bind.result, dict) else {}
            binding = (((result.get("bindings") or {}).get("work")) if isinstance(result.get("bindings"), dict) else {})
            self.assertEqual(str(binding.get("remote_space_id") or ""), "nb_auto_1")
            sync_result = result.get("sync_result") if isinstance(result.get("sync_result"), dict) else {}
            self.assertEqual(bool(sync_result.get("ok")), True)
            self.assertEqual(bool(sync_result.get("deferred")), True)
        finally:
            cleanup()

    def test_bind_work_lane_defers_sync_and_marks_pending_when_remote_changes(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("Space Rebind Fast Path")
            with patch(
                "cccc.daemon.space.group_space_ops.read_group_space_sync_state",
                return_value={"available": True, "remote_space_id": "nb_old_1", "converged": True},
            ), patch(
                "cccc.daemon.space.group_space_ops.mark_group_space_sync_pending",
                return_value={"ok": True, "state": "pending", "remote_space_id": "nb_existing_1"},
            ) as pending_mock, patch(
                "cccc.daemon.space.group_space_ops.sync_group_space_files",
                return_value={"ok": True, "converged": True, "unsynced_count": 0},
            ) as sync_mock:
                bind, _ = self._call(
                    "group_space_bind",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "bind",
                        "remote_space_id": "nb_existing_1",
                        "by": "user",
                    },
                )
            self.assertTrue(bind.ok, getattr(bind, "error", None))
            sync_mock.assert_not_called()
            pending_mock.assert_called_once_with(gid, provider="notebooklm", remote_space_id="nb_existing_1")
            result = bind.result if isinstance(bind.result, dict) else {}
            sync_result = result.get("sync_result") if isinstance(result.get("sync_result"), dict) else {}
            self.assertEqual(bool(sync_result.get("ok")), True)
            self.assertEqual(bool(sync_result.get("deferred")), True)
            self.assertEqual(str(sync_result.get("reason") or ""), "background_sync")
        finally:
            cleanup()

    def test_bind_work_lane_same_remote_keeps_existing_sync_state_fast_path(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("Space Same Remote Rebind")
            with patch(
                "cccc.daemon.space.group_space_ops.read_group_space_sync_state",
                return_value={"available": True, "remote_space_id": "nb_existing_1", "converged": True},
            ), patch(
                "cccc.daemon.space.group_space_ops.mark_group_space_sync_pending",
            ) as pending_mock, patch(
                "cccc.daemon.space.group_space_ops.sync_group_space_files",
                return_value={"ok": True, "converged": True, "unsynced_count": 0},
            ):
                bind, _ = self._call(
                    "group_space_bind",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "bind",
                        "remote_space_id": "nb_existing_1",
                        "by": "user",
                    },
                )
            self.assertTrue(bind.ok, getattr(bind, "error", None))
            pending_mock.assert_not_called()
            result = bind.result if isinstance(bind.result, dict) else {}
            sync_result = result.get("sync_result") if isinstance(result.get("sync_result"), dict) else {}
            self.assertEqual(bool(sync_result.get("ok")), True)
            self.assertEqual(bool(sync_result.get("deferred")), True)
            self.assertEqual(str(sync_result.get("remote_space_id") or ""), "nb_existing_1")
        finally:
            cleanup()

    def test_group_space_sync_work_lane_defers_to_executor_when_available(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.space.group_space_ops import handle_group_space_sync

            gid = self._create_group("space-sync-executor")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_exec_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            queued: list[dict[str, object]] = []
            with patch(
                "cccc.daemon.space.group_space_ops.mark_group_space_sync_pending",
                return_value={"ok": True, "state": "pending", "remote_space_id": "nb_exec_1"},
            ) as pending_mock, patch(
                "cccc.daemon.space.group_space_ops.sync_group_space_files",
            ) as sync_mock:
                resp = handle_group_space_sync(
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "action": "run",
                        "force": True,
                        "by": "user",
                    },
                    enqueue_group_space_sync_run=lambda **kwargs: queued.append(kwargs) or {
                        "accepted": True,
                        "queued": True,
                        "reason": "queued",
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "force": True,
                    },
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            pending_mock.assert_called_once_with(gid, provider="notebooklm", remote_space_id="nb_exec_1")
            sync_mock.assert_not_called()
            self.assertEqual(
                queued,
                [{"group_id": gid, "provider": "notebooklm", "force": True, "by": "user"}],
            )
            result = resp.result if isinstance(resp.result, dict) else {}
            sync_result = result.get("sync_result") if isinstance(result.get("sync_result"), dict) else {}
            self.assertEqual(bool(sync_result.get("deferred")), True)
            self.assertEqual(bool(sync_result.get("queued")), True)
        finally:
            cleanup()

    def test_group_space_sync_work_lane_enqueue_rejection_restores_previous_state(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.space.group_space_ops import handle_group_space_sync
            from cccc.daemon.space.group_space_sync import read_group_space_sync_state

            gid = self._create_group("space-sync-enqueue-reject")
            bind, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_exec_reject",
                    "by": "user",
                },
            )
            self.assertTrue(bind.ok, getattr(bind, "error", None))

            original_state = read_group_space_sync_state(gid)

            resp = handle_group_space_sync(
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "run",
                    "force": True,
                    "by": "user",
                },
                enqueue_group_space_sync_run=lambda **kwargs: {
                    "accepted": False,
                    "code": "queue_rejected",
                    "message": "executor rejected request",
                    "group_id": kwargs.get("group_id"),
                },
            )

            self.assertFalse(resp.ok)
            self.assertEqual(str(resp.error.code), "queue_rejected")
            restored_state = read_group_space_sync_state(gid)
            self.assertEqual(
                str(restored_state.get("remote_space_id") or ""),
                str(original_state.get("remote_space_id") or ""),
            )
            self.assertEqual(
                str(restored_state.get("state") or ""),
                str(original_state.get("state") or ""),
            )
            self.assertEqual(
                bool(restored_state.get("available")),
                bool(original_state.get("available")),
            )
        finally:
            cleanup()

    def test_notebooklm_auth_verification_works_inside_running_event_loop(self) -> None:
        from cccc.daemon.space import notebooklm_auth_flow as auth_flow

        fake_vendor_module = types.ModuleType("notebooklm_auth_fake")

        def _extract_cookies_from_storage(storage_state):
            raw = storage_state.get("cookies")
            return list(raw) if isinstance(raw, list) else []

        async def _fetch_tokens(cookies):
            if not cookies:
                raise RuntimeError("missing cookies")
            return ("csrf_token", "session_id")

        fake_vendor_module.extract_cookies_from_storage = _extract_cookies_from_storage
        fake_vendor_module.fetch_tokens = _fetch_tokens

        class _Compat:
            compatible = True
            reason = ""

        storage_state = {
            "cookies": [{"name": "__Secure-1PSID", "value": "x", "domain": ".google.com", "path": "/"}],
            "origins": [],
        }

        with patch.object(auth_flow, "parse_notebooklm_auth_json", return_value={"ok": True}), patch.object(
            auth_flow,
            "probe_notebooklm_vendor",
            return_value=_Compat(),
        ), patch.dict(
            sys.modules,
            {"cccc.providers.notebooklm._vendor.notebooklm.auth": fake_vendor_module},
        ):

            async def _run_verify() -> None:
                auth_flow._verify_storage_state(storage_state)

            asyncio.run(_run_verify())

    def test_notebooklm_auth_browser_profile_is_persistent_under_home(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.space import notebooklm_auth_flow as auth_flow

            profile_a = auth_flow._managed_browser_profile_dir()
            profile_b = auth_flow._managed_browser_profile_dir()
            self.assertEqual(profile_a, profile_b)
            self.assertTrue(profile_a.exists())
            self.assertTrue(profile_a.is_dir())
            self.assertIn(str(os.environ.get("CCCC_HOME") or ""), str(profile_a))
        finally:
            cleanup()

    def test_notebooklm_auth_browser_profile_session_dir_is_unique_under_home(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.space import notebooklm_auth_flow as auth_flow

            session_a = auth_flow._fresh_managed_browser_profile_session_dir()
            session_b = auth_flow._fresh_managed_browser_profile_session_dir()
            self.assertNotEqual(session_a, session_b)
            self.assertTrue(session_a.exists())
            self.assertTrue(session_b.exists())
            self.assertIn(str(auth_flow._managed_browser_profile_dir()), str(session_a))
            self.assertIn(str(auth_flow._managed_browser_profile_dir()), str(session_b))
        finally:
            cleanup()

    def test_notebooklm_projected_auth_browser_requires_system_browser_cdp(self) -> None:
        from cccc.daemon.space import notebooklm_auth_browser_runtime as runtime

        with patch.object(runtime._MANAGER, "open", return_value={"active": True, "state": "ready"}) as open_session:
            runtime.open_notebooklm_auth_browser_session(
                profile_dir=Path("/tmp/notebooklm-auth-profile"),
                url="https://notebooklm.google.com/",
                width=1280,
                height=900,
            )

        kwargs = open_session.call_args.kwargs
        self.assertEqual(tuple(kwargs.get("channel_candidates") or ()), ("chrome", "msedge"))
        self.assertEqual(kwargs.get("require_system_browser_cdp"), True)

    def test_notebooklm_auth_flow_reuses_saved_credential_without_browser(self) -> None:
        from cccc.daemon.space import notebooklm_auth_flow as auth_flow

        saved_storage = {
            "cookies": [{"name": "SID", "value": "saved", "domain": ".google.com", "path": "/"}],
            "origins": [],
        }
        with patch.object(auth_flow, "_load_saved_storage_state", return_value=saved_storage), patch.object(
            auth_flow,
            "_verify_storage_state",
            return_value=None,
        ), patch.object(
            auth_flow,
            "_ensure_sync_playwright",
            side_effect=AssertionError("browser should not start when saved credential is valid"),
        ), patch.object(
            auth_flow,
            "get_space_provider_state",
            return_value={"enabled": False, "real_enabled": False},
        ), patch.object(
            auth_flow,
            "set_space_provider_state",
            return_value={"enabled": True, "real_enabled": True, "mode": "active"},
        ):
            auth_flow._connect_worker(
                session_id="nbl_auth_test_reuse",
                timeout_seconds=120,
                cancel_event=threading.Event(),
            )
            state = auth_flow.get_notebooklm_auth_flow_status()
            self.assertEqual(str(state.get("state") or ""), "succeeded")
            self.assertIn("connected", str(state.get("message") or "").lower())

    def test_notebooklm_auth_flow_force_reauth_skips_saved_credential_reuse(self) -> None:
        from cccc.daemon.space import notebooklm_auth_flow as auth_flow

        saved_storage = {
            "cookies": [{"name": "SID", "value": "saved", "domain": ".google.com", "path": "/"}],
            "origins": [],
        }
        with patch.object(auth_flow, "_load_saved_storage_state", return_value=saved_storage), patch.object(
            auth_flow,
            "_verify_storage_state",
            side_effect=AssertionError("saved credential reuse must be skipped for force_reauth"),
        ), patch.object(
            auth_flow,
            "_ensure_sync_playwright",
            side_effect=RuntimeError("browser unavailable"),
        ), patch.object(
            auth_flow,
            "_fresh_managed_browser_profile_session_dir",
        ) as fresh_profile_mock, patch.object(
            auth_flow,
            "_start_browser_session",
            side_effect=AssertionError("browser session should not start when browser runtime is unavailable"),
        ), patch.object(
            auth_flow,
            "get_space_provider_state",
            return_value={"enabled": False, "real_enabled": False},
        ), patch.object(
            auth_flow,
            "set_space_provider_state",
            return_value={"enabled": False, "real_enabled": False, "mode": "disabled"},
        ):
            auth_flow._connect_worker(
                session_id="nbl_auth_test_force_reauth",
                timeout_seconds=120,
                cancel_event=threading.Event(),
                force_reauth=True,
            )
            fresh_profile_mock.assert_not_called()
            state = auth_flow.get_notebooklm_auth_flow_status()
            self.assertEqual(str(state.get("state") or ""), "failed")
            self.assertIn("failed to prepare browser runtime", str(state.get("message") or "").lower())

    def test_notebooklm_auth_flow_disconnect_ignores_locked_browser_profile_dir(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.space import notebooklm_auth_flow as auth_flow
            from cccc.daemon.space.group_space_store import update_space_provider_secrets

            _ = update_space_provider_secrets(
                "notebooklm",
                set_vars={
                    "NOTEBOOKLM_AUTH_JSON": '{"cookies":[{"name":"SID","value":"saved","domain":".google.com"}]}'
                },
                unset_keys=[],
                clear=False,
            )
            os.environ["CCCC_NOTEBOOKLM_AUTH_JSON"] = '{"cookies":[{"name":"SID","value":"env","domain":".google.com"}]}'

            profile_dir = auth_flow._managed_browser_profile_dir()
            marker = profile_dir / "locked.txt"
            marker.write_text("x", encoding="utf-8")

            real_rmtree = shutil.rmtree
            call_count = {"value": 0}

            def _flaky_rmtree(path: str | os.PathLike[str], *args: Any, **kwargs: Any) -> None:
                if Path(path) == profile_dir:
                    call_count["value"] += 1
                    raise PermissionError(32, "sharing violation", str(path))
                real_rmtree(path, *args, **kwargs)

            with patch.object(auth_flow.shutil, "rmtree", side_effect=_flaky_rmtree):
                state = auth_flow.disconnect_notebooklm_auth_flow()

            self.assertGreater(call_count["value"], 0)
            self.assertEqual(str(state.get("state") or ""), "idle")
            self.assertNotIn("CCCC_NOTEBOOKLM_AUTH_JSON", os.environ)
            self.assertTrue(profile_dir.exists())
        finally:
            cleanup()

    def test_notebooklm_auth_flow_closes_windows_system_browser_process_tree(self) -> None:
        from cccc.daemon.space import notebooklm_auth_flow as auth_flow

        class _FakeClosable:
            def __init__(self) -> None:
                self.closed = False

            def close(self) -> None:
                self.closed = True

        class _FakeProc:
            def __init__(self) -> None:
                self.pid = 4321
                self.terminated = False
                self.killed = False
                self.wait_calls: list[float] = []

            def poll(self) -> None:
                return None

            def wait(self, timeout: float | None = None) -> int:
                self.wait_calls.append(float(timeout or 0.0))
                return 0

            def terminate(self) -> None:
                self.terminated = True

            def kill(self) -> None:
                self.killed = True

        context = _FakeClosable()
        browser = _FakeClosable()
        proc = _FakeProc()
        session = auth_flow._AuthBrowserSession(
            browser=browser,
            context=context,
            strategy="system_browser_cdp:chrome.exe",
            process=proc,
        )

        with patch.object(auth_flow.os, "name", "nt"), patch.object(
            auth_flow.subprocess,
            "run",
            return_value=None,
        ) as taskkill_mock:
            session.close()

        self.assertTrue(context.closed)
        self.assertTrue(browser.closed)
        taskkill_mock.assert_called_once()
        self.assertEqual(
            taskkill_mock.call_args.args[0],
            ["taskkill", "/PID", "4321", "/T", "/F"],
        )
        self.assertEqual(proc.wait_calls, [1.0])
        self.assertFalse(proc.terminated)
        self.assertFalse(proc.killed)

    @unittest.skip("Deprecated: NotebookLM auth browser flow requires opening a real web page.")
    def test_notebooklm_auth_flow_waits_for_notebook_before_collecting_cookies(self) -> None:
        from cccc.daemon.space import notebooklm_auth_flow as auth_flow

        class _FakePage:
            def __init__(self) -> None:
                self.url = "https://notebooklm.google.com/"

            def goto(self, url: str, **_kwargs) -> None:
                self.url = url

        class _FakeContext:
            def __init__(self) -> None:
                self.pages = [_FakePage()]

            def new_page(self) -> _FakePage:
                page = _FakePage()
                self.pages.append(page)
                return page

        class _FakeBrowserSession:
            def __init__(self, context: _FakeContext) -> None:
                self.context = context
                self.strategy = "test_browser"
                self.closed = False

            def close(self) -> None:
                self.closed = True

        class _FakePlaywrightContextManager:
            def __enter__(self) -> object:
                return object()

            def __exit__(self, exc_type, exc, tb) -> bool:
                _ = exc_type, exc, tb
                return False

        fake_context = _FakeContext()
        fake_session = _FakeBrowserSession(fake_context)
        observed_urls = iter(
            [
                ["https://accounts.google.com/signin/v2/identifier"],
                ["https://accounts.google.com/signin/v2/challenge/pwd"],
                ["https://notebooklm.google.com/"],
            ]
        )
        collect_calls: list[int] = []
        fake_now = {"value": 1000.0}

        def _fake_time() -> float:
            return float(fake_now["value"])

        def _fake_sleep(seconds: float) -> None:
            fake_now["value"] += max(0.0, float(seconds))

        def _fake_page_urls(_context: Any) -> list[str]:
            try:
                return list(next(observed_urls))
            except StopIteration:
                return ["https://notebooklm.google.com/"]

        def _fake_collect_storage(_context: Any) -> Dict[str, Any]:
            collect_calls.append(1)
            return {
                "cookies": [{"name": "SID", "value": "fresh", "domain": ".google.com", "path": "/"}],
                "origins": [],
            }

        with patch.object(auth_flow, "_load_saved_storage_state", return_value=None), patch.object(
            auth_flow,
            "_ensure_sync_playwright",
            return_value=_FakePlaywrightContextManager,
        ), patch.object(
            auth_flow,
            "_start_browser_session",
            return_value=fake_session,
        ), patch.object(
            auth_flow,
            "_page_urls",
            side_effect=_fake_page_urls,
        ), patch.object(
            auth_flow,
            "_collect_storage_state",
            side_effect=_fake_collect_storage,
        ), patch.object(
            auth_flow,
            "_verify_storage_state",
            return_value=None,
        ), patch.object(
            auth_flow,
            "_persist_storage_state",
            return_value=None,
        ), patch.object(
            auth_flow,
            "get_space_provider_state",
            return_value={"enabled": False, "real_enabled": False},
        ), patch.object(
            auth_flow,
            "set_space_provider_state",
            return_value={"enabled": True, "real_enabled": True, "mode": "active"},
        ), patch.object(
            auth_flow.time,
            "time",
            side_effect=_fake_time,
        ), patch.object(
            auth_flow.time,
            "sleep",
            side_effect=_fake_sleep,
        ):
            auth_flow._connect_worker(
                session_id="nbl_auth_wait_for_notebook",
                timeout_seconds=120,
                cancel_event=threading.Event(),
            )

        self.assertEqual(len(collect_calls), 1)
        self.assertTrue(fake_session.closed)
        state = auth_flow.get_notebooklm_auth_flow_status()
        self.assertEqual(str(state.get("state") or ""), "succeeded")
        self.assertIn("connected", str(state.get("message") or "").lower())

    @unittest.skip("Deprecated: NotebookLM auth browser flow requires opening a real web page.")
    def test_notebooklm_auth_flow_cookie_peek_recovers_when_notebook_page_detection_misses(self) -> None:
        from cccc.daemon.space import notebooklm_auth_flow as auth_flow

        class _FakePage:
            def __init__(self) -> None:
                self.url = "https://accounts.google.com/signin/v2/identifier"

            def goto(self, url: str, **_kwargs) -> None:
                self.url = url

        class _FakeContext:
            def __init__(self) -> None:
                self.pages = [_FakePage()]

            def new_page(self) -> _FakePage:
                page = _FakePage()
                self.pages.append(page)
                return page

        class _FakeBrowserSession:
            def __init__(self, context: _FakeContext) -> None:
                self.context = context
                self.strategy = "test_browser"
                self.closed = False

            def close(self) -> None:
                self.closed = True

        class _FakePlaywrightContextManager:
            def __enter__(self) -> object:
                return object()

            def __exit__(self, exc_type, exc, tb) -> bool:
                _ = exc_type, exc, tb
                return False

        fake_context = _FakeContext()
        fake_session = _FakeBrowserSession(fake_context)
        peek_calls: list[int] = []
        persisted_states: list[Dict[str, Any]] = []
        peek_results = iter(
            [
                [],
                [{"name": "SID", "value": "fresh", "domain": ".google.com", "path": "/"}],
            ]
        )
        fake_now = {"value": 2000.0}

        def _fake_time() -> float:
            return float(fake_now["value"])

        def _fake_sleep(seconds: float) -> None:
            fake_now["value"] += max(0.0, float(seconds))

        def _fake_peek(_context: Any) -> list[Dict[str, Any]]:
            peek_calls.append(1)
            try:
                return list(next(peek_results))
            except StopIteration:
                return [{"name": "SID", "value": "fresh", "domain": ".google.com", "path": "/"}]

        def _fake_collect_storage(_context: Any) -> Dict[str, Any]:
            return {
                "cookies": [{"name": "SID", "value": "fresh", "domain": ".google.com", "path": "/"}],
                "origins": [],
            }

        def _fake_persist_storage(state: Dict[str, Any]) -> None:
            persisted_states.append(dict(state))

        with patch.object(auth_flow, "_load_saved_storage_state", return_value=None), patch.object(
            auth_flow,
            "_ensure_sync_playwright",
            return_value=_FakePlaywrightContextManager,
        ), patch.object(
            auth_flow,
            "_start_browser_session",
            return_value=fake_session,
        ), patch.object(
            auth_flow,
            "_page_urls",
            return_value=["https://accounts.google.com/signin/v2/identifier"],
        ), patch.object(
            auth_flow,
            "_peek_google_cookies",
            side_effect=_fake_peek,
        ), patch.object(
            auth_flow,
            "_collect_storage_state",
            side_effect=_fake_collect_storage,
        ), patch.object(
            auth_flow,
            "_verify_storage_state",
            return_value=None,
        ), patch.object(
            auth_flow,
            "_persist_storage_state",
            side_effect=_fake_persist_storage,
        ), patch.object(
            auth_flow,
            "get_space_provider_state",
            return_value={"enabled": False, "real_enabled": False},
        ), patch.object(
            auth_flow,
            "set_space_provider_state",
            return_value={"enabled": True, "real_enabled": True, "mode": "active"},
        ), patch.object(
            auth_flow.time,
            "time",
            side_effect=_fake_time,
        ), patch.object(
            auth_flow.time,
            "sleep",
            side_effect=_fake_sleep,
        ):
            auth_flow._connect_worker(
                session_id="nbl_auth_cookie_peek_fallback",
                timeout_seconds=120,
                cancel_event=threading.Event(),
                force_reauth=True,
            )

        self.assertGreaterEqual(len(peek_calls), 2)
        self.assertEqual(len(persisted_states), 1)
        cookies = persisted_states[0].get("cookies") if isinstance(persisted_states[0], dict) else []
        self.assertEqual(str((cookies[0] if cookies else {}).get("value") or ""), "fresh")
        self.assertTrue(fake_session.closed)
        state = auth_flow.get_notebooklm_auth_flow_status()
        self.assertEqual(str(state.get("state") or ""), "succeeded")
        self.assertIn("connected", str(state.get("message") or "").lower())

    def test_notebooklm_auth_flow_projected_browser_path_persists_and_verifies(self) -> None:
        from cccc.daemon.space import notebooklm_auth_flow as auth_flow

        browser_state = {
            "active": True,
            "state": "ready",
            "message": "ready",
            "error": {},
            "strategy": "playwright_channel:chrome_headless",
            "url": "https://notebooklm.google.com/",
            "width": 1366,
            "height": 900,
            "started_at": "2026-03-24T00:00:00Z",
            "updated_at": "2026-03-24T00:00:00Z",
            "last_frame_seq": 0,
            "last_frame_at": "",
            "controller_attached": False,
        }
        storage_state = {
            "cookies": [{"name": "SID", "value": "fresh", "domain": ".google.com", "path": "/"}],
            "origins": [],
        }

        with patch.object(auth_flow, "_load_saved_storage_state", return_value=None), patch.object(
            auth_flow,
            "open_notebooklm_auth_browser_session",
            return_value=browser_state,
        ) as open_browser_mock, patch.object(
            auth_flow,
            "notebooklm_auth_browser_page_urls",
            return_value=["https://notebooklm.google.com/"],
        ), patch.object(
            auth_flow,
            "notebooklm_auth_browser_storage_state",
            return_value=storage_state,
        ), patch.object(
            auth_flow,
            "notebooklm_auth_browser_google_cookies",
            return_value=[],
        ), patch.object(
            auth_flow,
            "_verify_storage_state",
            return_value=None,
        ), patch.object(
            auth_flow,
            "_persist_storage_state",
            return_value=None,
        ) as persist_mock, patch.object(
            auth_flow,
            "close_notebooklm_auth_browser_session",
            return_value={"closed": True},
        ) as close_browser_mock, patch.object(
            auth_flow,
            "get_space_provider_state",
            return_value={"enabled": False, "real_enabled": False},
        ), patch.object(
            auth_flow,
            "set_space_provider_state",
            return_value={"enabled": True, "real_enabled": True, "mode": "active"},
        ):
            auth_flow._connect_worker(
                session_id="nbl_auth_projected_success",
                timeout_seconds=120,
                cancel_event=threading.Event(),
                projected=True,
            )

        open_browser_mock.assert_called_once()
        persist_mock.assert_called_once()
        close_browser_mock.assert_called()
        state = auth_flow.get_notebooklm_auth_flow_status()
        self.assertEqual(str(state.get("state") or ""), "succeeded")
        self.assertEqual(str(state.get("delivery") or ""), "projected_browser")
        self.assertIn("connected", str(state.get("message") or "").lower())

    def test_notebooklm_auth_flow_projected_browser_cleans_session_profile_dir(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.space import notebooklm_auth_flow as auth_flow

            session_dir = auth_flow._fresh_managed_browser_profile_session_dir()
            marker = session_dir / "marker.txt"
            marker.write_text("x", encoding="utf-8")
            storage_state = {
                "cookies": [{"name": "SID", "value": "fresh", "domain": ".google.com", "path": "/"}],
                "origins": [],
            }

            with patch.object(auth_flow, "_load_saved_storage_state", return_value=None), patch.object(
                auth_flow,
                "_fresh_managed_browser_profile_session_dir",
                return_value=session_dir,
            ), patch.object(
                auth_flow,
                "open_notebooklm_auth_browser_session",
                return_value={
                    "active": True,
                    "state": "ready",
                    "message": "ready",
                    "error": {},
                    "strategy": "playwright_channel:chrome_headless",
                    "url": "https://notebooklm.google.com/",
                    "width": 1366,
                    "height": 900,
                    "started_at": "2026-03-24T00:00:00Z",
                    "updated_at": "2026-03-24T00:00:00Z",
                    "last_frame_seq": 0,
                    "last_frame_at": "",
                    "controller_attached": False,
                },
            ), patch.object(
                auth_flow,
                "notebooklm_auth_browser_page_urls",
                return_value=["https://notebooklm.google.com/"],
            ), patch.object(
                auth_flow,
                "notebooklm_auth_browser_storage_state",
                return_value=storage_state,
            ), patch.object(
                auth_flow,
                "notebooklm_auth_browser_google_cookies",
                return_value=[],
            ), patch.object(
                auth_flow,
                "_verify_storage_state",
                return_value=None,
            ), patch.object(
                auth_flow,
                "_persist_storage_state",
                return_value=None,
            ), patch.object(
                auth_flow,
                "close_notebooklm_auth_browser_session",
                return_value={"closed": True},
            ), patch.object(
                auth_flow,
                "get_space_provider_state",
                return_value={"enabled": False, "real_enabled": False},
            ), patch.object(
                auth_flow,
                "set_space_provider_state",
                return_value={"enabled": True, "real_enabled": True, "mode": "active"},
            ):
                auth_flow._connect_worker(
                    session_id="nbl_auth_projected_cleanup",
                    timeout_seconds=120,
                    cancel_event=threading.Event(),
                    projected=True,
                )

            self.assertFalse(session_dir.exists())
        finally:
            cleanup()

    def test_notebooklm_auth_flow_disconnect_clears_saved_state(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.space import notebooklm_auth_flow as auth_flow
            from cccc.daemon.space.group_space_store import load_space_provider_secrets, update_space_provider_secrets

            _ = update_space_provider_secrets(
                "notebooklm",
                set_vars={
                    "NOTEBOOKLM_AUTH_JSON": '{"cookies":[{"name":"SID","value":"saved","domain":".google.com"}]}'
                },
                unset_keys=[],
                clear=False,
            )
            os.environ["CCCC_NOTEBOOKLM_AUTH_JSON"] = '{"cookies":[{"name":"SID","value":"env","domain":".google.com"}]}'
            profile_dir = auth_flow._managed_browser_profile_dir()
            marker = profile_dir / "marker.txt"
            marker.write_text("x", encoding="utf-8")

            state = auth_flow.disconnect_notebooklm_auth_flow()

            self.assertEqual(str(state.get("state") or ""), "idle")
            self.assertEqual(str(state.get("phase") or ""), "idle")
            self.assertNotIn("CCCC_NOTEBOOKLM_AUTH_JSON", os.environ)
            self.assertFalse(profile_dir.exists())
            self.assertEqual(load_space_provider_secrets("notebooklm"), {})
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
