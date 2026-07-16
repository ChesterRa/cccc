from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestSlashSkillDispatch(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
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

    def test_dispatches_hidden_skill_turn_as_hidden_chat_ledger_message(self) -> None:
        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "slash-skill", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "architect",
                    "title": "负责架构设计",
                    "runtime": "codex",
                    "runner": "pty",
                    "enabled": True,
                },
            )
            self.assertTrue(added.ok, getattr(added, "error", None))

            deliveries: list[dict] = []

            def capture_delivery(**kwargs):
                deliveries.append(kwargs)

            with patch("cccc.daemon.messaging.slash_skill_dispatch_ops.deliver_chat_message", side_effect=capture_delivery):
                resp, _ = self._call(
                    "slash_skill_dispatch",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "to": ["architect"],
                        "task_text": "开始执行",
                        "command": "/using-superpowers",
                        "capability_id": "skill:agent_self_proposed:using-superpowers",
                        "priority": "attention",
                        "reply_required": True,
                        "client_id": "client-1",
                        "reply_to": "evt-original",
                        "quote_text": "原始请求",
                    },
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result or {}
            self.assertTrue(bool(result.get("hidden")))
            self.assertEqual(str(result.get("capability_id") or ""), "skill:agent_self_proposed:using-superpowers")
            self.assertEqual(str(result.get("command") or ""), "/using-superpowers")

            self.assertEqual(len(deliveries), 1)
            delivery = deliveries[0]
            self.assertEqual(delivery.get("effective_to"), ["architect"])
            self.assertEqual(str(delivery.get("by") or ""), "user")
            self.assertEqual(str(delivery.get("reply_to") or ""), "evt-original")
            text = str(delivery.get("delivery_text") or "")
            self.assertIn("INTERNAL CONTROL", text)
            self.assertIn("CCCC capability skill", text)
            self.assertIn("skill:agent_self_proposed:using-superpowers", text)
            self.assertIn("/using-superpowers", text)
            self.assertIn("run `cccc_help` first", text)
            self.assertIn("开始执行", text)

            ledger_path = Path(os.environ["CCCC_HOME"]) / "groups" / group_id / "ledger.jsonl"
            chat_events = []
            for line in ledger_path.read_text(encoding="utf-8").splitlines():
                event = json.loads(line)
                if str(event.get("kind") or "") == "chat.message":
                    chat_events.append(event)
            self.assertEqual(len(chat_events), 1)
            hidden_event = chat_events[0]
            data = hidden_event.get("data") if isinstance(hidden_event.get("data"), dict) else {}
            self.assertEqual(str(data.get("text") or ""), text)
            refs = data.get("refs") if isinstance(data.get("refs"), list) else []
            control_ref = refs[0] if refs and isinstance(refs[0], dict) else {}
            self.assertTrue(bool(control_ref.get("hidden")))
            self.assertEqual(str(control_ref.get("control_kind") or ""), "slash_skill_dispatch")
            self.assertEqual(str(control_ref.get("capability_id") or ""), "skill:agent_self_proposed:using-superpowers")

            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import unread_messages

            group = load_group(group_id)
            self.assertIsNotNone(group)
            unread = unread_messages(group, actor_id="architect", limit=10, kind_filter="all")
            self.assertEqual([str(item.get("id") or "") for item in unread if isinstance(item, dict)], [str(hidden_event.get("id") or "")])
        finally:
            cleanup()

    def test_replays_existing_dispatch_for_same_client_id(self) -> None:
        _, cleanup = self._with_home()
        try:
            created, _ = self._call("group_create", {"title": "slash-skill-idem", "topic": "", "by": "user"})
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "architect",
                    "runtime": "codex",
                    "runner": "pty",
                    "enabled": True,
                },
            )
            self.assertTrue(added.ok, getattr(added, "error", None))

            deliveries: list[dict] = []

            def capture_delivery(**kwargs):
                deliveries.append(kwargs)

            dispatch_args = {
                "group_id": group_id,
                "by": "user",
                "to": ["architect"],
                "task_text": "run once",
                "command": "/using-superpowers",
                "capability_id": "skill:agent_self_proposed:using-superpowers",
                "client_id": "client-retry-1",
            }
            with patch("cccc.daemon.messaging.slash_skill_dispatch_ops.deliver_chat_message", side_effect=capture_delivery):
                first, _ = self._call("slash_skill_dispatch", dispatch_args)
                second, _ = self._call("slash_skill_dispatch", dict(dispatch_args))

            self.assertTrue(first.ok, getattr(first, "error", None))
            self.assertTrue(second.ok, getattr(second, "error", None))
            first_result = first.result or {}
            second_result = second.result or {}
            self.assertNotIn("replayed", first_result)
            self.assertTrue(bool(second_result.get("replayed")))
            self.assertEqual(str(second_result.get("event_id") or ""), str(first_result.get("event_id") or ""))
            self.assertTrue(str(second_result.get("event_id") or ""))

            self.assertEqual(len(deliveries), 1)

            ledger_path = Path(os.environ["CCCC_HOME"]) / "groups" / group_id / "ledger.jsonl"
            chat_events = [
                json.loads(line)
                for line in ledger_path.read_text(encoding="utf-8").splitlines()
                if str(json.loads(line).get("kind") or "") == "chat.message"
            ]
            self.assertEqual(len(chat_events), 1)
        finally:
            cleanup()
