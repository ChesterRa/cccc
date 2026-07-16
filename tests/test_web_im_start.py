import os
import hashlib
import json
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import AsyncMock, patch

from fastapi.testclient import TestClient


class _AliveProc:
    def __init__(self, pid: int = 4321) -> None:
        self.pid = pid

    def poll(self) -> None:
        return None

    def wait(self, timeout: float | None = None) -> int:
        _ = timeout
        return 0


class _DummyLockFile:
    def close(self) -> None:
        return None


class _FakeBridge:
    def __init__(self, *args, **kwargs) -> None:
        self.args = args
        self.kwargs = kwargs

    def start(self) -> bool:
        return True

    def run_forever(self) -> None:
        return None


class TestWebImStart(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = os.path.realpath(td_ctx.__enter__())
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def _create_group(self, title: str = "im-start") -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title=title, topic="")
        return group.group_id

    def test_im_start_uses_detached_child_process_defaults(self) -> None:
        from cccc.ports.web.app import create_app

        home, cleanup = self._with_home()
        try:
            gid = self._create_group("im-start-route")
            with TestClient(create_app()) as client:
                set_resp = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "telegram",
                        "token_env": "TELEGRAM_BOT_TOKEN",
                    },
                )
                self.assertEqual(set_resp.status_code, 200)
                self.assertTrue(bool(set_resp.json().get("ok")))

                with patch("subprocess.Popen", return_value=_AliveProc()) as popen:
                    start_resp = client.post("/api/im/start", json={"group_id": gid})

                self.assertEqual(start_resp.status_code, 200)
                payload = start_resp.json()
                self.assertTrue(bool(payload.get("ok")))

            self.assertIsNotNone(popen.call_args)
            argv = list(popen.call_args.args[0])
            kwargs = dict(popen.call_args.kwargs)

            self.assertEqual(argv, [sys.executable, "-m", "cccc.ports.im", gid, "telegram"])
            self.assertIs(kwargs.get("stdin"), subprocess.DEVNULL)
            self.assertTrue(bool(kwargs.get("close_fds")))
            self.assertEqual(str(kwargs.get("cwd") or ""), home)
            self.assertIs(kwargs.get("stdout"), kwargs.get("stderr"))
            if os.name == "nt":
                self.assertIn("creationflags", kwargs)
                self.assertNotIn("start_new_session", kwargs)
            else:
                self.assertTrue(bool(kwargs.get("start_new_session")))

            pid_path = os.path.join(home, "groups", gid, "state", "im_bridge.pid")
            self.assertTrue(os.path.exists(pid_path))
            with open(pid_path, "r", encoding="utf-8") as fh:
                self.assertEqual(fh.read().strip(), "4321")
        finally:
            cleanup()


    def test_im_start_wecom_injects_credentials_into_env(self) -> None:
        from cccc.ports.web.app import create_app

        home, cleanup = self._with_home()
        try:
            gid = self._create_group("im-start-wecom")
            with TestClient(create_app()) as client:
                set_resp = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "wecom",
                        "wecom_bot_id": "corp123",
                        "wecom_secret": "sec456",
                    },
                )
                self.assertEqual(set_resp.status_code, 200)
                self.assertTrue(bool(set_resp.json().get("ok")))

                with patch("subprocess.Popen", return_value=_AliveProc()) as popen:
                    start_resp = client.post("/api/im/start", json={"group_id": gid})

                self.assertEqual(start_resp.status_code, 200)
                self.assertTrue(bool(start_resp.json().get("ok")))

            self.assertIsNotNone(popen.call_args)
            kwargs = dict(popen.call_args.kwargs)
            child_env = kwargs.get("env") or {}
            self.assertEqual(child_env.get("WECOM_BOT_ID"), "corp123")
            self.assertEqual(child_env.get("WECOM_SECRET"), "sec456")

            argv = list(popen.call_args.args[0])
            self.assertEqual(argv, [sys.executable, "-m", "cccc.ports.im", gid, "wecom"])
        finally:
            cleanup()

    def test_im_start_drops_inherited_ca_bundle_env(self) -> None:
        from cccc.ports.web.app import create_app

        old_ssl_cert_file = os.environ.get("SSL_CERT_FILE")
        old_requests_ca_bundle = os.environ.get("REQUESTS_CA_BUNDLE")
        old_curl_ca_bundle = os.environ.get("CURL_CA_BUNDLE")
        home, cleanup = self._with_home()
        try:
            os.environ["SSL_CERT_FILE"] = "/bad/emsdk/certifi/cacert.pem"
            os.environ["REQUESTS_CA_BUNDLE"] = "/bad/requests.pem"
            os.environ["CURL_CA_BUNDLE"] = "/bad/curl.pem"
            gid = self._create_group("im-start-env")
            with TestClient(create_app()) as client:
                set_resp = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "dingtalk",
                        "dingtalk_app_key": "app-key",
                        "dingtalk_app_secret": "app-secret",
                        "dingtalk_robot_code": "robot-code",
                    },
                )
                self.assertEqual(set_resp.status_code, 200)
                self.assertTrue(bool(set_resp.json().get("ok")))

                with patch("subprocess.Popen", return_value=_AliveProc()) as popen:
                    start_resp = client.post("/api/im/start", json={"group_id": gid})

                self.assertEqual(start_resp.status_code, 200)
                self.assertTrue(bool(start_resp.json().get("ok")))

            self.assertIsNotNone(popen.call_args)
            child_env = dict(popen.call_args.kwargs).get("env") or {}
            self.assertNotIn("SSL_CERT_FILE", child_env)
            self.assertNotIn("REQUESTS_CA_BUNDLE", child_env)
            self.assertNotIn("CURL_CA_BUNDLE", child_env)
            self.assertEqual(child_env.get("DINGTALK_APP_KEY"), "app-key")
            self.assertEqual(child_env.get("DINGTALK_APP_SECRET"), "app-secret")
            self.assertEqual(child_env.get("DINGTALK_ROBOT_CODE"), "robot-code")
        finally:
            for key, value in (
                ("SSL_CERT_FILE", old_ssl_cert_file),
                ("REQUESTS_CA_BUNDLE", old_requests_ca_bundle),
                ("CURL_CA_BUNDLE", old_curl_ca_bundle),
            ):
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
            cleanup()

    def test_start_bridge_wecom_lock_uses_bot_id_identity(self) -> None:
        from cccc.ports.im.bridge import start_bridge
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("bridge-lock-wecom")
            group = load_group(gid)
            assert group is not None
            group.doc["im"] = {
                "platform": "wecom",
                "wecom_bot_id": "corp123",
                "wecom_secret": "sec456",
            }
            group.save()

            lock_paths: list[str] = []

            def fake_acquire(lock_path):
                lock_paths.append(str(lock_path))
                return _DummyLockFile()

            with patch("cccc.ports.im.bridge._acquire_singleton_lock", side_effect=fake_acquire):
                with patch("cccc.ports.im.bridge.IMBridge", _FakeBridge):
                    start_bridge(gid, "wecom")

            self.assertGreaterEqual(len(lock_paths), 2)
            token_fingerprint = hashlib.sha256("wecom|bot_id=corp123".encode("utf-8")).hexdigest()[:12]
            self.assertTrue(lock_paths[0].endswith(f"im_bridge_wecom_{token_fingerprint}.lock"))
        finally:
            cleanup()

    def test_im_weixin_login_start_returns_qr_url_from_sdk(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("im-weixin-login-start")

            with TestClient(create_app()) as client:
                set_resp = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "weixin",
                    },
                )
                self.assertEqual(set_resp.status_code, 200)
                self.assertTrue(bool(set_resp.json().get("ok")))

                class _FakeApi:
                    async def get_qr_code(self, base_url: str):
                        self.base_url = base_url
                        return {
                            "qrcode_img_content": "https://example.test/qr",
                            "qrcode": "qr-token-1",
                        }

                async def _fake_load_credentials(path: Path | None = None):
                    _ = path
                    return None

                fake_auth = types.ModuleType("wechatbot.auth")
                fake_auth.FIXED_QR_BASE_URL = "https://ilinkai.weixin.qq.com"
                fake_auth.load_credentials = _fake_load_credentials
                fake_protocol = types.ModuleType("wechatbot.protocol")
                fake_protocol.ILinkApi = _FakeApi
                fake_pkg = types.ModuleType("wechatbot")
                fake_pkg.__path__ = []  # mark as package

                with patch.dict(sys.modules, {
                    "wechatbot": fake_pkg,
                    "wechatbot.auth": fake_auth,
                    "wechatbot.protocol": fake_protocol,
                }):
                    start_resp = client.post("/api/im/weixin/login/start", json={"group_id": gid})

                self.assertEqual(start_resp.status_code, 200)
                payload = start_resp.json()
                self.assertTrue(bool(payload.get("ok")))
                result = payload.get("result") or {}
                self.assertEqual(result.get("status"), "waiting_scan")
                self.assertEqual(result.get("qrcode_url"), "https://example.test/qr")
        finally:
            cleanup()

    def test_im_weixin_login_status_rejects_confirmed_without_credentials(self) -> None:
        from cccc.ports.web.app import create_app

        home, cleanup = self._with_home()
        try:
            gid = self._create_group("im-weixin-login-missing-creds")

            with TestClient(create_app()) as client:
                set_resp = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "weixin",
                    },
                )
                self.assertEqual(set_resp.status_code, 200)
                self.assertTrue(bool(set_resp.json().get("ok")))

                state_dir = Path(home) / "groups" / gid / "state"
                state_dir.mkdir(parents=True, exist_ok=True)
                status_path = state_dir / "im_weixin_login.json"
                status_path.write_text(
                    json.dumps(
                        {
                            "status": "waiting_scan",
                            "logged_in": False,
                            "qrcode": "qr-token-1",
                            "qrcode_url": "https://example.test/qr",
                            "poll_base_url": "https://example.test",
                        }
                    ),
                    encoding="utf-8",
                )

                class _FakeApi:
                    async def poll_qr_status(self, base_url: str, qrcode: str):
                        _ = base_url
                        _ = qrcode
                        return {"status": "confirmed"}

                async def _unexpected_save_credentials(*args, **kwargs):
                    _ = args
                    _ = kwargs
                    raise AssertionError("credentials should not be saved when confirmed payload is incomplete")

                class _Credentials:
                    def __init__(self, **kwargs) -> None:
                        self.kwargs = kwargs

                fake_auth = types.ModuleType("wechatbot.auth")
                fake_auth.FIXED_QR_BASE_URL = "https://ilinkai.weixin.qq.com"
                fake_auth.save_credentials = _unexpected_save_credentials
                fake_protocol = types.ModuleType("wechatbot.protocol")
                fake_protocol.ILinkApi = _FakeApi
                fake_types = types.ModuleType("wechatbot.types")
                fake_types.Credentials = _Credentials
                fake_pkg = types.ModuleType("wechatbot")
                fake_pkg.__path__ = []  # mark as package

                with patch.dict(sys.modules, {
                    "wechatbot": fake_pkg,
                    "wechatbot.auth": fake_auth,
                    "wechatbot.protocol": fake_protocol,
                    "wechatbot.types": fake_types,
                }):
                    status_resp = client.get(f"/api/im/weixin/login/status?group_id={gid}")

                self.assertEqual(status_resp.status_code, 200)
                payload = status_resp.json()
                self.assertTrue(bool(payload.get("ok")))
                result = payload.get("result") or {}
                self.assertEqual(result.get("status"), "error")
                self.assertFalse(bool(result.get("logged_in")))
                self.assertIn("missing credentials", str(result.get("error") or ""))
                self.assertFalse((state_dir / "im_weixin_credentials.json").exists())
        finally:
            cleanup()

    def test_im_status_reports_disabled_after_stop(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("im-status-disabled-after-stop")

            with TestClient(create_app()) as client:
                set_resp = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "weixin",
                    },
                )
                self.assertEqual(set_resp.status_code, 200)
                self.assertTrue(bool(set_resp.json().get("ok")))

                with patch("subprocess.Popen", return_value=_AliveProc()):
                    start_resp = client.post("/api/im/start", json={"group_id": gid})
                self.assertEqual(start_resp.status_code, 200)
                self.assertTrue(bool(start_resp.json().get("ok")))

                stop_resp = client.post("/api/im/stop", json={"group_id": gid})
                self.assertEqual(stop_resp.status_code, 200)
                self.assertTrue(bool(stop_resp.json().get("ok")))

                status_resp = client.get(f"/api/im/status?group_id={gid}")
                self.assertEqual(status_resp.status_code, 200)
                payload = status_resp.json()
                self.assertTrue(bool(payload.get("ok")))
                result = payload.get("result") or {}
                self.assertTrue(bool(result.get("configured")))
                self.assertFalse(bool(result.get("enabled")))
                self.assertFalse(bool(result.get("running")))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
