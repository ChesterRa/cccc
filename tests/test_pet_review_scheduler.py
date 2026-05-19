import time
import unittest
from pathlib import Path
from unittest.mock import patch
import tempfile
import json

from cccc.daemon.pet import review_scheduler


class _FakeGroup:
    def __init__(self, group_id: str, root: Path) -> None:
        self.group_id = group_id
        self.path = root
        self.doc = {"title": "demo", "state": "active", "actors": []}

    @property
    def ledger_path(self) -> Path:
        self.path.mkdir(parents=True, exist_ok=True)
        path = self.path / "ledger.jsonl"
        if not path.exists():
            path.write_text("", encoding="utf-8")
        return path


class TestPetReviewScheduler(unittest.TestCase):
    def setUp(self) -> None:
        review_scheduler.cancel_pet_review("g-test")

    def tearDown(self) -> None:
        review_scheduler.cancel_pet_review("g-test")

    def test_assistive_job_registry_contains_pet_jobs(self) -> None:
        from cccc.daemon.pet import assistive_jobs

        self.assertIn(assistive_jobs.JOB_KIND_PET_REVIEW, assistive_jobs.registered_assistive_job_kinds())
        self.assertIn(assistive_jobs.JOB_KIND_PET_PROFILE_REFRESH, assistive_jobs.registered_assistive_job_kinds())
        self.assertIn(assistive_jobs.JOB_KIND_VOICE_IDLE_REVIEW, assistive_jobs.registered_assistive_job_kinds())
        with self.assertRaises(ValueError):
            assistive_jobs.request_job(
                "g-test",
                job_kind="unknown",
                trigger_class=assistive_jobs.TRIGGER_EVENT,
            )

    def test_chat_messages_are_debounced_into_single_review(self) -> None:
        emitted: list[tuple[str, set[str], str]] = []

        def _capture(group_id: str, reasons: set[str], source_event_id: str) -> None:
            emitted.append((group_id, set(reasons), source_event_id))

        with patch.object(review_scheduler, "_can_review_now", return_value=True), patch.object(
            review_scheduler,
            "_emit_pet_review",
            side_effect=_capture,
        ), patch.object(review_scheduler, "PET_REVIEW_DEBOUNCE_SECONDS", 0.05), patch.object(
            review_scheduler,
            "PET_REVIEW_MIN_INTERVAL_SECONDS",
            0.01,
        ), patch.object(review_scheduler, "PET_REVIEW_MAX_DELAY_SECONDS", 0.2):
            review_scheduler.request_pet_review("g-test", reason="chat_message", source_event_id="evt-1")
            review_scheduler.request_pet_review("g-test", reason="chat_message", source_event_id="evt-2")
            review_scheduler.request_pet_review("g-test", reason="chat_reply", source_event_id="evt-3")
            time.sleep(0.12)

        self.assertEqual(len(emitted), 1)
        self.assertEqual(emitted[0][0], "g-test")
        self.assertEqual(emitted[0][1], {"chat_message", "chat_reply"})
        self.assertEqual(emitted[0][2], "evt-3")

    def test_immediate_review_bypasses_debounce(self) -> None:
        emitted: list[float] = []

        def _capture(group_id: str, reasons: set[str], source_event_id: str) -> None:
            del group_id, reasons, source_event_id
            emitted.append(time.monotonic())

        with patch.object(review_scheduler, "_can_review_now", return_value=True), patch.object(
            review_scheduler,
            "_emit_pet_review",
            side_effect=_capture,
        ), patch.object(review_scheduler, "PET_REVIEW_DEBOUNCE_SECONDS", 0.2), patch.object(
            review_scheduler,
            "PET_REVIEW_MIN_INTERVAL_SECONDS",
            0.01,
        ), patch.object(review_scheduler, "PET_REVIEW_MAX_DELAY_SECONDS", 0.2):
            started_at = time.monotonic()
            review_scheduler.request_pet_review(
                "g-test",
                reason="actor_stop",
                source_event_id="evt-stop",
                immediate=True,
            )

        self.assertEqual(len(emitted), 1)
        self.assertLess(emitted[0] - started_at, 0.15)

    def test_immediate_review_finishes_before_return_without_pending_timer(self) -> None:
        emitted: list[tuple[str, set[str], str]] = []

        def _capture(group_id: str, reasons: set[str], source_event_id: str) -> None:
            emitted.append((group_id, set(reasons), source_event_id))

        with tempfile.TemporaryDirectory() as tmp, patch.object(
            review_scheduler.assistive_jobs,
            "ensure_home",
            return_value=Path(tmp),
        ), patch.object(review_scheduler, "_can_review_now", return_value=True), patch.object(
            review_scheduler,
            "_emit_pet_review",
            side_effect=_capture,
        ), patch.object(review_scheduler, "PET_REVIEW_DEBOUNCE_SECONDS", 0.2), patch.object(
            review_scheduler,
            "PET_REVIEW_MIN_INTERVAL_SECONDS",
            0.01,
        ), patch.object(review_scheduler, "PET_REVIEW_MAX_DELAY_SECONDS", 0.2):
            review_scheduler.request_pet_review(
                "g-test",
                reason="group_state_changed",
                source_event_id="evt-sync",
                immediate=True,
            )
            pending_path = Path(tmp) / "groups" / "g-test" / "state" / "assistive_jobs.json"
            self.assertEqual(emitted, [("g-test", {"group_state_changed"}, "evt-sync")])
            self.assertTrue(pending_path.exists())
            payload = json.loads(pending_path.read_text(encoding="utf-8"))
            job_payload = ((payload.get("jobs") or {}).get("pet_review") or {})
            self.assertTrue(bool(job_payload.get("in_flight")))
            self.assertFalse(bool(job_payload.get("pending")))

    def test_manual_review_allows_idle_group(self) -> None:
        fake_group = object()
        with patch.object(review_scheduler, "load_group", return_value=fake_group), patch.object(
            review_scheduler,
            "is_desktop_pet_enabled",
            return_value=True,
        ), patch.object(review_scheduler, "get_group_state", return_value="idle"), patch.object(
            review_scheduler,
            "get_pet_actor",
            return_value={"id": "pet-peer", "enabled": True},
        ), patch.object(review_scheduler, "emit_system_notify") as emit_notify:
            accepted = review_scheduler.request_manual_pet_review(
                "g-test",
                reason="bubble_click",
                source_event_id="evt-idle",
            )

        self.assertTrue(accepted)
        emit_notify.assert_called_once()

    def test_immediate_review_dispatches_notify_without_inline_flush(self) -> None:
        fake_group = object()
        with patch.object(review_scheduler, "load_group", return_value=fake_group), patch.object(
            review_scheduler,
            "is_desktop_pet_enabled",
            return_value=True,
        ), patch.object(review_scheduler, "get_group_state", return_value="active"), patch.object(
            review_scheduler,
            "get_pet_actor",
            return_value={"id": "pet-peer", "enabled": True},
        ), patch.object(review_scheduler, "emit_system_notify", return_value={}) as emit_notify:
            review_scheduler.request_pet_review(
                "g-test",
                reason="chat_reply",
                source_event_id="evt-inline",
                immediate=True,
            )

        emit_notify.assert_called_once()
        self.assertIs(emit_notify.call_args.kwargs.get("async_flush"), True)

    def test_review_packet_prefers_reason_bound_focus_task(self) -> None:
        from cccc.kernel.context import Context, ContextStorage, Coordination, CoordinationBrief, Task, TaskStatus, WaitingOn
        from cccc.kernel.ledger import append_event

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp) / "groups" / "g-demo")
            storage = ContextStorage(group)
            storage.save_context(
                Context(
                    coordination=Coordination(
                        brief=CoordinationBrief(
                            current_focus="Keep the waiting_user task open until the user confirms next direction.",
                        )
                    )
                )
            )
            storage.save_task(
                Task(
                    id="T031",
                    title="Confirm whether to continue or close after user feedback arrives",
                    status=TaskStatus.ACTIVE,
                    assignee="foreman",
                    waiting_on=WaitingOn.USER,
                    task_type="standard",
                )
            )
            source_event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={
                    "text": "Please keep T031 open for now. I will review the latest evidence first.",
                    "reply_required": True,
                },
            )

            packet = review_scheduler._build_review_packet(group, {"task_waiting_user", "chat_reply"}, str(source_event["id"]))

        self.assertEqual(str(packet.get("primary_reason") or ""), "task_waiting_user")
        self.assertEqual(packet.get("reasons"), ["task_waiting_user", "chat_reply"])
        self.assertEqual(str(packet.get("group_state") or ""), "active")
        self.assertEqual(
            packet.get("attention"),
            {
                "waiting_user_count": 1,
                "blocked_count": 0,
                "handoff_count": 0,
                "planned_count": 0,
            },
        )
        self.assertEqual(
            packet.get("focus_task"),
            {
                "id": "T031",
                "title": "Confirm whether to continue or close after user feedback arrives",
                "status": "active",
                "assignee": "foreman",
                "waiting_on": "user",
                "task_type": "standard",
            },
        )
        self.assertEqual(str((packet.get("source_event") or {}).get("id") or ""), str(source_event["id"]))
        self.assertEqual(str((packet.get("source_event") or {}).get("kind") or ""), "chat.message")
        self.assertTrue(bool((packet.get("source_event") or {}).get("reply_required")))
        self.assertIn("keep T031 open", str((packet.get("source_event") or {}).get("text") or ""))

    def test_review_packet_prefers_structured_source_task_id_over_generic_reason_pick(self) -> None:
        from cccc.kernel.context import Context, ContextStorage, Coordination, CoordinationBrief, Task, TaskStatus, WaitingOn
        from cccc.kernel.ledger import append_event

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp) / "groups" / "g-demo")
            storage = ContextStorage(group)
            storage.save_context(
                Context(
                    coordination=Coordination(
                        brief=CoordinationBrief(
                            current_focus="Handle the current user dependency, but keep the latest touched task in view.",
                        )
                    )
                )
            )
            storage.save_task(
                Task(
                    id="T030",
                    title="Old waiting-user task that would win by generic reason ranking",
                    status=TaskStatus.ACTIVE,
                    assignee="foreman",
                    waiting_on=WaitingOn.USER,
                    task_type="standard",
                )
            )
            storage.save_task(
                Task(
                    id="T031",
                    title="Newly touched task from context sync",
                    status=TaskStatus.PLANNED,
                    assignee="foreman",
                    task_type="optimization",
                )
            )
            source_event = append_event(
                group.ledger_path,
                kind="context.sync",
                group_id=group.group_id,
                scope_key="",
                by="foreman",
                data={
                    "changes": [
                        {
                            "op": "task.update",
                            "detail": "Updated task T031",
                            "task_id": "T031",
                        }
                    ]
                },
            )

            packet = review_scheduler._build_review_packet(group, {"task_waiting_user", "chat_message"}, str(source_event["id"]))

        self.assertEqual(str((packet.get("source_event") or {}).get("kind") or ""), "context.sync")
        self.assertEqual(str((packet.get("source_event") or {}).get("task_id") or ""), "T031")
        self.assertEqual(
            packet.get("focus_task"),
            {
                "id": "T031",
                "title": "Newly touched task from context sync",
                "status": "planned",
                "assignee": "foreman",
                "task_type": "optimization",
            },
        )

    def test_automatic_review_allows_idle_group(self) -> None:
        emitted: list[tuple[str, set[str], str]] = []

        def _capture(group_id: str, reasons: set[str], source_event_id: str) -> None:
            emitted.append((group_id, set(reasons), source_event_id))

        fake_group = object()
        with patch.object(review_scheduler, "load_group", return_value=fake_group), patch.object(
            review_scheduler,
            "is_desktop_pet_enabled",
            return_value=True,
        ), patch.object(review_scheduler, "get_group_state", return_value="idle"), patch.object(
            review_scheduler,
            "get_pet_actor",
            return_value={"id": "pet-peer", "enabled": True},
        ), patch.object(
            review_scheduler,
            "_emit_pet_review",
            side_effect=_capture,
        ), patch.object(review_scheduler, "PET_REVIEW_DEBOUNCE_SECONDS", 0.05), patch.object(
            review_scheduler,
            "PET_REVIEW_MIN_INTERVAL_SECONDS",
            0.01,
        ), patch.object(review_scheduler, "PET_REVIEW_MAX_DELAY_SECONDS", 0.2):
            review_scheduler.request_pet_review("g-test", reason="chat_message", source_event_id="evt-idle")
            time.sleep(0.12)

        self.assertEqual(emitted, [("g-test", {"chat_message"}, "evt-idle")])

    def test_manual_review_notify_includes_review_packet(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fake_group = _FakeGroup("g-test", Path(tmp) / "groups" / "g-test")
            with patch.object(review_scheduler, "load_group", return_value=fake_group), patch.object(
                review_scheduler,
                "is_desktop_pet_enabled",
                return_value=True,
            ), patch.object(review_scheduler, "get_group_state", return_value="idle"), patch.object(
                review_scheduler,
                "get_pet_actor",
                return_value={"id": "pet-peer", "enabled": True},
            ), patch.object(
                review_scheduler,
                "_build_review_packet",
                return_value={"schema": 1, "primary_reason": "bubble_click", "group_state": "idle"},
            ), patch.object(review_scheduler, "emit_system_notify") as emit_notify:
                accepted = review_scheduler.request_manual_pet_review(
                    "g-test",
                    reason="bubble_click",
                    source_event_id="evt-idle",
                )

            self.assertTrue(accepted)
            emit_notify.assert_called_once()
            notify = emit_notify.call_args.kwargs["notify"]
            self.assertEqual(
                (notify.context or {}).get("review_packet"),
                {"schema": 1, "primary_reason": "bubble_click", "group_state": "idle"},
            )

    def test_manual_review_rejects_paused_group(self) -> None:
        fake_group = object()
        with patch.object(review_scheduler, "load_group", return_value=fake_group), patch.object(
            review_scheduler,
            "is_desktop_pet_enabled",
            return_value=True,
        ), patch.object(review_scheduler, "get_group_state", return_value="paused"), patch.object(
            review_scheduler,
            "get_pet_actor",
            return_value={"id": "pet-peer", "enabled": True},
        ), patch.object(review_scheduler, "emit_system_notify") as emit_notify:
            accepted = review_scheduler.request_manual_pet_review(
                "g-test",
                reason="bubble_click",
                source_event_id="evt-paused",
            )

        self.assertFalse(accepted)
        emit_notify.assert_not_called()

    def test_pending_review_is_persisted_to_disk(self) -> None:
        with tempfile.TemporaryDirectory() as tmp, patch.object(
            review_scheduler.assistive_jobs,
            "ensure_home",
            return_value=Path(tmp),
        ), patch.object(review_scheduler, "_can_review_now", return_value=True), patch.object(
            review_scheduler,
            "PET_REVIEW_DEBOUNCE_SECONDS",
            60.0,
        ), patch.object(review_scheduler, "PET_REVIEW_MIN_INTERVAL_SECONDS", 0.01), patch.object(
            review_scheduler,
            "PET_REVIEW_MAX_DELAY_SECONDS",
            60.0,
        ):
            review_scheduler.request_pet_review("g-test", reason="chat_message", source_event_id="evt-1")
            pending_path = Path(tmp) / "groups" / "g-test" / "state" / "assistive_jobs.json"
            self.assertTrue(pending_path.exists())
            payload = json.loads(pending_path.read_text(encoding="utf-8"))
            job_payload = ((payload.get("jobs") or {}).get("pet_review") or {})
            due_at_wall = float(job_payload.get("due_at_wall") or 0.0)
            self.assertGreater(due_at_wall, time.time() + 30.0)

        review_scheduler.cancel_pet_review("g-test")

    def test_recover_pending_review_replays_after_restart(self) -> None:
        emitted: list[tuple[str, set[str], str]] = []

        def _capture(group_id: str, reasons: set[str], source_event_id: str) -> None:
            emitted.append((group_id, set(reasons), source_event_id))

        with tempfile.TemporaryDirectory() as tmp, patch.object(
            review_scheduler.assistive_jobs,
            "ensure_home",
            return_value=Path(tmp),
        ), patch.object(review_scheduler, "_can_review_now", return_value=True), patch.object(
            review_scheduler,
            "_emit_pet_review",
            side_effect=_capture,
        ), patch.object(review_scheduler, "PET_REVIEW_DEBOUNCE_SECONDS", 0.05), patch.object(
            review_scheduler,
            "PET_REVIEW_MIN_INTERVAL_SECONDS",
            0.01,
        ), patch.object(review_scheduler, "PET_REVIEW_MAX_DELAY_SECONDS", 0.2):
            review_scheduler.request_pet_review("g-test", reason="chat_message", source_event_id="evt-9")
            review_scheduler.cancel_pet_review("g-test")
            pending_path = Path(tmp) / "groups" / "g-test" / "state" / "assistive_jobs.json"
            pending_path.parent.mkdir(parents=True, exist_ok=True)
            pending_path.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": "g-test",
                        "jobs": {
                            "pet_review": {
                                "pending": True,
                                "in_flight": False,
                                "rerun_pending": False,
                                "dirty_since_wall": 1,
                                "due_at_wall": 0,
                                "last_started_wall": 0,
                                "last_finished_wall": 0,
                                "last_trigger_class": "event",
                                "reasons": ["chat_message"],
                                "source_event_id": "evt-9",
                                "suppressed_reason": "",
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )
            review_scheduler.recover_pending_pet_reviews()
            time.sleep(0.08)

        self.assertEqual(emitted, [("g-test", {"chat_message"}, "evt-9")])

    def test_due_review_keeps_pending_when_group_temporarily_cannot_review(self) -> None:
        emitted: list[tuple[str, set[str], str]] = []
        can_review = {"value": False}

        def _capture(group_id: str, reasons: set[str], source_event_id: str) -> None:
            emitted.append((group_id, set(reasons), source_event_id))

        def _can_review_now(_: str) -> bool:
            return bool(can_review["value"])

        with tempfile.TemporaryDirectory() as tmp, patch.object(
            review_scheduler.assistive_jobs,
            "ensure_home",
            return_value=Path(tmp),
        ), patch.object(review_scheduler, "_can_review_now", side_effect=_can_review_now), patch.object(
            review_scheduler,
            "_emit_pet_review",
            side_effect=_capture,
        ), patch.object(review_scheduler, "PET_REVIEW_DEBOUNCE_SECONDS", 0.02), patch.object(
            review_scheduler,
            "PET_REVIEW_MIN_INTERVAL_SECONDS",
            0.01,
        ), patch.object(review_scheduler, "PET_REVIEW_MAX_DELAY_SECONDS", 0.2):
            pending_path = Path(tmp) / "groups" / "g-test" / "state" / "assistive_jobs.json"
            pending_path.parent.mkdir(parents=True, exist_ok=True)
            pending_path.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": "g-test",
                        "jobs": {
                            "pet_review": {
                                "pending": True,
                                "in_flight": False,
                                "rerun_pending": False,
                                "dirty_since_wall": 1,
                                "due_at_wall": 0,
                                "last_started_wall": 0,
                                "last_finished_wall": 0,
                                "last_trigger_class": "event",
                                "reasons": ["chat_message"],
                                "source_event_id": "evt-9",
                                "suppressed_reason": "",
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )
            review_scheduler.recover_pending_pet_reviews()
            self.assertEqual(emitted, [])
            self.assertTrue(pending_path.exists())

            can_review["value"] = True
            review_scheduler.request_pet_review("g-test", reason="group_state_changed", source_event_id="evt-10", immediate=True)
            time.sleep(0.08)

        self.assertEqual(emitted, [("g-test", {"chat_message", "group_state_changed"}, "evt-10")])


    def test_review_reruns_after_in_flight_completion_when_new_signal_arrives(self) -> None:
        from cccc.daemon.pet import assistive_jobs

        emitted: list[tuple[str, set[str], str]] = []

        def _capture(group_id: str, reasons: set[str], source_event_id: str) -> None:
            emitted.append((group_id, set(reasons), source_event_id))

        with tempfile.TemporaryDirectory() as tmp, patch.object(
            review_scheduler.assistive_jobs,
            "ensure_home",
            return_value=Path(tmp),
        ), patch.object(review_scheduler, "_can_review_now", return_value=True), patch.object(
            review_scheduler,
            "_emit_pet_review",
            side_effect=_capture,
        ), patch.object(review_scheduler, "PET_REVIEW_DEBOUNCE_SECONDS", 0.02), patch.object(
            review_scheduler,
            "PET_REVIEW_MIN_INTERVAL_SECONDS",
            0.01,
        ), patch.object(review_scheduler, "PET_REVIEW_MAX_DELAY_SECONDS", 0.2):
            review_scheduler.request_pet_review(
                "g-test",
                reason="chat_message",
                source_event_id="evt-1",
                immediate=True,
            )
            review_scheduler.request_pet_review("g-test", reason="chat_reply", source_event_id="evt-2")
            time.sleep(0.04)
            self.assertEqual(emitted, [("g-test", {"chat_message"}, "evt-1")])

            assistive_jobs.mark_job_completed("g-test", assistive_jobs.JOB_KIND_PET_REVIEW)
            time.sleep(0.08)

        self.assertEqual(
            emitted,
            [
                ("g-test", {"chat_message"}, "evt-1"),
                ("g-test", {"chat_message", "chat_reply"}, "evt-2"),
            ],
        )

    def test_recover_replays_stale_in_flight_review(self) -> None:
        emitted: list[tuple[str, set[str], str]] = []

        def _capture(group_id: str, reasons: set[str], source_event_id: str) -> None:
            emitted.append((group_id, set(reasons), source_event_id))

        with tempfile.TemporaryDirectory() as tmp, patch.object(
            review_scheduler.assistive_jobs,
            "ensure_home",
            return_value=Path(tmp),
        ), patch.object(review_scheduler, "_can_review_now", return_value=True), patch.object(
            review_scheduler,
            "_emit_pet_review",
            side_effect=_capture,
        ), patch.object(review_scheduler, "PET_REVIEW_LEASE_SECONDS", 0.01), patch.object(
            review_scheduler,
            "PET_REVIEW_DEBOUNCE_SECONDS",
            0.02,
        ), patch.object(review_scheduler, "PET_REVIEW_MIN_INTERVAL_SECONDS", 0.01), patch.object(
            review_scheduler,
            "PET_REVIEW_MAX_DELAY_SECONDS",
            0.2,
        ):
            pending_path = Path(tmp) / "groups" / "g-test" / "state" / "assistive_jobs.json"
            pending_path.parent.mkdir(parents=True, exist_ok=True)
            pending_path.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": "g-test",
                        "jobs": {
                            "pet_review": {
                                "pending": False,
                                "in_flight": True,
                                "rerun_pending": False,
                                "dirty_since_wall": time.time() - 60,
                                "due_at_wall": 0,
                                "last_started_wall": time.time() - 60,
                                "last_finished_wall": 0,
                                "last_trigger_class": "event",
                                "reasons": ["chat_message"],
                                "source_event_id": "evt-stale",
                                "suppressed_reason": "",
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )
            review_scheduler.recover_pending_pet_reviews()
            time.sleep(0.08)

        self.assertEqual(emitted, [("g-test", {"chat_message"}, "evt-stale")])


if __name__ == "__main__":
    unittest.main()
