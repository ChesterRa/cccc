import os
import tempfile
import threading
import time
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebContextRoutesCache(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        old_mode = os.environ.get("CCCC_WEB_MODE")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODE", None)
            else:
                os.environ["CCCC_WEB_MODE"] = old_mode

        return td, cleanup

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        return create_group(reg, title="context-cache-test", topic="").group_id

    def test_summary_context_get_reads_local_snapshot_without_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            summary_payload = {"coordination": {"tasks": []}, "meta": {"summary_snapshot": {"state": "hit"}}}

            with patch(
                "cccc.ports.web.routes.groups._read_context_summary_local",
                return_value={"ok": True, "result": summary_payload},
            ) as mock_local, patch(
                "cccc.ports.web.app.call_daemon",
                side_effect=AssertionError("summary context should not call daemon"),
            ):
                with self._client() as client:
                    resp = client.get(f"/api/v1/groups/{group_id}/context")

            self.assertEqual(resp.status_code, 200)
            self.assertEqual(resp.json()["result"]["meta"]["summary_snapshot"]["state"], "hit")
            mock_local.assert_called_once_with(group_id)
        finally:
            cleanup()

    def test_normal_mode_full_context_get_uses_inflight_without_ttl(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            barrier = threading.Barrier(3)

            def fake_call_daemon(req: dict):
                nonlocal context_get_calls
                if str(req.get("op") or "") == "context_get":
                    with call_lock:
                        context_get_calls += 1
                    time.sleep(0.12)
                    return {"ok": True, "result": {"coordination": {"tasks": []}, "agent_states": [], "meta": {}}}
                return {"ok": True, "result": {}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    path = f"/api/v1/groups/{group_id}/context?detail=full"

                    def do_get() -> int:
                        barrier.wait(timeout=2)
                        resp = client.get(path)
                        return resp.status_code

                    with ThreadPoolExecutor(max_workers=2) as executor:
                        fut1 = executor.submit(do_get)
                        fut2 = executor.submit(do_get)
                        barrier.wait(timeout=2)
                        self.assertEqual(fut1.result(timeout=3), 200)
                        self.assertEqual(fut2.result(timeout=3), 200)

                    self.assertEqual(context_get_calls, 1)

                    follow_up = client.get(path)
                    self.assertEqual(follow_up.status_code, 200)
                    self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()

    def test_context_sync_invalidates_server_inflight(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            first_read_started = threading.Event()
            first_read_release = threading.Event()

            def fake_call_daemon(req: dict):
                nonlocal context_get_calls
                op = str(req.get("op") or "")
                if op == "context_get":
                    with call_lock:
                        context_get_calls += 1
                        current = context_get_calls
                    if current == 1:
                        first_read_started.set()
                        first_read_release.wait(timeout=2)
                        return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "stale"}}}
                    return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "fresh"}}}
                if op == "context_sync":
                    return {"ok": True, "result": {"version": "fresh"}}
                return {"ok": True, "result": {}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    path = f"/api/v1/groups/{group_id}/context?detail=full"

                    with ThreadPoolExecutor(max_workers=1) as executor:
                        stale_future = executor.submit(client.get, path)
                        self.assertTrue(first_read_started.wait(timeout=2))

                        sync_resp = client.post(path, json={"ops": [{"op": "coordination.brief.update", "current_focus": "fresh"}], "by": "user"})
                        self.assertEqual(sync_resp.status_code, 200)

                        fresh_resp = client.get(path)
                        self.assertEqual(fresh_resp.status_code, 200)
                        self.assertEqual(fresh_resp.json()["result"]["meta"]["version"], "fresh")

                        first_read_release.set()
                        stale_resp = stale_future.result(timeout=3)
                        self.assertEqual(stale_resp.status_code, 200)
                        self.assertEqual(stale_resp.json()["result"]["meta"]["version"], "stale")

                    self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()

    def test_project_md_put_invalidates_server_inflight(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            first_read_started = threading.Event()
            first_read_release = threading.Event()

            from cccc.kernel.group import attach_scope_to_group, load_group
            from cccc.kernel.registry import load_registry
            from cccc.kernel.scope import detect_scope

            with tempfile.TemporaryDirectory() as scope_root:
                reg = load_registry()
                group = load_group(group_id)
                assert group is not None
                attach_scope_to_group(reg, group, detect_scope(Path(scope_root)))

                def fake_call_daemon(req: dict):
                    nonlocal context_get_calls
                    op = str(req.get("op") or "")
                    if op == "context_get":
                        with call_lock:
                            context_get_calls += 1
                            current = context_get_calls
                        if current == 1:
                            first_read_started.set()
                            first_read_release.wait(timeout=2)
                            return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "stale"}}}
                        return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "fresh"}}}
                    if op == "context_sync":
                        return {"ok": True, "result": {"version": "fresh"}}
                    return {"ok": True, "result": {}}

                with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                    with self._client() as client:
                        path = f"/api/v1/groups/{group_id}/context?detail=full"

                        with ThreadPoolExecutor(max_workers=1) as executor:
                            stale_future = executor.submit(client.get, path)
                            self.assertTrue(first_read_started.wait(timeout=2))

                            put_resp = client.put(
                                f"/api/v1/groups/{group_id}/project_md",
                                json={"content": "# Fresh PROJECT"},
                            )
                            self.assertEqual(put_resp.status_code, 200)
                            self.assertEqual(put_resp.json()["result"]["content"], "# Fresh PROJECT")

                            fresh_resp = client.get(path)
                            self.assertEqual(fresh_resp.status_code, 200)
                            self.assertEqual(fresh_resp.json()["result"]["meta"]["version"], "fresh")

                            first_read_release.set()
                            stale_resp = stale_future.result(timeout=3)
                            self.assertEqual(stale_resp.status_code, 200)
                            self.assertEqual(stale_resp.json()["result"]["meta"]["version"], "stale")

                        self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()

    def test_fresh_context_get_bypasses_stale_server_inflight(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            first_read_started = threading.Event()
            first_read_release = threading.Event()

            def fake_call_daemon(req: dict):
                nonlocal context_get_calls
                op = str(req.get("op") or "")
                if op == "context_get":
                    with call_lock:
                        context_get_calls += 1
                        current = context_get_calls
                    if current == 1:
                        first_read_started.set()
                        first_read_release.wait(timeout=2)
                        return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "stale"}}}
                    return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "fresh"}}}
                return {"ok": True, "result": {}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    path = f"/api/v1/groups/{group_id}/context?detail=full"

                    with ThreadPoolExecutor(max_workers=1) as executor:
                        stale_future = executor.submit(client.get, path)
                        self.assertTrue(first_read_started.wait(timeout=2))

                        fresh_resp = client.get(f"{path}&fresh=1")
                        self.assertEqual(fresh_resp.status_code, 200)
                        self.assertEqual(fresh_resp.json()["result"]["meta"]["version"], "fresh")

                        first_read_release.set()
                        stale_resp = stale_future.result(timeout=3)
                        self.assertEqual(stale_resp.status_code, 200)
                        self.assertEqual(stale_resp.json()["result"]["meta"]["version"], "stale")

                    self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()

    def test_actor_delete_invalidates_server_inflight_context_reads(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            first_read_started = threading.Event()
            first_read_release = threading.Event()

            def fake_call_daemon(req: dict):
                nonlocal context_get_calls
                op = str(req.get("op") or "")
                if op == "context_get":
                    with call_lock:
                        context_get_calls += 1
                        current = context_get_calls
                    if current == 1:
                        first_read_started.set()
                        first_read_release.wait(timeout=2)
                        return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "stale"}}}
                    return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "fresh"}}}
                if op == "actor_remove":
                    return {"ok": True, "result": {"actor_id": "peer-1"}}
                return {"ok": True, "result": {}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    path = f"/api/v1/groups/{group_id}/context?detail=full"

                    with ThreadPoolExecutor(max_workers=1) as executor:
                        stale_future = executor.submit(client.get, path)
                        self.assertTrue(first_read_started.wait(timeout=2))

                        delete_resp = client.delete(f"/api/v1/groups/{group_id}/actors/peer-1")
                        self.assertEqual(delete_resp.status_code, 200)

                        fresh_resp = client.get(path)
                        self.assertEqual(fresh_resp.status_code, 200)
                        self.assertEqual(fresh_resp.json()["result"]["meta"]["version"], "fresh")

                        first_read_release.set()
                        stale_resp = stale_future.result(timeout=3)
                        self.assertEqual(stale_resp.status_code, 200)
                        self.assertEqual(stale_resp.json()["result"]["meta"]["version"], "stale")

                    self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()
