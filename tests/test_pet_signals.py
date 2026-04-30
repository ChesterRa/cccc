import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class _FakeGroup:
    def __init__(self, group_id: str, root: Path) -> None:
        self.group_id = group_id
        self.path = root
        self.doc = {
            "title": "demo",
            "actors": [
                {"id": "foreman-1", "enabled": True, "created_at": "2026-03-26T10:00:00Z"},
                {"id": "peer-1", "enabled": True, "created_at": "2026-03-26T10:00:00Z"},
            ],
        }

    @property
    def ledger_path(self) -> Path:
        self.path.mkdir(parents=True, exist_ok=True)
        return self.path / "ledger.jsonl"


class TestPetSignals(unittest.TestCase):
    def test_reply_pressure_and_rhythm_signals(self) -> None:
        from cccc.kernel.ledger import append_event
        from cccc.kernel.pet_signals import load_pet_signals
        from cccc.util.time import utc_now_iso

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            recent_ts = utc_now_iso()
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="foreman-1",
                data={
                    "text": "Need a reply",
                    "reply_required": True,
                    "to": ["peer-1"],
                },
            )
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="peer-1",
                data={
                    "text": "Ack",
                    "reply_to": "",
                    "to": ["foreman-1"],
                },
            )
            first = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="foreman-1",
                data={
                    "text": "Reply to this",
                    "reply_required": True,
                    "to": ["peer-1"],
                },
            )
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="peer-1",
                data={
                    "text": "Replying",
                    "reply_to": str(first.get("id") or ""),
                    "to": ["foreman-1"],
                },
            )
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="foreman-1",
                data={
                    "text": "Still waiting",
                    "reply_required": True,
                    "to": ["peer-1"],
                },
            )
            append_event(
                group.ledger_path,
                kind="context.sync",
                group_id=group.group_id,
                scope_key="",
                by="foreman-1",
                data={
                    "version": "v1",
                    "changes": [
                        {"index": 0, "op": "task.create", "detail": "Created task T5"},
                        {"index": 1, "op": "task.move", "detail": "Moved task T2 to active"},
                        {"index": 2, "op": "task.update", "detail": "Updated task T1"},
                    ],
                },
            )
            append_event(
                group.ledger_path,
                kind="context.sync",
                group_id=group.group_id,
                scope_key="",
                by="peer-1",
                data={
                    "version": "v2",
                    "changes": [
                        {"index": 0, "op": "task.delete", "detail": "Deleted task T9"},
                    ],
                },
            )

            signals = load_pet_signals(
                group,
                context_payload={
                    "attention": {
                        "blocked": [
                            {"id": "T1", "updated_at": recent_ts},
                        ],
                        "waiting_user": [
                            {"id": "T2", "updated_at": recent_ts},
                            {"id": "T3", "updated_at": recent_ts},
                        ],
                        "pending_handoffs": [
                            {"id": "T4", "updated_at": recent_ts},
                        ],
                    },
                    "planned_backlog_tasks": [],
                },
            )

        self.assertEqual(int(signals["reply_pressure"]["pending_count"]), 2)
        self.assertGreaterEqual(int(signals["reply_pressure"]["baseline_median_reply_seconds"]), 0)
        self.assertEqual(str(signals["coordination_rhythm"]["foreman_id"]), "foreman-1")
        self.assertEqual(int(signals["task_pressure"]["waiting_user_count"]), 2)
        self.assertEqual(str(signals["task_pressure"]["severity"]), "high")
        self.assertGreaterEqual(int(signals["task_pressure"]["trend_score"]), 4)
        self.assertEqual(int(signals["task_pressure"]["recent_task_create_ops"]), 1)
        self.assertEqual(int(signals["task_pressure"]["recent_task_move_ops"]), 1)
        self.assertEqual(int(signals["task_pressure"]["recent_task_update_ops"]), 1)
        self.assertEqual(int(signals["task_pressure"]["recent_task_delete_ops"]), 1)
        self.assertEqual(int(signals["task_pressure"]["recent_task_change_count"]), 4)
        self.assertEqual(int(signals["task_pressure"]["recent_task_context_sync_events"]), 2)
        self.assertGreaterEqual(int(signals["task_pressure"]["ledger_trend_score"]), 5)
        self.assertTrue(bool(signals["proposal_ready"]["ready"]))
        self.assertEqual(str(signals["proposal_ready"]["focus"]), "waiting_user")
        self.assertIn("user", str(signals["proposal_ready"]["summary"]).lower())

    def test_lightweight_signals_skip_obligation_and_context_sync_scans(self) -> None:
        from cccc.kernel.ledger import append_event
        from cccc.kernel.pet_signals import load_pet_signals

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="foreman-1",
                data={
                    "text": "Need a reply",
                    "reply_required": True,
                    "to": ["peer-1"],
                },
            )
            append_event(
                group.ledger_path,
                kind="context.sync",
                group_id=group.group_id,
                scope_key="",
                by="foreman-1",
                data={
                    "version": "v1",
                    "changes": [
                        {"index": 0, "op": "task.create", "detail": "Created task T5"},
                    ],
                },
            )

            with patch(
                "cccc.kernel.pet_signals.get_obligation_status_batch",
                side_effect=AssertionError("lightweight pet signals should not compute full obligations"),
            ):
                signals = load_pet_signals(
                    group,
                    context_payload={},
                    recent_chat_limit=10,
                    recent_chat_source="active_tail",
                    context_sync_limit=0,
                    include_reply_obligation_status=False,
                )

        self.assertEqual(int(signals["reply_pressure"]["pending_count"]), 1)
        self.assertEqual(int(signals["task_pressure"]["recent_task_change_count"]), 0)
        self.assertEqual(int(signals["task_pressure"]["ledger_trend_score"]), 0)


if __name__ == "__main__":
    unittest.main()
