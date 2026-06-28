"""Feishu Open API client helpers."""

from __future__ import annotations

import json
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Callable, Dict, Optional, Tuple

import httpx


class FeishuClient:
    """Small JSON API client with tenant token caching."""

    def __init__(
        self,
        *,
        app_id: str,
        app_secret: str,
        api_base: str,
        log_fn: Callable[[str], None],
    ):
        self.app_id = app_id
        self.app_secret = app_secret
        self.api_base = api_base
        self.log_fn = log_fn
        self.token: str = ""
        self.token_expires: float = 0
        self._token_lock = threading.Lock()

    def get_token(self) -> str:
        """Get valid tenant_access_token, refreshing if needed."""
        with self._token_lock:
            now = time.time()
            if self.token and now < self.token_expires - 300:
                return self.token
            if self.refresh_token():
                return self.token
            return ""

    def refresh_token(self) -> bool:
        """Refresh tenant_access_token."""
        url = f"{self.api_base}/auth/v3/tenant_access_token/internal"
        data = json.dumps(
            {
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            },
            ensure_ascii=False,
        ).encode("utf-8")

        req = urllib.request.Request(url, data=data, method="POST")
        req.add_header("Content-Type", "application/json; charset=utf-8")

        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                body = resp.read().decode("utf-8", errors="replace")
                result = json.loads(body)

            if result.get("code") == 0:
                self.token = result.get("tenant_access_token", "")
                expire = int(result.get("expire", 7200))
                self.token_expires = time.time() + expire
                self.log_fn(f"[token] Refreshed, expires in {expire}s")
                return True

            self.log_fn(f"[token] Failed: {result.get('msg', 'unknown')}")
            return False
        except Exception as e:
            self.log_fn(f"[token] Error: {e}")
            return False

    def api(
        self,
        method: str,
        endpoint: str,
        body: Optional[Dict[str, Any]] = None,
        timeout: int = 15,
    ) -> Dict[str, Any]:
        """Call a Feishu JSON API endpoint."""
        token = self.get_token()
        if not token:
            return {"code": -1, "msg": "No valid token"}

        url = f"{self.api_base}{endpoint}"
        if method == "GET" and body:
            query = urllib.parse.urlencode(body)
            url = f"{url}?{query}"
            data = None
        else:
            data = json.dumps(body or {}, ensure_ascii=False).encode("utf-8") if body else None

        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("Authorization", f"Bearer {token}")
        req.add_header("Content-Type", "application/json; charset=utf-8")
        req.add_header("Accept", "application/json")

        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                result_body = resp.read().decode("utf-8", errors="replace")
                return json.loads(result_body)
        except urllib.error.HTTPError as e:
            http_status = e.code
            err_text = ""
            try:
                err_text = e.read().decode("utf-8", "ignore")[:300]
            except Exception:
                pass
            self.log_fn(f"[api] {method} {endpoint}: HTTP {http_status} - {err_text}")
            return {"code": http_status, "msg": str(e), "error": err_text}
        except Exception as e:
            self.log_fn(f"[api] {method} {endpoint}: {e}")
            return {"code": -1, "msg": str(e)}

    def upload_media(
        self,
        raw: bytes,
        filename: str,
        mime_type: str,
        *,
        is_image: bool,
    ) -> Optional[Tuple[str, str, str]]:
        """Upload a Feishu message image or file and return message metadata."""
        token = self.get_token()
        if not token:
            return None

        try:
            if is_image:
                upload_resp = httpx.post(
                    f"{self.api_base}/im/v1/images",
                    headers={"Authorization": f"Bearer {token}"},
                    data={"image_type": "message"},
                    files={"image": (filename, raw, mime_type)},
                    timeout=60,
                )
            else:
                upload_resp = httpx.post(
                    f"{self.api_base}/im/v1/files",
                    headers={"Authorization": f"Bearer {token}"},
                    data={"file_type": "stream", "file_name": filename},
                    files={"file": (filename, raw, mime_type)},
                    timeout=60,
                )

            try:
                result = upload_resp.json()
            except Exception:
                result = {"code": upload_resp.status_code, "msg": upload_resp.text[:300]}

            if upload_resp.status_code >= 400 and result.get("code") == upload_resp.status_code:
                self.log_fn(f"[send_file] Upload HTTP {upload_resp.status_code}: {upload_resp.text[:300]}")
                return None
            if result.get("code") != 0:
                self.log_fn(f"[send_file] Upload failed: code={result.get('code')} msg={result.get('msg', 'unknown')}")
                return None

            msg_type = "image" if is_image else "file"
            key_name = "image_key" if is_image else "file_key"
            media_key = result.get("data", {}).get(key_name, "")
            if not media_key:
                self.log_fn(f"[send_file] No {key_name} in response")
                return None
            return msg_type, key_name, media_key
        except Exception as e:
            self.log_fn(f"[send_file] Upload error: {e}")
            return None

    def download_attachment(self, kind: str, key: str) -> bytes:
        """Download a Feishu image or file attachment."""
        token = self.get_token()
        if not token:
            raise ValueError("No valid token")

        if kind == "image":
            url = f"{self.api_base}/im/v1/images/{key}"
        elif kind == "file":
            url = f"{self.api_base}/im/v1/files/{key}"
        else:
            raise ValueError(f"Unknown attachment kind: {kind}")

        req = urllib.request.Request(url, method="GET")
        req.add_header("Authorization", f"Bearer {token}")

        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                return resp.read()
        except Exception as e:
            raise ValueError(f"Download failed: {e}")
