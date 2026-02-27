from __future__ import annotations

import os
import tempfile
import unittest

from fastapi.testclient import TestClient
from unittest.mock import patch


class TestWebGroupSpaceApi(unittest.TestCase):
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

    def _create_group(self, title: str = "web-space") -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title=title, topic="")
        return group.group_id

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

    def test_group_space_endpoints_work_in_normal_mode(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        cleanup_stub = self._with_env("CCCC_NOTEBOOKLM_STUB", "1")
        try:
            gid = self._create_group("web-space-normal")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = TestClient(create_app())

                status = client.get(f"/api/v1/groups/{gid}/space/status")
                self.assertEqual(status.status_code, 200)
                status_body = status.json()
                self.assertTrue(status_body.get("ok"))

                spaces = client.get(f"/api/v1/groups/{gid}/space/spaces?provider=notebooklm")
                self.assertEqual(spaces.status_code, 200)
                spaces_body = spaces.json()
                self.assertTrue(spaces_body.get("ok"))
                spaces_list = ((spaces_body.get("result") or {}).get("spaces") or [])
                self.assertIsInstance(spaces_list, list)

                bind = client.post(
                    f"/api/v1/groups/{gid}/space/bind",
                    json={
                        "by": "user",
                        "provider": "notebooklm",
                        "action": "bind",
                        "remote_space_id": "nb_web_1",
                    },
                )
                self.assertEqual(bind.status_code, 200)
                bind_body = bind.json()
                self.assertTrue(bind_body.get("ok"))
                binding = ((bind_body.get("result") or {}).get("binding") or {})
                self.assertEqual(str(binding.get("status") or ""), "bound")

                query = client.post(
                    f"/api/v1/groups/{gid}/space/query",
                    json={"provider": "notebooklm", "query": "status?", "options": {}},
                )
                self.assertEqual(query.status_code, 200)
                query_body = query.json()
                self.assertTrue(query_body.get("ok"))

                sources = client.get(f"/api/v1/groups/{gid}/space/sources?provider=notebooklm")
                self.assertEqual(sources.status_code, 200)
                self.assertTrue(sources.json().get("ok"))

                source_refresh = client.post(
                    f"/api/v1/groups/{gid}/space/sources",
                    json={
                        "by": "user",
                        "provider": "notebooklm",
                        "action": "refresh",
                        "source_id": "src_web_1",
                    },
                )
                self.assertEqual(source_refresh.status_code, 200)
                self.assertTrue(source_refresh.json().get("ok"))

                artifacts = client.get(f"/api/v1/groups/{gid}/space/artifacts?provider=notebooklm")
                self.assertEqual(artifacts.status_code, 200)
                self.assertTrue(artifacts.json().get("ok"))

                artifact_generate = client.post(
                    f"/api/v1/groups/{gid}/space/artifacts",
                    json={
                        "by": "user",
                        "provider": "notebooklm",
                        "action": "generate",
                        "kind": "report",
                        "options": {"language": "en"},
                        "wait": False,
                        "save_to_space": False,
                    },
                )
                self.assertEqual(artifact_generate.status_code, 200)
                self.assertTrue(artifact_generate.json().get("ok"))

                ingest = client.post(
                    f"/api/v1/groups/{gid}/space/ingest",
                    json={
                        "by": "user",
                        "provider": "notebooklm",
                        "kind": "context_sync",
                        "payload": {"vision": "v0.5"},
                        "idempotency_key": "web-space-1",
                    },
                )
                self.assertEqual(ingest.status_code, 200)
                ingest_body = ingest.json()
                self.assertTrue(ingest_body.get("ok"))

                jobs = client.get(f"/api/v1/groups/{gid}/space/jobs?provider=notebooklm&limit=20")
                self.assertEqual(jobs.status_code, 200)
                jobs_body = jobs.json()
                self.assertTrue(jobs_body.get("ok"))
                jobs_list = ((jobs_body.get("result") or {}).get("jobs") or [])
                self.assertIsInstance(jobs_list, list)

                sync = client.post(
                    f"/api/v1/groups/{gid}/space/sync",
                    json={
                        "by": "user",
                        "provider": "notebooklm",
                        "action": "status",
                        "force": False,
                    },
                )
                self.assertEqual(sync.status_code, 200)
                sync_body = sync.json()
                self.assertTrue(sync_body.get("ok"))

                credential_status = client.get("/api/v1/space/providers/notebooklm/credential?by=user")
                self.assertEqual(credential_status.status_code, 200)
                self.assertTrue(credential_status.json().get("ok"))

                credential_update = client.post(
                    "/api/v1/space/providers/notebooklm/credential",
                    json={
                        "by": "user",
                        "auth_json": '{"cookies":[{"name":"SID","value":"x","domain":".google.com"}]}',
                        "clear": False,
                    },
                )
                self.assertEqual(credential_update.status_code, 200)
                self.assertTrue(credential_update.json().get("ok"))

                with patch(
                    "cccc.daemon.space.group_space_ops.notebooklm_health_check",
                    return_value={"provider": "notebooklm", "enabled": True, "compatible": True, "reason": "ok"},
                ):
                    health = client.post("/api/v1/space/providers/notebooklm/health?by=user")
                self.assertEqual(health.status_code, 200)
                health_body = health.json()
                self.assertTrue(health_body.get("ok"))
                self.assertEqual(bool((health_body.get("result") or {}).get("healthy")), True)

                with patch(
                    "cccc.daemon.space.group_space_ops.start_notebooklm_auth_flow",
                    return_value={
                        "provider": "notebooklm",
                        "state": "running",
                        "phase": "waiting_user_login",
                        "session_id": "nbl_auth_web",
                    },
                ), patch(
                    "cccc.daemon.space.group_space_ops.get_notebooklm_auth_flow_status",
                    return_value={"provider": "notebooklm", "state": "running", "phase": "waiting_user_login"},
                ), patch(
                    "cccc.daemon.space.group_space_ops.cancel_notebooklm_auth_flow",
                    return_value={"provider": "notebooklm", "state": "running", "phase": "canceling"},
                ):
                    auth_start = client.post(
                        "/api/v1/space/providers/notebooklm/auth",
                        json={"by": "user", "action": "start", "timeout_seconds": 120},
                    )
                    self.assertEqual(auth_start.status_code, 200)
                    self.assertTrue(auth_start.json().get("ok"))

                    auth_status = client.get("/api/v1/space/providers/notebooklm/auth?by=user")
                    self.assertEqual(auth_status.status_code, 200)
                    self.assertTrue(auth_status.json().get("ok"))

                    auth_cancel = client.post(
                        "/api/v1/space/providers/notebooklm/auth",
                        json={"by": "user", "action": "cancel"},
                    )
                    self.assertEqual(auth_cancel.status_code, 200)
                    self.assertTrue(auth_cancel.json().get("ok"))
        finally:
            cleanup_stub()
            cleanup()

    def test_group_space_write_endpoints_blocked_in_exhibit_mode(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        cleanup_mode = self._with_env("CCCC_WEB_MODE", "exhibit")
        try:
            gid = self._create_group("web-space-exhibit")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = TestClient(create_app())

                write_resp = client.post(
                    f"/api/v1/groups/{gid}/space/bind",
                    json={
                        "by": "user",
                        "provider": "notebooklm",
                        "action": "bind",
                        "remote_space_id": "nb_exhibit",
                    },
                )
                self.assertEqual(write_resp.status_code, 403)
                body = write_resp.json()
                self.assertFalse(body.get("ok"))
                self.assertEqual(str((body.get("error") or {}).get("code") or ""), "read_only")

                sync_write_resp = client.post(
                    f"/api/v1/groups/{gid}/space/sync",
                    json={"by": "user", "provider": "notebooklm", "action": "run", "force": True},
                )
                self.assertEqual(sync_write_resp.status_code, 403)
                self.assertEqual(str((sync_write_resp.json().get("error") or {}).get("code") or ""), "read_only")

                sources_read_resp = client.get(f"/api/v1/groups/{gid}/space/sources?provider=notebooklm")
                self.assertEqual(sources_read_resp.status_code, 200)
                self.assertFalse(sources_read_resp.json().get("ok"))
                self.assertEqual(
                    str((sources_read_resp.json().get("error") or {}).get("code") or ""),
                    "space_binding_missing",
                )

                sources_write_resp = client.post(
                    f"/api/v1/groups/{gid}/space/sources",
                    json={
                        "by": "user",
                        "provider": "notebooklm",
                        "action": "delete",
                        "source_id": "src_ro_1",
                    },
                )
                self.assertEqual(sources_write_resp.status_code, 403)
                self.assertEqual(str((sources_write_resp.json().get("error") or {}).get("code") or ""), "read_only")

                artifacts_read_resp = client.get(f"/api/v1/groups/{gid}/space/artifacts?provider=notebooklm")
                self.assertEqual(artifacts_read_resp.status_code, 200)
                self.assertFalse(artifacts_read_resp.json().get("ok"))
                self.assertEqual(
                    str((artifacts_read_resp.json().get("error") or {}).get("code") or ""),
                    "space_binding_missing",
                )

                artifacts_write_resp = client.post(
                    f"/api/v1/groups/{gid}/space/artifacts",
                    json={
                        "by": "user",
                        "provider": "notebooklm",
                        "action": "generate",
                        "kind": "report",
                        "options": {},
                    },
                )
                self.assertEqual(artifacts_write_resp.status_code, 403)
                self.assertEqual(str((artifacts_write_resp.json().get("error") or {}).get("code") or ""), "read_only")

                cred_write_resp = client.post(
                    "/api/v1/space/providers/notebooklm/credential",
                    json={"by": "user", "auth_json": "{}", "clear": False},
                )
                self.assertEqual(cred_write_resp.status_code, 403)
                self.assertEqual(str((cred_write_resp.json().get("error") or {}).get("code") or ""), "read_only")

                health_write_resp = client.post("/api/v1/space/providers/notebooklm/health?by=user")
                self.assertEqual(health_write_resp.status_code, 403)
                self.assertEqual(str((health_write_resp.json().get("error") or {}).get("code") or ""), "read_only")

                auth_status_resp = client.get("/api/v1/space/providers/notebooklm/auth?by=user")
                self.assertEqual(auth_status_resp.status_code, 200)
                self.assertTrue(auth_status_resp.json().get("ok"))

                auth_start_resp = client.post(
                    "/api/v1/space/providers/notebooklm/auth",
                    json={"by": "user", "action": "start"},
                )
                self.assertEqual(auth_start_resp.status_code, 403)
                self.assertEqual(str((auth_start_resp.json().get("error") or {}).get("code") or ""), "read_only")

                auth_cancel_resp = client.post(
                    "/api/v1/space/providers/notebooklm/auth",
                    json={"by": "user", "action": "cancel"},
                )
                self.assertEqual(auth_cancel_resp.status_code, 403)
                self.assertEqual(str((auth_cancel_resp.json().get("error") or {}).get("code") or ""), "read_only")
        finally:
            cleanup_mode()
            cleanup()


if __name__ == "__main__":
    unittest.main()
