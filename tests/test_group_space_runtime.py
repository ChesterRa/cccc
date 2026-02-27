from __future__ import annotations

import os
import tempfile
import threading
import time
import unittest
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

    def test_write_execution_is_serialized_per_remote_space(self) -> None:
        from cccc.daemon.space.group_space_runtime import execute_space_job
        from cccc.daemon.space.group_space_store import enqueue_space_job, set_space_provider_state

        _, cleanup = self._with_home()
        try:
            set_space_provider_state("notebooklm", enabled=True, mode="active", last_error="", touch_health=True)
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
        from cccc.daemon.space.group_space_store import enqueue_space_job, set_space_provider_state

        _, cleanup = self._with_home()
        try:
            set_space_provider_state("notebooklm", enabled=True, mode="active", last_error="", touch_health=True)
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
        from cccc.daemon.space.group_space_store import enqueue_space_job, set_space_provider_state

        _, cleanup = self._with_home()
        try:
            set_space_provider_state("notebooklm", enabled=True, mode="active", last_error="", touch_health=True)
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
