import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebGroupsLocalProjection(unittest.TestCase):
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

        return cleanup

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def test_groups_route_reads_local_projection_without_daemon(self) -> None:
        cleanup = self._with_home()
        try:
            with patch(
                "cccc.ports.web.routes.groups._read_groups_local",
                return_value={"ok": True, "result": {"groups": [{"group_id": "g1", "title": "T"}], "registry_health": {}}},
            ), patch("cccc.ports.web.app.call_daemon", side_effect=AssertionError("daemon should not be called")):
                with self._client() as client:
                    resp = client.get("/api/v1/groups")
                    self.assertEqual(resp.status_code, 200)
                    data = resp.json()
                    self.assertEqual(data["result"]["groups"][0]["group_id"], "g1")
        finally:
            cleanup()

    def test_groups_route_marks_headless_codex_group_running_from_local_supervisor(self) -> None:
        cleanup = self._with_home()
        try:
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import create_group, load_group
            from cccc.kernel.registry import load_registry
            from cccc.daemon.runner_state_ops import headless_state_path
            from cccc.util.fs import atomic_write_json

            reg = load_registry()
            gid = create_group(reg, title="codex-running", topic="").group_id
            group = load_group(gid)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", title="Peer 1", runtime="codex", runner="headless")  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            atomic_write_json(
                headless_state_path(gid, "peer1"),
                {
                    "v": 1,
                    "kind": "headless",
                    "runtime": "codex",
                    "group_id": gid,
                    "actor_id": "peer1",
                    "pid": os.getpid(),
                    "status": "idle",
                },
            )
            with self._client() as client:
                resp = client.get("/api/v1/groups")

            self.assertEqual(resp.status_code, 200)
            groups = resp.json()["result"]["groups"]
            match = next(item for item in groups if str(item.get("group_id") or "") == gid)
            self.assertTrue(bool(match.get("running")))
            self.assertTrue(bool(((match.get("runtime_status") or {}).get("runtime_running"))))
        finally:
            cleanup()

    def test_groups_route_marks_web_model_marker_running(self) -> None:
        cleanup = self._with_home()
        try:
            from cccc.daemon.runner_state_ops import write_headless_state
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import create_group, load_group
            from cccc.kernel.registry import load_registry

            reg = load_registry()
            gid = create_group(reg, title="web-model-running", topic="").group_id
            group = load_group(gid)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="webpeer", title="Web Peer", runtime="web_model", runner="headless")  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]
            write_headless_state(gid, "webpeer")

            with self._client() as client:
                resp = client.get("/api/v1/groups")

            self.assertEqual(resp.status_code, 200)
            groups = resp.json()["result"]["groups"]
            match = next(item for item in groups if str(item.get("group_id") or "") == gid)
            self.assertTrue(bool(match.get("running")))
            self.assertTrue(bool(((match.get("runtime_status") or {}).get("runtime_running"))))
        finally:
            cleanup()

    def test_groups_route_prefers_group_level_supervisor_running_signal(self) -> None:
        cleanup = self._with_home()
        try:
            from cccc.kernel.group import create_group
            from cccc.kernel.registry import load_registry

            reg = load_registry()
            gid = create_group(reg, title="group-running-supervisor", topic="").group_id

            with patch("cccc.ports.web.routes.groups.codex_app_supervisor.group_running", return_value=True):
                with self._client() as client:
                    resp = client.get("/api/v1/groups")

            self.assertEqual(resp.status_code, 200)
            groups = resp.json()["result"]["groups"]
            match = next(item for item in groups if str(item.get("group_id") or "") == gid)
            self.assertTrue(bool(match.get("running")))
            self.assertTrue(bool(((match.get("runtime_status") or {}).get("runtime_running"))))
        finally:
            cleanup()

    def test_groups_route_treats_codex_pty_actor_as_internal_headless_on_restart(self) -> None:
        cleanup = self._with_home()
        try:
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import create_group, load_group
            from cccc.kernel.registry import load_registry
            from cccc.daemon.runner_state_ops import headless_state_path
            from cccc.util.fs import atomic_write_json

            reg = load_registry()
            gid = create_group(reg, title="codex-pty-restarts-running", topic="").group_id
            group = load_group(gid)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", title="Peer 1", runtime="codex", runner="pty")  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            atomic_write_json(
                headless_state_path(gid, "peer1"),
                {
                    "v": 1,
                    "kind": "headless",
                    "runtime": "codex",
                    "group_id": gid,
                    "actor_id": "peer1",
                    "pid": os.getpid(),
                    "status": "idle",
                },
            )
            with self._client() as client:
                resp = client.get("/api/v1/groups")

            self.assertEqual(resp.status_code, 200)
            groups = resp.json()["result"]["groups"]
            match = next(item for item in groups if str(item.get("group_id") or "") == gid)
            self.assertTrue(bool(match.get("running")))
        finally:
            cleanup()
