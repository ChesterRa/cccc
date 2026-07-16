import json
import unittest
import urllib.error
from unittest.mock import patch

from cccc.ports.im.adapters.feishu.client import FeishuClient


class _FakeHttpResponse:
    def __init__(self, body: dict):
        self._body = json.dumps(body, ensure_ascii=False).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self) -> bytes:
        return self._body


class _FakeHttpError(urllib.error.HTTPError):
    def __init__(self, url: str, code: int, body: str):
        super().__init__(url, code, "failed", hdrs=None, fp=None)
        self._body = body.encode("utf-8")

    def read(self) -> bytes:
        return self._body


class _FakeHttpxResponse:
    def __init__(self, body: dict, status_code: int = 200):
        self._body = body
        self.status_code = status_code
        self.text = json.dumps(body, ensure_ascii=False)

    def json(self) -> dict:
        return self._body


class TestFeishuClient(unittest.TestCase):
    def test_get_token_refreshes_and_caches_tenant_token(self) -> None:
        calls = []

        def fake_urlopen(req, timeout=0):
            calls.append((req, timeout))
            return _FakeHttpResponse({"code": 0, "tenant_access_token": "tenant-token", "expire": 7200})

        client = FeishuClient(
            app_id="cli_test",
            app_secret="secret",
            api_base="https://open.feishu.cn/open-apis",
            log_fn=lambda _msg: None,
        )

        with patch("urllib.request.urlopen", side_effect=fake_urlopen):
            self.assertEqual(client.get_token(), "tenant-token")
            self.assertEqual(client.get_token(), "tenant-token")

        self.assertEqual(len(calls), 1)
        req = calls[0][0]
        self.assertEqual(req.full_url, "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
        self.assertEqual(calls[0][1], 10)
        self.assertEqual(json.loads(req.data.decode("utf-8")), {"app_id": "cli_test", "app_secret": "secret"})

    def test_api_get_encodes_query_params_and_auth_header(self) -> None:
        seen = []

        def fake_urlopen(req, timeout=0):
            seen.append((req, timeout))
            return _FakeHttpResponse({"code": 0, "data": {"ok": True}})

        client = FeishuClient(
            app_id="cli_test",
            app_secret="secret",
            api_base="https://open.feishu.cn/open-apis",
            log_fn=lambda _msg: None,
        )
        client.token = "tenant-token"
        client.token_expires = 9_999_999_999

        with patch("urllib.request.urlopen", side_effect=fake_urlopen):
            result = client.api("GET", "/im/v1/chats/oc_1", {"user_id_type": "open_id"}, timeout=7)

        self.assertEqual(result, {"code": 0, "data": {"ok": True}})
        req = seen[0][0]
        self.assertEqual(req.full_url, "https://open.feishu.cn/open-apis/im/v1/chats/oc_1?user_id_type=open_id")
        self.assertEqual(req.get_header("Authorization"), "Bearer tenant-token")
        self.assertEqual(seen[0][1], 7)

    def test_api_returns_structured_error_for_http_error(self) -> None:
        logs = []
        client = FeishuClient(
            app_id="cli_test",
            app_secret="secret",
            api_base="https://open.feishu.cn/open-apis",
            log_fn=logs.append,
        )
        client.token = "tenant-token"
        client.token_expires = 9_999_999_999

        with patch("urllib.request.urlopen", side_effect=_FakeHttpError("https://example.invalid", 403, "denied")):
            result = client.api("POST", "/im/v1/messages", {"content": "{}"})

        self.assertEqual(result["code"], 403)
        self.assertEqual(result["error"], "denied")
        self.assertTrue(any("HTTP 403" in line for line in logs))

    def test_upload_media_uploads_regular_file(self) -> None:
        calls = []
        client = FeishuClient(
            app_id="cli_test",
            app_secret="secret",
            api_base="https://open.feishu.cn/open-apis",
            log_fn=lambda _msg: None,
        )
        client.token = "tenant-token"
        client.token_expires = 9_999_999_999

        def fake_post(url, *, headers=None, data=None, files=None, timeout=0):
            calls.append((url, headers, data, files, timeout))
            return _FakeHttpxResponse({"code": 0, "data": {"file_key": "file_key_1"}})

        with patch("httpx.post", side_effect=fake_post):
            media = client.upload_media(b"hello", "report.md", "text/markdown", is_image=False)

        self.assertEqual(media, ("file", "file_key", "file_key_1"))
        url, headers, data, files, timeout = calls[0]
        self.assertEqual(url, "https://open.feishu.cn/open-apis/im/v1/files")
        self.assertEqual(headers, {"Authorization": "Bearer tenant-token"})
        self.assertEqual(data, {"file_type": "stream", "file_name": "report.md"})
        self.assertEqual(files, {"file": ("report.md", b"hello", "text/markdown")})
        self.assertEqual(timeout, 60)

    def test_upload_media_uploads_image(self) -> None:
        client = FeishuClient(
            app_id="cli_test",
            app_secret="secret",
            api_base="https://open.feishu.cn/open-apis",
            log_fn=lambda _msg: None,
        )
        client.token = "tenant-token"
        client.token_expires = 9_999_999_999

        def fake_post(url, *, headers=None, data=None, files=None, timeout=0):
            self.assertEqual(url, "https://open.feishu.cn/open-apis/im/v1/images")
            self.assertEqual(data, {"image_type": "message"})
            self.assertEqual(files, {"image": ("figure.png", b"png", "image/png")})
            return _FakeHttpxResponse({"code": 0, "data": {"image_key": "image_key_1"}})

        with patch("httpx.post", side_effect=fake_post):
            media = client.upload_media(b"png", "figure.png", "image/png", is_image=True)

        self.assertEqual(media, ("image", "image_key", "image_key_1"))

    def test_download_attachment_downloads_image_bytes(self) -> None:
        seen = []
        client = FeishuClient(
            app_id="cli_test",
            app_secret="secret",
            api_base="https://open.feishu.cn/open-apis",
            log_fn=lambda _msg: None,
        )
        client.token = "tenant-token"
        client.token_expires = 9_999_999_999

        class _BinaryResponse:
            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

            def read(self) -> bytes:
                return b"image-bytes"

        def fake_urlopen(req, timeout=0):
            seen.append((req, timeout))
            return _BinaryResponse()

        with patch("urllib.request.urlopen", side_effect=fake_urlopen):
            raw = client.download_attachment("image", "img_key")

        self.assertEqual(raw, b"image-bytes")
        req, timeout = seen[0]
        self.assertEqual(req.full_url, "https://open.feishu.cn/open-apis/im/v1/images/img_key")
        self.assertEqual(req.get_header("Authorization"), "Bearer tenant-token")
        self.assertEqual(timeout, 30)


if __name__ == "__main__":
    unittest.main()
