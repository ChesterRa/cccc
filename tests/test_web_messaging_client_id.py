import os
import tempfile
import unittest
from io import BytesIO
from types import SimpleNamespace
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebMessagingClientId(unittest.TestCase):
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

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

    def test_send_and_reply_preserve_client_id(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="client-id", topic="")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()

                send_resp = client.post(
                    f"/api/v1/groups/{group.group_id}/send",
                    json={
                        "text": "hello",
                        "by": "user",
                        "to": ["user"],
                        "client_id": "local-send-1",
                        "refs": [
                            {
                                "kind": "presentation_ref",
                                "slot_id": "slot-2",
                                "label": "P2",
                                "locator_label": "PDF p.12",
                            }
                        ],
                    },
                )
                self.assertEqual(send_resp.status_code, 200)
                send_body = send_resp.json()
                self.assertTrue(bool(send_body.get("ok")))
                send_event = ((send_body.get("result") or {}).get("event")) or {}
                self.assertEqual((((send_event.get("data") or {}).get("client_id")) or ""), "local-send-1")
                self.assertEqual((((send_event.get("data") or {}).get("refs")) or [])[0].get("slot_id"), "slot-2")

                reply_to = str(send_event.get("id") or "")
                reply_resp = client.post(
                    f"/api/v1/groups/{group.group_id}/reply",
                    json={
                        "text": "reply",
                        "by": "user",
                        "to": ["user"],
                        "reply_to": reply_to,
                        "client_id": "local-reply-1",
                        "refs": [
                            {
                                "kind": "presentation_ref",
                                "slot_id": "slot-2",
                                "label": "P2",
                                "locator_label": "PDF p.12",
                            }
                        ],
                    },
                )
                self.assertEqual(reply_resp.status_code, 200)
                reply_body = reply_resp.json()
                self.assertTrue(bool(reply_body.get("ok")))
                reply_event = ((reply_body.get("result") or {}).get("event")) or {}
                self.assertEqual((((reply_event.get("data") or {}).get("client_id")) or ""), "local-reply-1")
                self.assertEqual((((reply_event.get("data") or {}).get("refs")) or [])[0].get("slot_id"), "slot-2")
        finally:
            cleanup()

    def test_send_preserves_federation_provenance_fields(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="federation-provenance", topic="")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()
                resp = client.post(
                    f"/api/v1/groups/{group.group_id}/send",
                    json={
                        "text": "hello from peer",
                        "by": "federation:peer_a",
                        "to": ["user"],
                        "source_platform": "federation_session",
                        "source_user_id": "peer_a",
                        "source_user_name": "Remote Group",
                        "src_group_id": "g_remote",
                        "src_event_id": "remote-event-1",
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(bool(body.get("ok")))
            data = (((body.get("result") or {}).get("event") or {}).get("data")) or {}
            self.assertEqual(data.get("source_platform"), "federation_session")
            self.assertEqual(data.get("source_user_id"), "peer_a")
            self.assertEqual(data.get("source_user_name"), "Remote Group")
            self.assertEqual(data.get("src_group_id"), "g_remote")
            self.assertEqual(data.get("src_event_id"), "remote-event-1")
        finally:
            cleanup()

    def test_send_replays_existing_event_for_duplicate_client_id(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="client-id-replay", topic="")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()
                payload = {
                    "text": "hello once",
                    "by": "user",
                    "to": ["user"],
                    "client_id": "local-send-replay-1",
                }

                first_resp = client.post(f"/api/v1/groups/{group.group_id}/send", json=payload)
                second_resp = client.post(f"/api/v1/groups/{group.group_id}/send", json=payload)

                self.assertEqual(first_resp.status_code, 200)
                self.assertEqual(second_resp.status_code, 200)
                first_result = first_resp.json().get("result") or {}
                second_result = second_resp.json().get("result") or {}
                first_event = first_result.get("event") or {}
                self.assertEqual(first_event.get("id"), second_result.get("event_id"))
                self.assertTrue(bool(second_result.get("replayed")))
        finally:
            cleanup()

    def test_send_duplicate_client_id_is_scoped_to_sender(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="client-id-sender-scope", topic="")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()
                first_resp = client.post(
                    f"/api/v1/groups/{group.group_id}/send",
                    json={
                        "text": "from user",
                        "by": "user",
                        "to": ["user"],
                        "client_id": "same-client-id",
                    },
                )
                second_resp = client.post(
                    f"/api/v1/groups/{group.group_id}/send",
                    json={
                        "text": "from peer",
                        "by": "peer-1",
                        "to": ["user"],
                        "client_id": "same-client-id",
                    },
                )

                self.assertEqual(first_resp.status_code, 200)
                self.assertEqual(second_resp.status_code, 200)
                first_result = first_resp.json().get("result") or {}
                second_result = second_resp.json().get("result") or {}
                first_event = first_result.get("event") or {}
                second_event = second_result.get("event") or {}
                self.assertNotEqual(first_event.get("id"), second_event.get("id"))
                self.assertFalse(bool(second_result.get("replayed")))
                self.assertEqual(str(second_event.get("by") or ""), "peer-1")
        finally:
            cleanup()

    def test_upload_routes_preserve_client_id(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="client-id-upload", topic="")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()

                send_upload_resp = client.post(
                    f"/api/v1/groups/{group.group_id}/send_upload",
                    data={
                        "by": "user",
                        "text": "upload hello",
                        "to_json": "[\"user\"]",
                        "client_id": "upload-send-1",
                        "refs_json": "[{\"kind\":\"presentation_ref\",\"slot_id\":\"slot-3\",\"label\":\"P3\",\"locator_label\":\"Web\"}]",
                    },
                    files={"files": ("note.txt", BytesIO(b"hello"), "text/plain")},
                )
                self.assertEqual(send_upload_resp.status_code, 200)
                send_upload_body = send_upload_resp.json()
                self.assertTrue(bool(send_upload_body.get("ok")))
                send_upload_event = ((send_upload_body.get("result") or {}).get("event")) or {}
                self.assertEqual((((send_upload_event.get("data") or {}).get("client_id")) or ""), "upload-send-1")
                self.assertEqual((((send_upload_event.get("data") or {}).get("refs")) or [])[0].get("slot_id"), "slot-3")

                reply_to = str(send_upload_event.get("id") or "")
                reply_upload_resp = client.post(
                    f"/api/v1/groups/{group.group_id}/reply_upload",
                    data={
                        "by": "user",
                        "text": "upload reply",
                        "to_json": "[\"user\"]",
                        "reply_to": reply_to,
                        "client_id": "upload-reply-1",
                        "refs_json": "[{\"kind\":\"presentation_ref\",\"slot_id\":\"slot-3\",\"label\":\"P3\",\"locator_label\":\"Web\"}]",
                    },
                    files={"files": ("reply.txt", BytesIO(b"reply"), "text/plain")},
                )
                self.assertEqual(reply_upload_resp.status_code, 200)
                reply_upload_body = reply_upload_resp.json()
                self.assertTrue(bool(reply_upload_body.get("ok")))
                reply_upload_event = ((reply_upload_body.get("result") or {}).get("event")) or {}
                self.assertEqual((((reply_upload_event.get("data") or {}).get("client_id")) or ""), "upload-reply-1")
                self.assertEqual((((reply_upload_event.get("data") or {}).get("refs")) or [])[0].get("slot_id"), "slot-3")
        finally:
            cleanup()

    def test_cross_group_upload_routes_remote_attachments_to_daemon(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        captured: list[dict] = []

        def fake_call_daemon(req: dict):
            captured.append(req)
            return {"ok": True, "result": {"remote_send": {"status": "sent"}}}

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="remote-upload", topic="")
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon),
                patch(
                    "cccc.ports.web.routes.messaging.resolve_remote_group_route",
                    return_value=SimpleNamespace(remote_group_id="g_remote", registration_id="reg_remote"),
                ),
            ):
                client = self._client()
                resp = client.post(
                    f"/api/v1/groups/{group.group_id}/send_cross_group_upload",
                    data={
                        "by": "user",
                        "text": "remote upload",
                        "dst_group_id": "g_remote",
                        "to_json": "[\"@foreman\"]",
                    },
                    files={"files": ("shot.png", BytesIO(b"png-bytes"), "image/png")},
                )

            self.assertEqual(resp.status_code, 200)
            self.assertTrue(resp.json().get("ok"))
            sends = [item for item in captured if item.get("op") == "send_cross_group"]
            self.assertEqual(len(sends), 1)
            req = sends[0]
            self.assertEqual(req.get("op"), "send_cross_group")
            args = req.get("args") or {}
            self.assertEqual(args.get("dst_group_id"), "g_remote")
            self.assertEqual(args.get("to"), ["@foreman"])
            attachments = args.get("attachments") or []
            self.assertEqual(len(attachments), 1)
            self.assertEqual(attachments[0].get("title"), "shot.png")
            self.assertEqual(attachments[0].get("mime_type"), "image/png")
            self.assertTrue(str(attachments[0].get("path") or "").startswith("state/blobs/"))
        finally:
            cleanup()

    def test_reply_upload_returns_400_when_default_reply_recipient_is_invalid(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="reply-upload-invalid-default", topic="")
            original = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key=str(group.doc.get("active_scope_key") or ""),
                by="user",
                data={"text": "original", "to": ["unknown-recipient"]},
            )

            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()
                resp = client.post(
                    f"/api/v1/groups/{group.group_id}/reply_upload",
                    data={
                        "by": "user",
                        "text": "upload reply",
                        "reply_to": str(original.get("id") or ""),
                    },
                    files={"files": ("reply.txt", BytesIO(b"reply"), "text/plain")},
                )

                self.assertEqual(resp.status_code, 400)
                error = resp.json().get("error") or {}
                self.assertEqual(error.get("code"), "invalid_recipient")
                self.assertIn("unknown recipient", str(error.get("message") or ""))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
