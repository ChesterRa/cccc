"""DingTalk OpenAPI client used by the IM adapter."""

from __future__ import annotations

import json
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Callable, Dict, Optional

DINGTALK_API_OLD = "https://oapi.dingtalk.com"
DINGTALK_API_NEW = "https://api.dingtalk.com"


class DingTalkApiClient:
    """Small synchronous client for DingTalk token and REST API calls."""

    def __init__(
        self,
        *,
        app_key: str,
        app_secret: str,
        log: Callable[[str], None],
    ) -> None:
        self.app_key = app_key
        self.app_secret = app_secret
        self._log = log
        self._token = ""
        self._token_expires = 0.0
        self._token_lock = threading.Lock()

    def get_token(self) -> str:
        """Return a valid access token, refreshing when close to expiry."""
        with self._token_lock:
            now = time.time()
            if self._token and now < self._token_expires - 300:
                return self._token
            if self.refresh_token():
                return self._token
            return ""

    def refresh_token(self) -> bool:
        """Refresh DingTalk access_token using app credentials."""
        params = urllib.parse.urlencode({
            "appkey": self.app_key,
            "appsecret": self.app_secret,
        })
        req = urllib.request.Request(f"{DINGTALK_API_OLD}/gettoken?{params}", method="GET")
        req.add_header("Accept", "application/json")

        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                body = resp.read().decode("utf-8", errors="replace")
                result = json.loads(body)

            if result.get("errcode") == 0:
                self._token = result.get("access_token", "")
                expire = int(result.get("expires_in", 7200))
                self._token_expires = time.time() + expire
                self._log(f"[token] Refreshed, expires in {expire}s")
                return True
            self._log(f"[token] Failed: {result.get('errmsg', 'unknown')}")
            return False
        except Exception as e:
            self._log(f"[token] Error: {e}")
            return False

    def api_old(
        self,
        method: str,
        endpoint: str,
        body: Optional[Dict[str, Any]] = None,
        timeout: int = 15,
    ) -> Dict[str, Any]:
        """Call DingTalk old API at oapi.dingtalk.com."""
        token = self.get_token()
        if not token and "gettoken" not in endpoint:
            return {"errcode": -1, "errmsg": "No valid token"}

        url = f"{DINGTALK_API_OLD}{endpoint}"
        if method == "GET":
            if body:
                url = f"{url}?{urllib.parse.urlencode(body)}"
            if token and "access_token" not in url:
                sep = "&" if "?" in url else "?"
                url = f"{url}{sep}access_token={token}"
            data = None
        else:
            if token and "access_token" not in url:
                sep = "&" if "?" in url else "?"
                url = f"{url}{sep}access_token={token}"
            data = json.dumps(body or {}, ensure_ascii=False).encode("utf-8") if body else None

        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("Content-Type", "application/json; charset=utf-8")
        req.add_header("Accept", "application/json")

        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                result_body = resp.read().decode("utf-8", errors="replace")
                return json.loads(result_body)
        except urllib.error.HTTPError as e:
            err_text = _read_http_error(e)
            self._log(f"[api_old] {method} {endpoint}: HTTP {e.code} - {err_text}")
            return {"errcode": e.code, "errmsg": str(e), "error": err_text}
        except Exception as e:
            self._log(f"[api_old] {method} {endpoint}: {e}")
            return {"errcode": -1, "errmsg": str(e)}

    def api_new(
        self,
        method: str,
        endpoint: str,
        body: Optional[Dict[str, Any]] = None,
        timeout: int = 15,
    ) -> Dict[str, Any]:
        """Call DingTalk new API at api.dingtalk.com."""
        token = self.get_token()
        if not token:
            return {"code": -1, "message": "No valid token"}

        url = f"{DINGTALK_API_NEW}{endpoint}"
        if method == "GET" and body:
            url = f"{url}?{urllib.parse.urlencode(body)}"
            data = None
        else:
            data = json.dumps(body or {}, ensure_ascii=False).encode("utf-8") if body else None

        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("x-acs-dingtalk-access-token", token)
        req.add_header("Content-Type", "application/json; charset=utf-8")
        req.add_header("Accept", "application/json")

        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                result_body = resp.read().decode("utf-8", errors="replace")
                return json.loads(result_body)
        except urllib.error.HTTPError as e:
            err_text = _read_http_error(e)
            self._log(f"[api_new] {method} {endpoint}: HTTP {e.code} - {err_text}")
            return {"code": e.code, "message": str(e), "error": err_text}
        except Exception as e:
            self._log(f"[api_new] {method} {endpoint}: {e}")
            return {"code": -1, "message": str(e)}


def _read_http_error(error: urllib.error.HTTPError) -> str:
    try:
        return error.read().decode("utf-8", "ignore")[:300]
    except Exception:
        return ""
