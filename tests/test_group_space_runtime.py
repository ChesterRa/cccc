from __future__ import annotations

import json
import os
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch


class TestGroupSpaceRuntime(unittest.TestCase):
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

    def _create_group(self, title: str = "space-runtime") -> str:
        create, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        gid = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _bind(self, group_id: str, remote_space_id: str = "nb_runtime") -> None:
        bind, _ = self._call(
            "group_space_bind",
            {
                "group_id": group_id,
                "provider": "notebooklm",
                "lane": "work",
                "action": "bind",
                "remote_space_id": remote_space_id,
                "by": "user",
            },
        )
        self.assertTrue(bind.ok, getattr(bind, "error", None))

    def test_transient_ingest_schedules_retry_instead_of_immediate_loop(self) -> None:
        from cccc.daemon.space.group_space_provider import SpaceProviderError

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("space-retry-schedule")
            self._bind(gid, "nb_retry_1")
            calls = {"n": 0}

            def _flaky_ingest(provider: str, *, remote_space_id: str, kind: str, payload: dict):
                calls["n"] += 1
                raise SpaceProviderError(
                    "space_upstream_busy",
                    "temporary upstream busy",
                    transient=True,
                    degrade_provider=False,
                )

            with patch("cccc.daemon.space.group_space_runtime.provider_ingest", side_effect=_flaky_ingest):
                ingest, _ = self._call(
                    "group_space_ingest",
                    {
                        "group_id": gid,
                        "provider": "notebooklm",
                        "lane": "work",
                        "kind": "context_sync",
                        "payload": {"k": "v"},
                        "idempotency_key": "rt-schedule-1",
                        "by": "user",
                    },
                )

            self.assertTrue(ingest.ok, getattr(ingest, "error", None))
            result = ingest.result if isinstance(ingest.result, dict) else {}
            job = result.get("job") if isinstance(result.get("job"), dict) else {}
            self.assertEqual(str(job.get("state") or ""), "pending")
            self.assertEqual(int(job.get("attempt") or 0), 1)
            self.assertTrue(str(job.get("next_run_at") or "").strip())
            self.assertEqual(calls["n"], 1)
        finally:
            cleanup()

    def test_due_worker_processes_ready_pending_job(self) -> None:
        from cccc.daemon.space.group_space_provider import SpaceProviderError
        from cccc.daemon.space.group_space_runtime import process_due_space_jobs
        from cccc.daemon.space.group_space_store import get_space_job

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("space-due-worker")
            self._bind(gid, "nb_retry_2")
            calls = {"n": 0}

            def _flaky_then_ok(provider: str, *, remote_space_id: str, kind: str, payload: dict):
                calls["n"] += 1
                if calls["n"] == 1:
                    raise SpaceProviderError(
                        "space_upstream_busy",
                        "temporary upstream busy",
                        transient=True,
                        degrade_provider=False,
                    )
                return {"ok": True}

            with patch("cccc.daemon.space.group_space_runtime._RETRY_BACKOFF_SECONDS", (0, 0)):
                with patch("cccc.daemon.space.group_space_runtime.provider_ingest", side_effect=_flaky_then_ok):
                    ingest, _ = self._call(
                        "group_space_ingest",
                        {
                            "group_id": gid,
                            "provider": "notebooklm",
                            "lane": "work",
                            "kind": "resource_ingest",
                            "payload": {"doc": "a.md"},
                            "idempotency_key": "rt-worker-1",
                            "by": "user",
                        },
                    )
                    self.assertTrue(ingest.ok, getattr(ingest, "error", None))
                    result = ingest.result if isinstance(ingest.result, dict) else {}
                    job_id = str(result.get("job_id") or "")
                    self.assertTrue(job_id)
                    pre = get_space_job(job_id) or {}
                    self.assertEqual(str(pre.get("state") or ""), "pending")

                    tick_result = process_due_space_jobs(limit=10)
                    self.assertGreaterEqual(int(tick_result.get("processed") or 0), 1)

                    post = get_space_job(job_id) or {}
                    self.assertEqual(str(post.get("state") or ""), "succeeded")
                    self.assertGreaterEqual(int(post.get("attempt") or 0), 2)

            self.assertEqual(calls["n"], 2)
        finally:
            cleanup()

    def test_due_worker_cancels_stale_work_job_after_unbind(self) -> None:
        from cccc.daemon.space.group_space_runtime import process_due_space_jobs
        from cccc.daemon.space.group_space_store import (
            enqueue_space_job,
            get_space_job,
            set_space_binding_unbound,
            set_space_provider_state,
            upsert_space_binding,
        )

        _, cleanup = self._with_home()
        try:
            set_space_provider_state("notebooklm", enabled=True, mode="active", last_error="", touch_health=True)
            upsert_space_binding(
                "g_stale_1",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_stale_old",
                by="user",
                status="bound",
            )
            job, _ = enqueue_space_job(
                group_id="g_stale_1",
                provider="notebooklm",
                remote_space_id="nb_stale_old",
                kind="context_sync",
                payload={"summary": {"tasks": []}},
                idempotency_key="stale-job-1",
            )
            job_id = str(job.get("job_id") or "")
            self.assertTrue(job_id)

            set_space_binding_unbound("g_stale_1", provider="notebooklm", lane="work", by="user")

            with patch("cccc.daemon.space.group_space_runtime.provider_ingest") as provider_ingest:
                tick_result = process_due_space_jobs(limit=20)
            self.assertGreaterEqual(int(tick_result.get("processed") or 0), 1)
            provider_ingest.assert_not_called()

            post = get_space_job(job_id) or {}
            self.assertEqual(str(post.get("state") or ""), "canceled")
        finally:
            cleanup()

    def test_queue_summary_reconciles_stale_running_jobs(self) -> None:
        from cccc.daemon.space.group_space_store import (
            enqueue_space_job,
            get_space_job,
            mark_space_job_running,
            space_queue_summary,
        )

        home, cleanup = self._with_home()
        try:
            job, _ = enqueue_space_job(
                group_id="g_stale_running",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_stale_running",
                kind="context_sync",
                payload={"summary": {"tasks": []}},
                idempotency_key="stale-running-job",
            )
            job_id = str(job.get("job_id") or "")
            self.assertTrue(job_id)
            mark_space_job_running(job_id)

            jobs_path = Path(home) / "state" / "space" / "jobs.json"
            doc = json.loads(jobs_path.read_text(encoding="utf-8"))
            doc["jobs"][job_id]["updated_at"] = "2026-01-01T00:00:00Z"
            jobs_path.write_text(json.dumps(doc), encoding="utf-8")

            summary = space_queue_summary(
                group_id="g_stale_running",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_stale_running",
            )
            self.assertEqual(int(summary.get("running") or 0), 0)
            self.assertEqual(int(summary.get("failed") or 0), 1)
            updated = get_space_job(job_id) or {}
            self.assertEqual(str(updated.get("state") or ""), "failed")
            self.assertEqual(str((updated.get("last_error") or {}).get("code") or ""), "space_job_stale_running")
        finally:
            cleanup()

    def test_space_job_store_reuses_cached_jobs_doc_between_reads(self) -> None:
        from cccc.daemon.space import group_space_store as store
        from cccc.daemon.space.group_space_store import enqueue_space_job, list_due_space_jobs

        _, cleanup = self._with_home()
        try:
            real_read_json = store.read_json
            read_calls = {"n": 0}

            def _counted_read_json(path):
                read_calls["n"] += 1
                return real_read_json(path)

            with patch.object(store, "read_json", side_effect=_counted_read_json):
                job, deduped = enqueue_space_job(
                    group_id="g_cache_1",
                    provider="notebooklm",
                    remote_space_id="nb_cache_1",
                    kind="context_sync",
                    payload={"summary": {"tasks": ["a"]}},
                    idempotency_key="cache-job-1",
                )
                self.assertFalse(deduped)
                self.assertTrue(str(job.get("job_id") or "").strip())

                due_first = list_due_space_jobs(limit=20)
                due_second = list_due_space_jobs(limit=20)

            self.assertEqual(len(due_first), 1)
            self.assertEqual(len(due_second), 1)
            self.assertLessEqual(read_calls["n"], 2)
        finally:
            cleanup()

    def test_space_job_payload_is_stored_out_of_line_and_hydrated_on_read(self) -> None:
        import json
        from pathlib import Path

        from cccc.daemon.space.group_space_store import enqueue_space_job, get_space_job

        home, cleanup = self._with_home()
        try:
            payload = {
                "group_id": "g_payload_1",
                "summary": {"tasks": ["a", "b"], "brief": "x" * 4096},
                "changes": [{"op": "update", "detail": "sync"}],
            }
            job, deduped = enqueue_space_job(
                group_id="g_payload_1",
                provider="notebooklm",
                remote_space_id="nb_payload_1",
                kind="context_sync",
                payload=payload,
                idempotency_key="payload-job-1",
            )
            self.assertFalse(deduped)
            job_id = str(job.get("job_id") or "")
            self.assertTrue(job_id)
            self.assertEqual(job.get("payload"), payload)

            jobs_doc = json.loads(Path(home, "state", "space", "jobs.json").read_text(encoding="utf-8"))
            stored = (((jobs_doc.get("jobs") or {}) if isinstance(jobs_doc, dict) else {}).get(job_id) or {})
            self.assertEqual(stored.get("payload"), {})
            self.assertTrue(str(stored.get("payload_ref") or "").strip())
            self.assertGreater(int(stored.get("payload_bytes") or 0), 0)
            payload_path = Path(home, "state", "space", "job_payloads", str(stored.get("payload_ref") or ""))
            self.assertTrue(payload_path.exists())

            hydrated = get_space_job(job_id) or {}
            self.assertEqual(hydrated.get("payload"), payload)
        finally:
            cleanup()

    def test_compact_space_jobs_storage_drops_old_terminal_jobs(self) -> None:
        import json
        from pathlib import Path

        from cccc.daemon.space.group_space_store import (
            compact_space_jobs_storage,
            enqueue_space_job,
            get_space_job,
            mark_space_job_failed,
            mark_space_job_succeeded,
        )

        _, cleanup = self._with_home()
        try:
            with patch.dict(
                os.environ,
                {
                    "CCCC_SPACE_JOBS_TERMINAL_KEEP": "1",
                    "CCCC_SPACE_JOBS_TERMINAL_MAX_AGE_DAYS": "365",
                },
                clear=False,
            ):
                first, _ = enqueue_space_job(
                    group_id="g_compact_1",
                    provider="notebooklm",
                    remote_space_id="nb_compact_1",
                    kind="context_sync",
                    payload={"summary": {"tasks": ["first"]}},
                    idempotency_key="compact-1",
                )
                kept_terminal_ids: list[str] = []
                for idx in range(20):
                    item, _ = enqueue_space_job(
                        group_id="g_compact_1",
                        provider="notebooklm",
                        remote_space_id="nb_compact_1",
                        kind="context_sync",
                        payload={"summary": {"tasks": [f"keep-{idx}"]}},
                        idempotency_key=f"compact-keep-{idx}",
                    )
                    item_id = str(item.get("job_id") or "")
                    self.assertTrue(item_id)
                    mark_space_job_succeeded(item_id, result={"ok": True, "idx": idx})
                    kept_terminal_ids.append(item_id)
                active, _ = enqueue_space_job(
                    group_id="g_compact_1",
                    provider="notebooklm",
                    remote_space_id="nb_compact_1",
                    kind="context_sync",
                    payload={"summary": {"tasks": ["active"]}},
                    idempotency_key="compact-3",
                )
                first_id = str(first.get("job_id") or "")
                active_id = str(active.get("job_id") or "")
                self.assertTrue(first_id and active_id)

                first_done = mark_space_job_failed(first_id, code="space_failed", message="failed")
                first_payload_ref = str(first_done.get("payload_ref") or "")
                self.assertTrue(first_payload_ref)
                jobs_path = Path(os.environ["CCCC_HOME"], "state", "space", "jobs.json")
                jobs_doc = json.loads(jobs_path.read_text(encoding="utf-8"))
                jobs = jobs_doc.get("jobs") if isinstance(jobs_doc.get("jobs"), dict) else {}
                first_item = jobs.get(first_id) if isinstance(jobs.get(first_id), dict) else None
                if isinstance(first_item, dict):
                    first_item["updated_at"] = "2025-01-01T00:00:00Z"
                    first_item["created_at"] = "2025-01-01T00:00:00Z"
                jobs_path.write_text(json.dumps(jobs_doc, ensure_ascii=False, indent=2), encoding="utf-8")

                stats = compact_space_jobs_storage()
                self.assertGreaterEqual(int(stats.get("dropped_jobs") or 0), 1)
                self.assertIsNone(get_space_job(first_id))
                self.assertIsNotNone(get_space_job(kept_terminal_ids[-1]))
                self.assertIsNotNone(get_space_job(active_id))
                self.assertFalse(Path(os.environ["CCCC_HOME"], "state", "space", "job_payloads", first_payload_ref).exists())
        finally:
            cleanup()

    def test_write_execution_is_serialized_per_remote_space(self) -> None:
        from cccc.daemon.space.group_space_runtime import execute_space_job
        from cccc.daemon.space.group_space_store import enqueue_space_job, set_space_provider_state, upsert_space_binding

        _, cleanup = self._with_home()
        try:
            set_space_provider_state("notebooklm", enabled=True, mode="active", last_error="", touch_health=True)
            upsert_space_binding(
                "g_lock_1",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_lock",
                by="user",
                status="bound",
            )
            upsert_space_binding(
                "g_lock_2",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_lock",
                by="user",
                status="bound",
            )
            job1, _ = enqueue_space_job(
                group_id="g_lock_1",
                provider="notebooklm",
                remote_space_id="nb_lock",
                kind="context_sync",
                payload={"n": 1},
                idempotency_key="lock-1",
            )
            job2, _ = enqueue_space_job(
                group_id="g_lock_2",
                provider="notebooklm",
                remote_space_id="nb_lock",
                kind="resource_ingest",
                payload={"n": 2},
                idempotency_key="lock-2",
            )

            mu = threading.Lock()
            active = {"n": 0, "max": 0}

            def _slow_ingest(provider: str, *, remote_space_id: str, kind: str, payload: dict):
                _ = provider, remote_space_id, kind, payload
                with mu:
                    active["n"] += 1
                    active["max"] = max(active["max"], active["n"])
                time.sleep(0.08)
                with mu:
                    active["n"] -= 1
                return {"ok": True}

            with patch("cccc.daemon.space.group_space_runtime.provider_ingest", side_effect=_slow_ingest):
                t1 = threading.Thread(target=execute_space_job, args=(str(job1.get("job_id") or ""),))
                t2 = threading.Thread(target=execute_space_job, args=(str(job2.get("job_id") or ""),))
                t1.start()
                t2.start()
                t1.join()
                t2.join()

            self.assertEqual(active["max"], 1)
        finally:
            cleanup()

    def test_write_execution_allows_parallelism_across_different_remotes(self) -> None:
        from cccc.daemon.space.group_space_runtime import execute_space_job
        from cccc.daemon.space.group_space_store import enqueue_space_job, set_space_provider_state, upsert_space_binding

        _, cleanup = self._with_home()
        try:
            set_space_provider_state("notebooklm", enabled=True, mode="active", last_error="", touch_health=True)
            upsert_space_binding(
                "g_lock_3",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_lock_a",
                by="user",
                status="bound",
            )
            upsert_space_binding(
                "g_lock_4",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_lock_b",
                by="user",
                status="bound",
            )
            job1, _ = enqueue_space_job(
                group_id="g_lock_3",
                provider="notebooklm",
                remote_space_id="nb_lock_a",
                kind="context_sync",
                payload={"n": 1},
                idempotency_key="lock-a",
            )
            job2, _ = enqueue_space_job(
                group_id="g_lock_4",
                provider="notebooklm",
                remote_space_id="nb_lock_b",
                kind="resource_ingest",
                payload={"n": 2},
                idempotency_key="lock-b",
            )

            mu = threading.Lock()
            active = {"n": 0, "max": 0}

            def _slow_ingest(provider: str, *, remote_space_id: str, kind: str, payload: dict):
                _ = provider, remote_space_id, kind, payload
                with mu:
                    active["n"] += 1
                    active["max"] = max(active["max"], active["n"])
                time.sleep(0.08)
                with mu:
                    active["n"] -= 1
                return {"ok": True}

            with patch("cccc.daemon.space.group_space_runtime.provider_ingest", side_effect=_slow_ingest):
                t1 = threading.Thread(target=execute_space_job, args=(str(job1.get("job_id") or ""),))
                t2 = threading.Thread(target=execute_space_job, args=(str(job2.get("job_id") or ""),))
                t1.start()
                t2.start()
                t1.join()
                t2.join()

            self.assertGreaterEqual(active["max"], 2)
        finally:
            cleanup()

    def test_global_provider_write_cap_can_serialize_different_remotes(self) -> None:
        from cccc.daemon.space.group_space_runtime import execute_space_job
        from cccc.daemon.space.group_space_store import enqueue_space_job, set_space_provider_state, upsert_space_binding

        _, cleanup = self._with_home()
        try:
            set_space_provider_state("notebooklm", enabled=True, mode="active", last_error="", touch_health=True)
            upsert_space_binding(
                "g_cap_1",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_cap_a",
                by="user",
                status="bound",
            )
            upsert_space_binding(
                "g_cap_2",
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_cap_b",
                by="user",
                status="bound",
            )
            job1, _ = enqueue_space_job(
                group_id="g_cap_1",
                provider="notebooklm",
                remote_space_id="nb_cap_a",
                kind="context_sync",
                payload={"n": 1},
                idempotency_key="cap-a",
            )
            job2, _ = enqueue_space_job(
                group_id="g_cap_2",
                provider="notebooklm",
                remote_space_id="nb_cap_b",
                kind="resource_ingest",
                payload={"n": 2},
                idempotency_key="cap-b",
            )

            mu = threading.Lock()
            active = {"n": 0, "max": 0}

            def _slow_ingest(provider: str, *, remote_space_id: str, kind: str, payload: dict):
                _ = provider, remote_space_id, kind, payload
                with mu:
                    active["n"] += 1
                    active["max"] = max(active["max"], active["n"])
                time.sleep(0.08)
                with mu:
                    active["n"] -= 1
                return {"ok": True}

            sem = threading.BoundedSemaphore(1)
            with patch("cccc.daemon.space.group_space_runtime._provider_write_semaphore", return_value=sem):
                with patch("cccc.daemon.space.group_space_runtime.provider_ingest", side_effect=_slow_ingest):
                    t1 = threading.Thread(target=execute_space_job, args=(str(job1.get("job_id") or ""),))
                    t2 = threading.Thread(target=execute_space_job, args=(str(job2.get("job_id") or ""),))
                    t1.start()
                    t2.start()
                    t1.join()
                    t2.join()

            self.assertEqual(active["max"], 1)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
