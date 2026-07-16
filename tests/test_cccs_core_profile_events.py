import json
import os
import tempfile
import unittest
from pathlib import Path


class TestCCCSCoreProfileEvents(unittest.TestCase):
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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_core_profile_event_kinds_are_emitted(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "cccs-core", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_actor_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                },
            )
            self.assertTrue(add_actor_resp.ok, getattr(add_actor_resp, "error", None))

            send_resp, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "text": "attention ping",
                    "to": ["peer1"],
                    "priority": "attention",
                },
            )
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            chat_event = (send_resp.result or {}).get("event") if isinstance(send_resp.result, dict) else {}
            chat_event_id = str((chat_event or {}).get("id") or "").strip()
            self.assertTrue(chat_event_id)

            ack_resp, _ = self._call(
                "chat_ack",
                {"group_id": group_id, "actor_id": "peer1", "event_id": chat_event_id, "by": "peer1"},
            )
            self.assertTrue(ack_resp.ok, getattr(ack_resp, "error", None))

            read_resp, _ = self._call(
                "inbox_mark_read",
                {"group_id": group_id, "actor_id": "peer1", "event_id": chat_event_id, "by": "peer1"},
            )
            self.assertTrue(read_resp.ok, getattr(read_resp, "error", None))

            notify_resp, _ = self._call(
                "system_notify",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "info",
                    "title": "t",
                    "message": "m",
                    "target_actor_id": "peer1",
                    "requires_ack": True,
                },
            )
            self.assertTrue(notify_resp.ok, getattr(notify_resp, "error", None))
            notify_event = (notify_resp.result or {}).get("event") if isinstance(notify_resp.result, dict) else {}
            notify_event_id = str((notify_event or {}).get("id") or "").strip()
            self.assertTrue(notify_event_id)

            notify_ack_resp, _ = self._call(
                "notify_ack",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "notify_event_id": notify_event_id,
                    "by": "peer1",
                },
            )
            self.assertTrue(notify_ack_resp.ok, getattr(notify_ack_resp, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            ledger_path = Path(group.ledger_path)
            self.assertTrue(ledger_path.exists())

            kinds = set()
            with ledger_path.open("r", encoding="utf-8") as f:
                for line in f:
                    raw = line.strip()
                    if not raw:
                        continue
                    obj = json.loads(raw)
                    kinds.add(str(obj.get("kind") or ""))

            self.assertTrue({"chat.message", "chat.ack", "chat.read", "system.notify", "system.notify_ack"}.issubset(kinds))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
