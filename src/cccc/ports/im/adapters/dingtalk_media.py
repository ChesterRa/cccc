"""DingTalk media upload, download, and file sending helpers."""

from __future__ import annotations

import json
import uuid
import urllib.request
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional

from .dingtalk_api import DINGTALK_API_OLD
from .dingtalk_messages import is_image_file


class DingTalkMediaService:
    """Handle DingTalk media transfer and file-message fallback paths."""

    def __init__(
        self,
        *,
        get_token: Callable[[], str],
        api_new: Callable[[str, str, Optional[Dict[str, Any]], int], Dict[str, Any]],
        robot_code_getter: Callable[[], str],
        is_group: Callable[[str], bool],
        user_id_for_chat: Callable[[str], str],
        webhook_entry: Callable[[str], Optional[tuple[str, float]]],
        send_message: Callable[[str, str, Optional[List[str]]], bool],
        rate_limit: Callable[[str], None],
        log: Callable[[str], None],
    ) -> None:
        self._get_token = get_token
        self._api_new = api_new
        self._robot_code_getter = robot_code_getter
        self._is_group = is_group
        self._user_id_for_chat = user_id_for_chat
        self._webhook_entry = webhook_entry
        self._send_message = send_message
        self._rate_limit = rate_limit
        self._log = log

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        """Download an attachment from DingTalk."""
        download_code = attachment.get("download_code", "")
        if not download_code:
            raise ValueError("Missing download_code")

        robot_code = self._robot_code_getter()
        if not robot_code:
            raise ValueError("Missing robot_code (configure DINGTALK_ROBOT_CODE to download attachments)")

        token = self._get_token()
        if not token:
            raise ValueError("No valid token")

        resp = self._api_new("POST", "/v1.0/robot/messageFiles/download", {
            "downloadCode": download_code,
            "robotCode": robot_code,
        }, 15)
        download_url = resp.get("downloadUrl", "")
        if not download_url:
            raise ValueError(f"Failed to get download URL: {resp}")

        req = urllib.request.Request(download_url, method="GET")
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                return r.read()
        except Exception as e:
            raise ValueError(f"Download failed: {e}")

    def upload_media(self, raw: bytes, filename: str, media_type: str = "file") -> Optional[str]:
        """Upload file bytes to DingTalk media API and return media_id."""
        token = self._get_token()
        if not token:
            return None

        boundary = "----cccc" + uuid.uuid4().hex
        upload_url = f"{DINGTALK_API_OLD}/media/upload?access_token={token}&type={media_type}"
        safe_fn = _safe_filename(filename)
        content_type = _content_type_for(filename, media_type)

        body = b""
        body += (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="media"; filename="{safe_fn}"\r\n'
            f"Content-Type: {content_type}\r\n\r\n"
        ).encode("utf-8")
        body += raw
        body += f"\r\n--{boundary}--\r\n".encode("utf-8")

        req = urllib.request.Request(upload_url, data=body, method="POST")
        req.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")

        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                result = json.loads(resp.read().decode("utf-8", errors="replace"))

            if result.get("errcode") != 0:
                self._log(f"[upload_media] Upload failed: {result.get('errmsg', 'unknown')}")
                return None

            media_id = result.get("media_id", "")
            if not media_id:
                self._log("[upload_media] No media_id in response")
                return None
            return media_id
        except Exception as e:
            self._log(f"[upload_media] Upload error: {e}")
            return None

    def send_file_via_webhook(
        self,
        webhook_url: str,
        raw: bytes,
        filename: str,
        is_image: bool = False,
    ) -> bool:
        """Upload and send a file using DingTalk sessionWebhook."""
        media_type = "image" if is_image else "file"
        media_id = self.upload_media(raw, filename, media_type)
        if not media_id:
            return False

        body = _webhook_file_body(media_id=media_id, filename=filename, is_image=is_image)

        req = urllib.request.Request(
            webhook_url,
            data=json.dumps(body, ensure_ascii=False).encode("utf-8"),
            method="POST",
        )
        req.add_header("Content-Type", "application/json; charset=utf-8")

        try:
            with urllib.request.urlopen(req, timeout=15) as resp:
                result = json.loads(resp.read().decode("utf-8", errors="replace"))
            if result.get("errcode") == 0:
                self._log("[send_file_webhook] Sent successfully via webhook")
                return True
            self._log(f"[send_file_webhook] Failed: {result}")
            return False
        except Exception as e:
            self._log(f"[send_file_webhook] Error: {e}")
            return False

    def send_file_via_api(
        self,
        chat_id: str,
        raw: bytes,
        filename: str,
        is_image: bool = False,
    ) -> bool:
        """Upload and send a file through DingTalk robot OpenAPI."""
        robot_code = self._robot_code_getter()
        if not robot_code:
            self._log("[send_file_api] Missing robot_code; cannot use new API.")
            return False

        media_type = "image" if is_image else "file"
        media_id = self.upload_media(raw, filename, media_type)
        if not media_id:
            return False

        if is_image:
            msg_key = "sampleImageMsg"
            msg_param = json.dumps({"photoURL": media_id}, ensure_ascii=False)
        else:
            msg_key = "sampleFile"
            msg_param = json.dumps({"mediaId": media_id, "fileName": filename}, ensure_ascii=False)

        body: Dict[str, Any] = {
            "robotCode": robot_code,
            "msgKey": msg_key,
            "msgParam": msg_param,
        }
        if self._is_group(chat_id):
            body["openConversationId"] = chat_id
            endpoint = "/v1.0/robot/groupMessages/send"
        else:
            user_id = self._user_id_for_chat(chat_id) or chat_id
            body["userIds"] = [user_id]
            endpoint = "/v1.0/robot/oToMessages/batchSend"

        resp = self._api_new("POST", endpoint, body, 15)
        if resp.get("processQueryKey") or resp.get("sendResults"):
            self._log("[send_file_api] Sent successfully via API")
            return True

        self._log(f"[send_file_api] Failed: {resp}")
        return False

    def send_file(
        self,
        chat_id: str,
        *,
        file_path: Path,
        filename: str,
        caption: str = "",
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        """Send a file, preferring sessionWebhook and falling back to robot API."""
        self._rate_limit(chat_id)

        try:
            raw = file_path.read_bytes()
        except Exception as e:
            self._log(f"[send_file] Read failed: {e}")
            return False

        safe_fn = _safe_filename(filename or file_path.name or "file")
        is_image = is_image_file(safe_fn)

        webhook = self._webhook_entry(chat_id)
        if webhook:
            webhook_url, _expires_at = webhook
            if self.send_file_via_webhook(webhook_url, raw, safe_fn, is_image):
                self._send_caption(chat_id, caption, mention_user_ids)
                return True
            self._log("[send_file] Webhook failed, falling back to API...")
        else:
            self._log("[send_file] No cached sessionWebhook; using API.")

        if self.send_file_via_api(chat_id, raw, safe_fn, is_image):
            self._send_caption(chat_id, caption, mention_user_ids)
            return True

        self._log(f"[send_file] All methods failed for chat {chat_id}")
        return False

    def _send_caption(
        self,
        chat_id: str,
        caption: str,
        mention_user_ids: Optional[List[str]],
    ) -> None:
        if caption:
            self._send_message(chat_id, caption, mention_user_ids)


def _safe_filename(filename: str) -> str:
    return (filename or "file").replace("\\", "_").replace("/", "_")


def _content_type_for(filename: str, media_type: str) -> str:
    if media_type != "image":
        return "application/octet-stream"
    ext = Path(filename).suffix.lower()
    return {
        ".png": "image/png",
        ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg",
        ".gif": "image/gif",
        ".bmp": "image/bmp",
        ".webp": "image/webp",
    }.get(ext, "application/octet-stream")


def _webhook_file_body(*, media_id: str, filename: str, is_image: bool) -> Dict[str, Any]:
    if is_image:
        return {"msgtype": "image", "image": {"picURL": media_id}}
    return {
        "msgtype": "file",
        "file": {
            "mediaId": media_id,
            "fileType": _file_type_for(filename),
        },
    }


def _file_type_for(filename: str) -> str:
    suffix = Path(filename or "").suffix.lower().lstrip(".")
    return suffix or "file"
