import os
import shutil
import tempfile
import time
import unittest
from pathlib import Path
from typing import Any, Dict, List, Optional

from cccc.contracts.v1 import DaemonRequest
from cccc.daemon.messaging.delivery import PendingMessage, render_single_message
from cccc.daemon.server import handle_request
from cccc.kernel.actors import add_actor
from cccc.kernel.group import Group, load_group
from cccc.ports.im.adapters.base import IMAdapter
from cccc.ports.im.bridge import IMBridge


class _FakeDingTalkAdapter(IMAdapter):
    platform = "dingtalk"

    def __init__(self, messages: List[Dict[str, Any]]):
        self._messages = list(messages)
        self._connected = False
        self.sent_messages: List[Dict[str, Any]] = []
        self.sent_files: List[Dict[str, Any]] = []

    def connect(self) -> bool:
        self._connected = True
        return True

    def disconnect(self) -> None:
        self._connected = False

    def poll(self) -> List[Dict[str, Any]]:
        if not self._connected:
            return []
        out = list(self._messages)
        self._messages = []
        return out

    def send_message(
        self,
        chat_id: str,
        text: str,
        thread_id: Optional[int] = None,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        self.sent_messages.append(
            {
                "chat_id": chat_id,
                "text": text,
                "thread_id": thread_id,
                "mention_user_ids": mention_user_ids,
            }
        )
        return True

    def get_chat_title(self, chat_id: str) -> str:
        return str(chat_id)

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        _ = attachment
        return b""

    def send_file(
        self,
        chat_id: str,
        *,
        file_path: Path,
        filename: str,
        caption: str = "",
        thread_id: Optional[int] = None,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        self.sent_files.append(
            {
                "chat_id": chat_id,
                "file_path": str(file_path),
                "filename": filename,
                "caption": caption,
                "thread_id": thread_id,
                "mention_user_ids": mention_user_ids,
            }
        )
        return True


class _FakeWecomFileImageAdapter(IMAdapter):
    platform = "wecom"

    def __init__(self) -> None:
        self._connected = False
        self._messages = [
            {
                "chat_id": "cid_wecom",
                "chat_title": "ops",
                "chat_type": "p2p",
                "routed": True,
                "thread_id": 0,
                "text": "[file: unknown]",
                "from_user": "Alice",
                "from_user_id": "staff_001",
                "attachments": [
                    {
                        "kind": "file",
                        "media_id": "media_file_img",
                        "file_name": "file",
                        "mime_type": "",
                    }
                ],
                "message_id": "msg_file_img",
                "timestamp": time.time(),
            }
        ]

    def connect(self) -> bool:
        self._connected = True
        return True

    def disconnect(self) -> None:
        self._connected = False

    def poll(self) -> List[Dict[str, Any]]:
        if not self._connected:
            return []
        out = list(self._messages)
        self._messages = []
        return out

    def send_message(
        self,
        chat_id: str,
        text: str,
        thread_id: Optional[int] = None,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        _ = chat_id
        _ = text
        _ = thread_id
        _ = mention_user_ids
        return True

    def get_chat_title(self, chat_id: str) -> str:
        return str(chat_id)

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        _ = attachment
        return b"\x89PNG\r\n\x1a\nfake-png"


class _FakeWecomUnknownTextFileAdapter(IMAdapter):
    platform = "wecom"

    def __init__(self) -> None:
        self._connected = False
        self._messages = [
            {
                "chat_id": "cid_wecom_text",
                "chat_title": "ops",
                "chat_type": "p2p",
                "routed": True,
                "thread_id": 0,
                "text": "[file]",
                "from_user": "Alice",
                "from_user_id": "staff_001",
                "attachments": [
                    {
                        "kind": "file",
                        "media_id": "media_unknown_text",
                        "file_name": "file",
                        "mime_type": "application/octet-stream",
                    }
                ],
                "message_id": "msg_unknown_text",
                "timestamp": time.time(),
            }
        ]

    def connect(self) -> bool:
        self._connected = True
        return True

    def disconnect(self) -> None:
        self._connected = False

    def poll(self) -> List[Dict[str, Any]]:
        if not self._connected:
            return []
        out = list(self._messages)
        self._messages = []
        return out

    def send_message(
        self,
        chat_id: str,
        text: str,
        thread_id: Optional[int] = None,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        _ = chat_id
        _ = text
        _ = thread_id
        _ = mention_user_ids
        return True

    def get_chat_title(self, chat_id: str) -> str:
        return str(chat_id)

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        _ = attachment
        return (
            b"[GLOBAL]\n"
            b"GatewayAddress = office.truesightai.com\n"
            b"GatewayPort = 9443\n"
        )


class _FakeWecomDispositionNamedTextFileAdapter(_FakeWecomUnknownTextFileAdapter):
    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        attachment["file_name"] = "tsvpn.ini"
        return super().download_attachment(attachment)


class _FakeWecomUnknownRawFileAdapter(IMAdapter):
    platform = "wecom"

    def __init__(self, raw: bytes, *, message_id: str = "msg_unknown_raw") -> None:
        self._connected = False
        self._raw = raw
        self._messages = [
            {
                "chat_id": "cid_wecom_raw",
                "chat_title": "ops",
                "chat_type": "p2p",
                "routed": True,
                "thread_id": 0,
                "text": "[file]",
                "from_user": "Alice",
                "from_user_id": "staff_001",
                "attachments": [
                    {
                        "kind": "file",
                        "media_id": "media_unknown_raw",
                        "file_name": "file",
                        "mime_type": "",
                    }
                ],
                "message_id": message_id,
                "timestamp": time.time(),
            }
        ]

    def connect(self) -> bool:
        self._connected = True
        return True

    def disconnect(self) -> None:
        self._connected = False

    def poll(self) -> List[Dict[str, Any]]:
        if not self._connected:
            return []
        out = list(self._messages)
        self._messages = []
        return out

    def send_message(
        self,
        chat_id: str,
        text: str,
        thread_id: Optional[int] = None,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        _ = chat_id
        _ = text
        _ = thread_id
        _ = mention_user_ids
        return True

    def get_chat_title(self, chat_id: str) -> str:
        return str(chat_id)

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        _ = attachment
        return self._raw


class TestImSenderIdentity(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            for attempt in range(5):
                try:
                    shutil.rmtree(td)
                    break
                except FileNotFoundError:
                    break
                except OSError:
                    if attempt >= 4:
                        raise
                    time.sleep(0.05)

        return td, cleanup

    def _create_group_with_peer(self) -> tuple[Group, str]:
        resp, _ = handle_request(
            DaemonRequest.model_validate(
                {"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}}
            )
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        group_id = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        add_actor(group, actor_id="peer1", runtime="codex", runner="pty", enabled=True)
        return group, group_id

    def test_bridge_inbound_send_persists_source_identity(self) -> None:
        _, cleanup = self._with_home()
        bridge: IMBridge | None = None
        try:
            group, _group_id = self._create_group_with_peer()
            adapter = _FakeDingTalkAdapter(
                [
                    {
                        "chat_id": "cid_g1",
                        "chat_title": "ops",
                        "chat_type": "group",
                        "routed": True,
                        "thread_id": 0,
                        "text": "你知道我是谁吗",
                        "from_user": "Alice",
                        "from_user_id": "staff_001",
                        "mention_user_ids": ["staff_001"],
                        "message_id": "msg_001",
                        "timestamp": time.time(),
                    }
                ]
            )
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]

            captured: List[Dict[str, Any]] = []

            def _daemon(req: Dict[str, Any]) -> Dict[str, Any]:
                resp, _ = handle_request(DaemonRequest.model_validate(req))
                payload: Dict[str, Any] = {"ok": bool(resp.ok)}
                if resp.ok:
                    payload["result"] = resp.result
                else:
                    payload["error"] = resp.error.model_dump() if resp.error else {}
                captured.append(payload)
                return payload

            bridge._daemon = _daemon  # type: ignore[method-assign]
            bridge._process_inbound()

            self.assertEqual(len(captured), 1)
            event = ((captured[0].get("result") or {}).get("event") or {})
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            self.assertEqual(str(data.get("source_platform") or ""), "dingtalk")
            self.assertEqual(str(data.get("source_user_name") or ""), "Alice")
            self.assertEqual(str(data.get("source_user_id") or ""), "staff_001")
            self.assertEqual(data.get("mention_user_ids"), ["staff_001"])
            self.assertEqual(str(event.get("by") or ""), "user")
        finally:
            if bridge is not None:
                bridge.stop()
            cleanup()

    def test_bridge_inbound_wecom_file_image_is_normalized_to_image_blob(self) -> None:
        _, cleanup = self._with_home()
        bridge: IMBridge | None = None
        try:
            group, _group_id = self._create_group_with_peer()
            adapter = _FakeWecomFileImageAdapter()
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]

            captured: List[Dict[str, Any]] = []

            def _daemon(req: Dict[str, Any]) -> Dict[str, Any]:
                resp, _ = handle_request(DaemonRequest.model_validate(req))
                payload: Dict[str, Any] = {"ok": bool(resp.ok)}
                if resp.ok:
                    payload["result"] = resp.result
                else:
                    payload["error"] = resp.error.model_dump() if resp.error else {}
                captured.append(payload)
                return payload

            bridge._daemon = _daemon  # type: ignore[method-assign]
            bridge._process_inbound()

            self.assertEqual(len(captured), 1)
            event = ((captured[0].get("result") or {}).get("event") or {})
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            attachments = data.get("attachments") if isinstance(data.get("attachments"), list) else []
            self.assertEqual(len(attachments), 1)
            self.assertEqual(str(data.get("text") or ""), "[file] attachment.png")
            self.assertEqual(str(attachments[0].get("kind") or ""), "image")
            self.assertEqual(str(attachments[0].get("mime_type") or ""), "image/png")
            self.assertTrue(str(attachments[0].get("title") or "").endswith(".png"))
        finally:
            if bridge is not None:
                bridge.stop()
            cleanup()

    def test_bridge_inbound_wecom_unknown_text_file_gets_inferred_name(self) -> None:
        _, cleanup = self._with_home()
        bridge: IMBridge | None = None
        try:
            group, _group_id = self._create_group_with_peer()
            adapter = _FakeWecomUnknownTextFileAdapter()
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]

            captured: List[Dict[str, Any]] = []

            def _daemon(req: Dict[str, Any]) -> Dict[str, Any]:
                resp, _ = handle_request(DaemonRequest.model_validate(req))
                payload: Dict[str, Any] = {"ok": bool(resp.ok)}
                if resp.ok:
                    payload["result"] = resp.result
                else:
                    payload["error"] = resp.error.model_dump() if resp.error else {}
                captured.append(payload)
                return payload

            bridge._daemon = _daemon  # type: ignore[method-assign]
            bridge._process_inbound()

            self.assertEqual(len(captured), 1)
            event = ((captured[0].get("result") or {}).get("event") or {})
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            attachments = data.get("attachments") if isinstance(data.get("attachments"), list) else []
            self.assertEqual(len(attachments), 1)
            self.assertEqual(str(data.get("text") or ""), "[file] attachment.ini")
            self.assertEqual(str(attachments[0].get("title") or ""), "attachment.ini")
            self.assertEqual(str(attachments[0].get("mime_type") or ""), "text/plain")
        finally:
            if bridge is not None:
                bridge.stop()
            cleanup()

    def test_bridge_inbound_wecom_download_filename_overrides_generic_name(self) -> None:
        _, cleanup = self._with_home()
        bridge: IMBridge | None = None
        try:
            group, _group_id = self._create_group_with_peer()
            adapter = _FakeWecomDispositionNamedTextFileAdapter()
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]

            captured: List[Dict[str, Any]] = []

            def _daemon(req: Dict[str, Any]) -> Dict[str, Any]:
                resp, _ = handle_request(DaemonRequest.model_validate(req))
                payload: Dict[str, Any] = {"ok": bool(resp.ok)}
                if resp.ok:
                    payload["result"] = resp.result
                else:
                    payload["error"] = resp.error.model_dump() if resp.error else {}
                captured.append(payload)
                return payload

            bridge._daemon = _daemon  # type: ignore[method-assign]
            bridge._process_inbound()

            self.assertEqual(len(captured), 1)
            event = ((captured[0].get("result") or {}).get("event") or {})
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            attachments = data.get("attachments") if isinstance(data.get("attachments"), list) else []
            self.assertEqual(len(attachments), 1)
            self.assertEqual(str(data.get("text") or ""), "[file] tsvpn.ini")
            self.assertEqual(str(attachments[0].get("title") or ""), "tsvpn.ini")
            self.assertEqual(str(attachments[0].get("mime_type") or ""), "text/plain")
        finally:
            if bridge is not None:
                bridge.stop()
            cleanup()

    def test_bridge_inbound_wecom_unknown_binary_files_get_inferred_names(self) -> None:
        cases = [
            (
                "pdf",
                b"%PDF-1.7\n%fake pdf bytes\n",
                "attachment.pdf",
                "application/pdf",
            ),
            (
                "zip",
                b"PK\x03\x04" + b"\0" * 32,
                "attachment.zip",
                "application/zip",
            ),
            (
                "docx",
                b"PK\x03\x04" + b"\0" * 32 + b"[Content_Types].xml word/document.xml",
                "attachment.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            (
                "xlsx",
                b"PK\x03\x04" + b"\0" * 32 + b"[Content_Types].xml xl/workbook.xml",
                "attachment.xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            (
                "pptx",
                b"PK\x03\x04" + b"\0" * 32 + b"[Content_Types].xml ppt/presentation.xml",
                "attachment.pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
        ]

        for label, raw, expected_title, expected_mime_type in cases:
            with self.subTest(label=label):
                _, cleanup = self._with_home()
                bridge: IMBridge | None = None
                try:
                    group, _group_id = self._create_group_with_peer()
                    adapter = _FakeWecomUnknownRawFileAdapter(raw, message_id=f"msg_unknown_{label}")
                    bridge = IMBridge(group=group, adapter=adapter)
                    self.assertTrue(bridge.start())
                    bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]

                    captured: List[Dict[str, Any]] = []

                    def _daemon(req: Dict[str, Any]) -> Dict[str, Any]:
                        resp, _ = handle_request(DaemonRequest.model_validate(req))
                        payload: Dict[str, Any] = {"ok": bool(resp.ok)}
                        if resp.ok:
                            payload["result"] = resp.result
                        else:
                            payload["error"] = resp.error.model_dump() if resp.error else {}
                        captured.append(payload)
                        return payload

                    bridge._daemon = _daemon  # type: ignore[method-assign]
                    bridge._process_inbound()

                    self.assertEqual(len(captured), 1)
                    event = ((captured[0].get("result") or {}).get("event") or {})
                    data = event.get("data") if isinstance(event.get("data"), dict) else {}
                    attachments = data.get("attachments") if isinstance(data.get("attachments"), list) else []
                    self.assertEqual(len(attachments), 1)
                    self.assertEqual(str(data.get("text") or ""), f"[file] {expected_title}")
                    self.assertEqual(str(attachments[0].get("title") or ""), expected_title)
                    self.assertEqual(str(attachments[0].get("mime_type") or ""), expected_mime_type)
                finally:
                    if bridge is not None:
                        bridge.stop()
                    cleanup()

    def test_render_single_message_includes_source_identity(self) -> None:
        rendered = render_single_message(
            PendingMessage(
                event_id="evt1",
                by="user",
                to=["@foreman"],
                text="请看一下",
                source_platform="dingtalk",
                source_user_name="Alice",
                source_user_id="staff_001",
            )
        )
        self.assertIn("[cccc] user[dingtalk / Alice / staff_001] → @foreman", rendered)

    def test_render_single_message_without_source_identity_keeps_legacy_header(self) -> None:
        rendered = render_single_message(
            PendingMessage(
                event_id="evt2",
                by="user",
                to=["@foreman"],
                text="普通消息",
            )
        )
        self.assertEqual(rendered, "[cccc] user → @foreman: 普通消息")

    def test_plain_send_without_im_source_keeps_legacy_event_shape(self) -> None:
        _, cleanup = self._with_home()
        try:
            _group, group_id = self._create_group_with_peer()

            resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "send",
                        "args": {
                            "group_id": group_id,
                            "text": "普通消息",
                            "by": "user",
                            "to": ["peer1"],
                        },
                    }
                )
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event") or {}
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            self.assertIsNone(data.get("source_platform"))
            self.assertIsNone(data.get("source_user_name"))
            self.assertIsNone(data.get("source_user_id"))
            self.assertEqual(str(event.get("by") or ""), "user")
        finally:
            cleanup()

    def test_bridge_forward_passes_explicit_mention_targets(self) -> None:
        _, cleanup = self._with_home()
        bridge: IMBridge | None = None
        try:
            group, _group_id = self._create_group_with_peer()
            adapter = _FakeDingTalkAdapter([])
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]
            bridge.subscribers.subscribe("cid_g1", "ops", platform="dingtalk")
            bridge._remember_mention_targets("cid_g1", 0, {"mention_user_ids": ["staff_001"]})

            bridge._forward_event(
                {
                    "kind": "chat.message",
                    "by": "claude-1",
                    "data": {
                        "text": "reply",
                        "to": ["user"],
                        "attachments": [],
                    },
                }
            )

            self.assertEqual(len(adapter.sent_messages), 1)
            self.assertEqual(adapter.sent_messages[0]["mention_user_ids"], ["staff_001"])
        finally:
            if bridge is not None:
                bridge.stop()
            cleanup()

    def test_bridge_forward_does_not_promote_source_user_id_to_mention_target(self) -> None:
        _, cleanup = self._with_home()
        bridge: IMBridge | None = None
        try:
            group, _group_id = self._create_group_with_peer()
            adapter = _FakeDingTalkAdapter([])
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]
            bridge.subscribers.subscribe("cid_g1", "ops", platform="dingtalk")

            bridge._forward_event(
                {
                    "kind": "chat.message",
                    "by": "claude-1",
                    "data": {
                        "text": "reply",
                        "to": ["user"],
                        "attachments": [],
                        "source_platform": "dingtalk",
                        "source_user_id": "union_or_sender_id",
                    },
                }
            )

            self.assertEqual(len(adapter.sent_messages), 1)
            self.assertIsNone(adapter.sent_messages[0]["mention_user_ids"])
        finally:
            if bridge is not None:
                bridge.stop()
            cleanup()

    def test_bridge_forward_passes_explicit_mention_targets_to_file_caption(self) -> None:
        _, cleanup = self._with_home()
        bridge: IMBridge | None = None
        try:
            group, _group_id = self._create_group_with_peer()
            adapter = _FakeDingTalkAdapter([])
            bridge = IMBridge(group=group, adapter=adapter)
            self.assertTrue(bridge.start())
            bridge.key_manager.is_authorized = lambda *_args, **_kwargs: True  # type: ignore[method-assign]
            bridge.subscribers.subscribe("cid_g1", "ops", platform="dingtalk")

            sample_file = Path(group.path) / "sample.txt"
            sample_file.write_text("ok", encoding="utf-8")

            from unittest.mock import patch

            with patch("cccc.ports.im.bridge.resolve_blob_attachment_path", return_value=sample_file):
                bridge._forward_event(
                    {
                        "kind": "chat.message",
                        "by": "claude-1",
                        "data": {
                            "text": "reply with file",
                            "to": ["user"],
                            "mention_user_ids": ["staff_001"],
                            "attachments": [{"path": "state/blobs/demo.txt", "title": "demo.txt"}],
                        },
                    }
                )

            self.assertEqual(len(adapter.sent_files), 1)
            self.assertEqual(adapter.sent_files[0]["mention_user_ids"], ["staff_001"])
        finally:
            if bridge is not None:
                bridge.stop()
            cleanup()


if __name__ == "__main__":
    unittest.main()
