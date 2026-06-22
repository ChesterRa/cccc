import os
import shutil
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch


class TestMaintenanceOps(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            try:
                td_ctx.__exit__(None, None, None)
            except OSError:
                pass
            shutil.rmtree(td, ignore_errors=True)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_send_cross_group_honors_explicit_all(self) -> None:
        _, cleanup = self._with_home()
        try:
            src_create, _ = self._call("group_create", {"title": "src", "topic": "", "by": "user"})
            self.assertTrue(src_create.ok, getattr(src_create, "error", None))
            src_group_id = str((src_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(src_group_id)

            dst_create, _ = self._call("group_create", {"title": "dst", "topic": "", "by": "user"})
            self.assertTrue(dst_create.ok, getattr(dst_create, "error", None))
            dst_group_id = str((dst_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(dst_group_id)
            add_peer, _ = self._call(
                "actor_add",
                {"group_id": dst_group_id, "actor_id": "dst-peer", "runtime": "claude", "by": "user"},
            )
            self.assertTrue(add_peer.ok, getattr(add_peer, "error", None))

            relay, _ = self._call(
                "send_cross_group",
                {
                    "group_id": src_group_id,
                    "dst_group_id": dst_group_id,
                    "by": "user",
                    "text": "relay ping",
                    "to": ["@all"],
                },
            )
            self.assertTrue(relay.ok, getattr(relay, "error", None))
            src_event = (relay.result or {}).get("src_event") if isinstance(relay.result, dict) else {}
            dst_event = (relay.result or {}).get("dst_event") if isinstance(relay.result, dict) else {}
            self.assertIsInstance(src_event, dict)
            self.assertIsInstance(dst_event, dict)
            assert isinstance(src_event, dict)
            assert isinstance(dst_event, dict)
            self.assertEqual(str(src_event.get("kind") or ""), "chat.message")
            self.assertEqual(str(dst_event.get("kind") or ""), "chat.message")
            self.assertEqual(((src_event or {}).get("data") or {}).get("dst_to"), ["@all"])
            self.assertEqual(((dst_event or {}).get("data") or {}).get("to"), ["@all"])
        finally:
            cleanup()

    def test_send_cross_group_defaults_to_target_foreman_not_all(self) -> None:
        _, cleanup = self._with_home()
        try:
            src_create, _ = self._call("group_create", {"title": "src", "topic": "", "by": "user"})
            self.assertTrue(src_create.ok, getattr(src_create, "error", None))
            src_group_id = str((src_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(src_group_id)

            dst_create, _ = self._call("group_create", {"title": "dst", "topic": "", "by": "user"})
            self.assertTrue(dst_create.ok, getattr(dst_create, "error", None))
            dst_group_id = str((dst_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(dst_group_id)
            add_foreman, _ = self._call(
                "actor_add",
                {"group_id": dst_group_id, "actor_id": "dst-foreman", "runtime": "claude", "by": "user"},
            )
            self.assertTrue(add_foreman.ok, getattr(add_foreman, "error", None))

            relay, _ = self._call(
                "send_cross_group",
                {
                    "group_id": src_group_id,
                    "dst_group_id": dst_group_id,
                    "by": "user",
                    "text": "relay ping",
                },
            )
            self.assertTrue(relay.ok, getattr(relay, "error", None))
            src_event = (relay.result or {}).get("src_event") if isinstance(relay.result, dict) else {}
            dst_event = (relay.result or {}).get("dst_event") if isinstance(relay.result, dict) else {}
            self.assertEqual(((src_event or {}).get("data") or {}).get("dst_to"), ["@foreman"])
            self.assertEqual(((dst_event or {}).get("data") or {}).get("to"), ["@foreman"])
        finally:
            cleanup()

    def test_send_cross_group_rejects_refs(self) -> None:
        _, cleanup = self._with_home()
        try:
            src_create, _ = self._call("group_create", {"title": "src", "topic": "", "by": "user"})
            self.assertTrue(src_create.ok, getattr(src_create, "error", None))
            src_group_id = str((src_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(src_group_id)

            dst_create, _ = self._call("group_create", {"title": "dst", "topic": "", "by": "user"})
            self.assertTrue(dst_create.ok, getattr(dst_create, "error", None))
            dst_group_id = str((dst_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(dst_group_id)

            relay, _ = self._call(
                "send_cross_group",
                {
                    "group_id": src_group_id,
                    "dst_group_id": dst_group_id,
                    "by": "user",
                    "text": "relay ping",
                    "to": ["user"],
                    "refs": [{"kind": "presentation_ref", "slot_id": "slot-2"}],
                },
            )
            self.assertFalse(relay.ok)
            self.assertEqual(str(getattr(relay.error, "code", "") or ""), "refs_not_supported")
        finally:
            cleanup()

    def test_send_cross_group_rejects_hash_recipient_syntax(self) -> None:
        _, cleanup = self._with_home()
        try:
            src_create, _ = self._call("group_create", {"title": "src", "topic": "", "by": "user"})
            self.assertTrue(src_create.ok, getattr(src_create, "error", None))
            src_group_id = str((src_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(src_group_id)

            dst_create, _ = self._call("group_create", {"title": "dst", "topic": "", "by": "user"})
            self.assertTrue(dst_create.ok, getattr(dst_create, "error", None))
            dst_group_id = str((dst_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(dst_group_id)

            relay, _ = self._call(
                "send_cross_group",
                {
                    "group_id": src_group_id,
                    "dst_group_id": dst_group_id,
                    "by": "user",
                    "text": "relay ping",
                    "to": ["#self-agent"],
                },
            )
            self.assertFalse(relay.ok)
            self.assertEqual(str(getattr(relay.error, "code", "") or ""), "invalid_recipient_syntax")
            message = str(getattr(relay.error, "message", "") or "")
            self.assertIn("#group tokens are routing hints, not recipients", message)
            self.assertIn('cccc_group(action="resolve"', message)
            self.assertIn("unique real group_id", message)
            self.assertIn("cccc_message_send(dst_group_id=<g_...>", message)
            self.assertIn("text=<your own natural message to the target>", message)
            self.assertNotIn("unknown recipient", message)
        finally:
            cleanup()

    def test_send_cross_group_group_not_found_explains_group_resolution(self) -> None:
        _, cleanup = self._with_home()
        try:
            src_create, _ = self._call("group_create", {"title": "src", "topic": "", "by": "user"})
            self.assertTrue(src_create.ok, getattr(src_create, "error", None))
            src_group_id = str((src_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(src_group_id)

            relay, _ = self._call(
                "send_cross_group",
                {
                    "group_id": src_group_id,
                    "dst_group_id": "cccc",
                    "by": "user",
                    "text": "relay ping",
                    "to": ["@foreman"],
                },
            )
            self.assertFalse(relay.ok)
            self.assertEqual(str(getattr(relay.error, "code", "") or ""), "group_not_found")
            message = str(getattr(relay.error, "message", "") or "")
            self.assertIn("group not found: cccc", message)
            self.assertIn("dst_group_id must be the real group id", message)
            self.assertIn('cccc_group(action="resolve", token="cccc")', message)
            self.assertIn("Do not use #token/title as dst_group_id", message)
        finally:
            cleanup()

    def test_send_cross_group_uses_remote_bridge_route(self) -> None:
        from cccc.contracts.v1 import DaemonResponse
        from cccc.daemon.ops import maintenance_ops

        captured: dict[str, dict] = {}

        def fake_dispatch(op: str, args: dict):
            captured["dispatch"] = {"op": op, "args": args}
            return DaemonResponse(
                ok=True,
                result={
                    "event": {
                        "id": "evt_src",
                        "kind": "chat.message",
                        "group_id": args.get("group_id"),
                        "data": dict(args),
                    }
                },
            ), False

        def fake_remote_send(args: dict):
            captured["remote_send"] = args
            return DaemonResponse(ok=True, result={"receipt": {"ok": True, "status": "delivered"}})

        with patch.object(maintenance_ops, "load_group", return_value=SimpleNamespace(group_id="g_src")), \
             patch.object(
                 maintenance_ops,
                 "resolve_remote_group_route",
                 return_value=SimpleNamespace(
                     remote_group_id="g_remote",
                     registration_id="reg_remote",
                 ),
             ), \
             patch.object(maintenance_ops, "handle_remote_send", side_effect=fake_remote_send):
            resp = maintenance_ops.handle_send_cross_group(
                {
                    "group_id": "g_src",
                    "dst_group_id": "g_remote",
                    "by": "user",
                    "text": "remote ping",
                    "attachments": [
                        {
                            "kind": "image",
                            "path": "state/blobs/hash_shot.png",
                            "title": "shot.png",
                            "mime_type": "image/png",
                            "bytes": 10,
                            "sha256": "hash",
                        }
                    ],
                },
                dispatch_send=fake_dispatch,
            )

        self.assertTrue(resp.ok, getattr(resp, "error", None))
        self.assertEqual(captured["dispatch"]["op"], "send")
        self.assertEqual(captured["dispatch"]["args"]["to"], ["user"])
        self.assertEqual(captured["dispatch"]["args"]["dst_to"], ["@foreman"])
        self.assertEqual(captured["remote_send"]["registration_id"], "reg_remote")
        self.assertEqual(captured["remote_send"]["source_event_id"], "evt_src")
        self.assertEqual(captured["remote_send"]["payload"]["text"], "remote ping")
        self.assertEqual(captured["remote_send"]["payload"]["to"], ["@foreman"])
        self.assertEqual(captured["remote_send"]["payload"]["attachments"][0]["title"], "shot.png")
        self.assertEqual(captured["dispatch"]["args"]["attachments"][0]["title"], "shot.png")
        self.assertEqual((resp.result or {}).get("remote_group_id"), "g_remote")
        self.assertNotIn("dst_event", resp.result or {})

    def test_send_cross_group_propagates_terminal_remote_failure(self) -> None:
        from cccc.contracts.v1 import DaemonResponse
        from cccc.daemon.ops import maintenance_ops

        def fake_dispatch(op: str, args: dict):
            return DaemonResponse(
                ok=True,
                result={
                    "event": {
                        "id": "evt_src",
                        "kind": "chat.message",
                        "group_id": args.get("group_id"),
                        "data": dict(args),
                    }
                },
            ), False

        def fake_remote_send(args: dict):
            _ = args
            return DaemonResponse(
                ok=True,
                result={
                    "receipt": {
                        "ok": False,
                        "status": "failed",
                        "error": {"code": "invalid_attachments", "message": "attachment hash mismatch"},
                    }
                },
            )

        with patch.object(maintenance_ops, "load_group", return_value=SimpleNamespace(group_id="g_src")), \
             patch.object(
                 maintenance_ops,
                 "resolve_remote_group_route",
                 return_value=SimpleNamespace(
                     remote_group_id="g_remote",
                     registration_id="reg_remote",
                 ),
             ), \
             patch.object(maintenance_ops, "handle_remote_send", side_effect=fake_remote_send):
            resp = maintenance_ops.handle_send_cross_group(
                {
                    "group_id": "g_src",
                    "dst_group_id": "g_remote",
                    "by": "user",
                    "text": "remote ping",
                },
                dispatch_send=fake_dispatch,
            )

        self.assertFalse(resp.ok)
        self.assertEqual(str(getattr(resp.error, "code", "") or ""), "invalid_attachments")
        self.assertEqual(str(getattr(resp.error, "message", "") or ""), "attachment hash mismatch")
        self.assertEqual((getattr(resp.error, "details", {}) or {}).get("remote_group_id"), "g_remote")

    def test_send_cross_group_rejects_local_attachment_relay(self) -> None:
        _, cleanup = self._with_home()
        try:
            src_create, _ = self._call("group_create", {"title": "src", "topic": "", "by": "user"})
            self.assertTrue(src_create.ok, getattr(src_create, "error", None))
            src_group_id = str((src_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(src_group_id)

            dst_create, _ = self._call("group_create", {"title": "dst", "topic": "", "by": "user"})
            self.assertTrue(dst_create.ok, getattr(dst_create, "error", None))
            dst_group_id = str((dst_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(dst_group_id)

            relay, _ = self._call(
                "send_cross_group",
                {
                    "group_id": src_group_id,
                    "dst_group_id": dst_group_id,
                    "by": "user",
                    "text": "relay ping",
                    "to": ["@foreman"],
                    "attachments": [{"path": "state/blobs/hash_file.png", "title": "file.png"}],
                },
            )
            self.assertFalse(relay.ok)
            self.assertEqual(str(getattr(relay.error, "code", "") or ""), "attachments_not_supported")
        finally:
            cleanup()

    def test_send_cross_group_allows_target_foreman_recipient(self) -> None:
        _, cleanup = self._with_home()
        try:
            src_create, _ = self._call("group_create", {"title": "src", "topic": "", "by": "user"})
            self.assertTrue(src_create.ok, getattr(src_create, "error", None))
            src_group_id = str((src_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(src_group_id)

            dst_create, _ = self._call("group_create", {"title": "dst", "topic": "", "by": "user"})
            self.assertTrue(dst_create.ok, getattr(dst_create, "error", None))
            dst_group_id = str((dst_create.result or {}).get("group_id") or "").strip()
            self.assertTrue(dst_group_id)
            added, _ = self._call(
                "actor_add",
                {"group_id": dst_group_id, "actor_id": "dst-foreman", "runtime": "claude", "role": "foreman"},
            )
            self.assertTrue(added.ok, getattr(added, "error", None))

            relay, _ = self._call(
                "send_cross_group",
                {
                    "group_id": src_group_id,
                    "dst_group_id": dst_group_id,
                    "by": "user",
                    "text": "relay ping",
                    "to": ["@foreman"],
                },
            )
            self.assertTrue(relay.ok, getattr(relay, "error", None))
            dst_event = (relay.result or {}).get("dst_event") if isinstance(relay.result, dict) else {}
            self.assertEqual(((dst_event or {}).get("data") or {}).get("to"), ["@foreman"])
        finally:
            cleanup()

    def test_ledger_snapshot_and_compact(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.ledger_index import lookup_event_by_id

            create, _ = self._call("group_create", {"title": "ledger", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            sent, _ = self._call("send", {"group_id": group_id, "text": "hello", "by": "user", "to": ["user"]})
            self.assertTrue(sent.ok, getattr(sent, "error", None))
            event = (sent.result or {}).get("event") if isinstance(sent.result, dict) else {}
            event_id = str((event or {}).get("id") or "")
            self.assertTrue(event_id)

            snap, _ = self._call("ledger_snapshot", {"group_id": group_id, "by": "user", "reason": "test"})
            self.assertTrue(snap.ok, getattr(snap, "error", None))
            snapshot = (snap.result or {}).get("snapshot") if isinstance(snap.result, dict) else {}
            self.assertIsInstance(snapshot, dict)
            self.assertIn("manifest", snapshot)

            compact, _ = self._call("ledger_compact", {"group_id": group_id, "by": "user", "reason": "test", "force": True})
            self.assertTrue(compact.ok, getattr(compact, "error", None))
            self.assertIsInstance(compact.result, dict)
            result = compact.result if isinstance(compact.result, dict) else {}
            inner = result.get("result") if isinstance(result.get("result"), dict) else {}
            rotation = inner.get("rotation") if isinstance(inner.get("rotation"), dict) else {}
            self.assertTrue(bool(rotation.get("rotated")))
            compression = inner.get("compression") if isinstance(inner.get("compression"), dict) else {}
            self.assertGreaterEqual(int(compression.get("count") or 0), 1)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            manifest_path = group.path / "state" / "ledger" / "manifest.json"
            self.assertTrue(manifest_path.exists())
            manifest = manifest_path.read_text(encoding="utf-8")
            self.assertIn(".jsonl.gz", manifest)

            found = lookup_event_by_id(group.ledger_path, event_id)
            self.assertIsInstance(found, dict)
            self.assertEqual(str((found or {}).get("id") or ""), event_id)
            self.assertEqual(str((found or {}).get("kind") or ""), "chat.message")
        finally:
            cleanup()

    def test_ledger_retention_defaults_rotate_active_ledger_at_five_mb(self) -> None:
        from cccc.kernel.ledger_retention import LedgerRetentionConfig

        self.assertEqual(LedgerRetentionConfig.max_active_bytes, 5_000_000)

    def test_term_resize_rejects_tiny_size(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "resize", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            tiny, _ = self._call(
                "term_resize",
                {"group_id": group_id, "actor_id": "peer1", "cols": 9, "rows": 1},
            )
            self.assertFalse(tiny.ok)
            self.assertEqual(str(getattr(tiny.error, "code", "") or ""), "invalid_size")
        finally:
            cleanup()

    def test_term_resize_accepts_minimum_supported_size(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "resize-min", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            ok, _ = self._call(
                "term_resize",
                {"group_id": group_id, "actor_id": "peer1", "cols": 10, "rows": 2},
            )
            self.assertTrue(ok.ok, getattr(ok, "error", None))
            result = ok.result if isinstance(ok.result, dict) else {}
            self.assertIsInstance(result, dict)
            assert isinstance(result, dict)
            self.assertEqual(int(result.get("cols") or 0), 10)
            self.assertEqual(int(result.get("rows") or 0), 2)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
