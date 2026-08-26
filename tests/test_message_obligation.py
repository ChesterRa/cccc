import os
import tempfile
import unittest
from unittest.mock import patch


class TestMessageObligation(unittest.TestCase):
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

        return cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group_with_peer(self):
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import load_group

        response, _ = self._call(
            "group_create", {"title": "obligation", "topic": "", "by": "user"}
        )
        self.assertTrue(response.ok, getattr(response, "error", None))
        group_id = str((response.result or {}).get("group_id") or "").strip()
        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        add_actor(
            group,
            actor_id="peer1",
            runtime="codex",
            runner="pty",
            enabled=True,
        )
        return group, group_id

    def _append_request(self, group, *, by: str = "user", to: str = "peer1"):
        from cccc.contracts.v1 import ChatMessageData
        from cccc.kernel.ledger import append_event

        return append_event(
            group.ledger_path,
            kind="chat.message",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data=ChatMessageData(
                text="please answer",
                to=[to],
                message_mode="request_reply",
            ).model_dump(),
        )

    def test_send_persists_request_reply_mode(self) -> None:
        cleanup = self._with_home()
        try:
            _group, group_id = self._create_group_with_peer()
            response, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "text": "please report status",
                    "by": "user",
                    "to": ["peer1"],
                    "message_mode": "request_reply",
                },
            )
            self.assertTrue(response.ok, getattr(response, "error", None))
            event = (response.result or {}).get("event") or {}
            self.assertEqual(event.get("data", {}).get("message_mode"), "request_reply")
            self.assertNotIn("reply_required", event.get("data", {}))
            self.assertNotIn("requires_ack", event.get("data", {}))
        finally:
            cleanup()

    def test_obligation_lifecycle_is_reply_and_cancel_without_mail_read(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.kernel.inbox import get_obligation_status_batch
        from cccc.kernel.ledger import append_event

        cleanup = self._with_home()
        try:
            group, group_id = self._create_group_with_peer()
            message = self._append_request(group)
            message_id = str(message["id"])

            initial = get_obligation_status_batch(group, [message])[message_id]["peer1"]
            self.assertEqual(
                initial,
                {
                    "replied": False,
                    "reply_requested": True,
                    "cancelled": False,
                    "delivery_state": "",
                },
            )

            consumed, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 50,
                    "by": "peer1",
                },
            )
            self.assertTrue(consumed.ok, getattr(consumed, "error", None))
            self.assertEqual((consumed.result or {})["messages"], [])
            self.assertIsNone((consumed.result or {})["event"])

            reply = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="peer1",
                data=ChatMessageData(
                    text="done",
                    to=["user"],
                    reply_to=message_id,
                    message_mode="send",
                ).model_dump(),
            )
            replied = get_obligation_status_batch(group, [message])[message_id]["peer1"]
            self.assertTrue(replied["replied"])
            self.assertFalse(replied["cancelled"])
            self.assertEqual(reply["data"]["message_mode"], "send")

            second = self._append_request(group)
            cancelled, _ = self._call(
                "reply_request_cancel",
                {
                    "group_id": group_id,
                    "source_event_id": second["id"],
                    "by": "user",
                },
            )
            self.assertTrue(cancelled.ok, getattr(cancelled, "error", None))
            status = get_obligation_status_batch(group, [second])[second["id"]]["peer1"]
            self.assertTrue(status["reply_requested"])
            self.assertTrue(status["cancelled"])
            self.assertFalse(status["replied"])
        finally:
            cleanup()

    def test_first_reply_or_cancellation_is_terminal_in_live_and_cached_status(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.kernel.inbox import get_obligation_status_batch
        from cccc.kernel.ledger import append_event
        from cccc.kernel.ledger_status_cache import (
            get_cached_message_status_batch,
            warm_message_status_cache_from_event,
        )

        cleanup = self._with_home()
        try:
            group, group_id = self._create_group_with_peer()

            cancelled_first = self._append_request(group)
            warm_message_status_cache_from_event(group, cancelled_first["id"])
            cancelled, _ = self._call(
                "reply_request_cancel",
                {
                    "group_id": group_id,
                    "source_event_id": cancelled_first["id"],
                    "by": "user",
                },
            )
            self.assertTrue(cancelled.ok, getattr(cancelled, "error", None))
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="peer1",
                data=ChatMessageData(
                    text="too late",
                    to=["user"],
                    reply_to=cancelled_first["id"],
                    message_mode="send",
                ).model_dump(),
            )
            cancelled_status = get_obligation_status_batch(group, [cancelled_first])[
                cancelled_first["id"]
            ]["peer1"]
            self.assertTrue(cancelled_status["cancelled"])
            self.assertFalse(cancelled_status["replied"])
            self.assertEqual(
                get_cached_message_status_batch(group, [cancelled_first["id"]]), {}
            )

            replied_first = self._append_request(group)
            warm_message_status_cache_from_event(group, replied_first["id"])
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="peer1",
                data=ChatMessageData(
                    text="on time",
                    to=["user"],
                    reply_to=replied_first["id"],
                    message_mode="send",
                ).model_dump(),
            )
            cancelled, _ = self._call(
                "reply_request_cancel",
                {
                    "group_id": group_id,
                    "source_event_id": replied_first["id"],
                    "by": "user",
                },
            )
            self.assertTrue(cancelled.ok, getattr(cancelled, "error", None))
            replied_status = get_obligation_status_batch(group, [replied_first])[
                replied_first["id"]
            ]["peer1"]
            self.assertTrue(replied_status["replied"])
            self.assertFalse(replied_status["cancelled"])
            self.assertEqual(
                get_cached_message_status_batch(group, [replied_first["id"]]), {}
            )
        finally:
            cleanup()

    def test_reply_operation_uses_send_and_never_appends_chat_ack(self) -> None:
        from cccc.kernel.inbox import iter_events

        cleanup = self._with_home()
        try:
            group, group_id = self._create_group_with_peer()
            source = self._append_request(group)
            response, _ = self._call(
                "reply",
                {
                    "group_id": group_id,
                    "reply_to": source["id"],
                    "text": "answered",
                    "by": "peer1",
                    "to": ["user"],
                },
            )
            self.assertTrue(response.ok, getattr(response, "error", None))
            reply = (response.result or {}).get("event") or {}
            self.assertEqual(reply.get("data", {}).get("message_mode"), "send")
            self.assertNotIn("ack_event", response.result or {})
            self.assertNotIn(
                "chat.ack",
                {str(event.get("kind") or "") for event in iter_events(group.ledger_path)},
            )
        finally:
            cleanup()

    def test_mail_reply_fulfills_the_original_obligation_without_prompt_delivery(self) -> None:
        from cccc.kernel.actors import add_actor
        from cccc.kernel.inbox import get_obligation_status_batch

        cleanup = self._with_home()
        try:
            group, group_id = self._create_group_with_peer()
            add_actor(
                group,
                actor_id="peer2",
                runtime="codex",
                runner="pty",
                enabled=True,
            )
            source = self._append_request(group, by="peer1", to="peer2")
            with patch("cccc.daemon.messaging.chat_ops.run_group_chat_post_commit") as schedule_delivery:
                response, _ = self._call(
                    "reply",
                    {
                        "group_id": group_id,
                        "reply_to": source["id"],
                        "text": "answered without interrupting",
                        "by": "peer2",
                        "to": ["peer1"],
                        "message_mode": "mail",
                    },
                )

            self.assertTrue(response.ok, getattr(response, "error", None))
            reply = (response.result or {}).get("event") or {}
            self.assertEqual((response.result or {}).get("message_mode"), "mail")
            self.assertEqual(reply.get("data", {}).get("message_mode"), "mail")
            schedule_delivery.assert_not_called()
            status = get_obligation_status_batch(group, [source])[source["id"]]["peer2"]
            self.assertTrue(status["replied"])
            self.assertFalse(status["cancelled"])
        finally:
            cleanup()

    def test_manual_delivery_claims_before_post_commit_scheduling(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_message_deliver

        cleanup = self._with_home()
        try:
            group, _group_id = self._create_group_with_peer()
            source = self._append_request(group)
            queued: list[object] = []
            args = {
                "group_id": group.group_id,
                "source_event_id": source["id"],
                "actor_ids": ["peer1"],
                "by": "user",
            }

            with patch(
                "cccc.daemon.messaging.chat_ops.run_group_chat_post_commit",
                side_effect=lambda _group_id, _label, fn: queued.append(fn),
            ):
                first = handle_message_deliver(
                    args,
                    coerce_bool=bool,
                    effective_runner_kind=lambda _runtime: "pty",
                    auto_wake_recipients=lambda _group, _actors, _by: [],
                )
                second = handle_message_deliver(
                    args,
                    coerce_bool=bool,
                    effective_runner_kind=lambda _runtime: "pty",
                    auto_wake_recipients=lambda _group, _actors, _by: [],
                )

            self.assertTrue(first.ok, getattr(first, "error", None))
            self.assertFalse(second.ok)
            self.assertEqual(getattr(second.error, "code", ""), "delivery_in_progress")
            self.assertEqual(len(queued), 1)
        finally:
            cleanup()

    def test_manual_delivery_to_web_model_is_claimed_for_the_pull_consumer(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_message_deliver
        from cccc.daemon.runner_state_ops import write_headless_state
        from cccc.kernel.actors import update_actor
        from cccc.kernel.inbox import iter_events

        cleanup = self._with_home()
        try:
            group, group_id = self._create_group_with_peer()
            update_actor(
                group,
                "peer1",
                {
                    "runtime": "web_model",
                    "runner": "headless",
                    "env": {"CCCC_WEB_MODEL_DELIVERY_MODE": "pull"},
                },
            )
            group.doc["running"] = True
            group.save()
            write_headless_state(group_id, "peer1")
            source = self._append_request(group)
            queued: list[object] = []

            with patch(
                "cccc.daemon.messaging.chat_ops.run_group_chat_post_commit",
                side_effect=lambda _group_id, _label, fn: queued.append(fn),
            ):
                response = handle_message_deliver(
                    {
                        "group_id": group_id,
                        "source_event_id": source["id"],
                        "actor_ids": ["peer1"],
                        "by": "user",
                    },
                    coerce_bool=bool,
                    effective_runner_kind=lambda _runner: "headless",
                    auto_wake_recipients=lambda _group, _actors, _by: [],
                )

            self.assertTrue(response.ok, getattr(response, "error", None))
            self.assertEqual(len(queued), 1)
            queued[0]()
            delivery = next(
                event
                for event in reversed(list(iter_events(group.ledger_path)))
                if str(event.get("kind") or "") == "runtime.delivery"
            )
            self.assertEqual(delivery.get("data", {}).get("state"), "claimed")
            self.assertEqual(delivery.get("data", {}).get("transport"), "web_model_pull")

            turn, _ = self._call(
                "runtime_wait_next_turn",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "transport": "web_model_pull",
                },
            )
            self.assertTrue(turn.ok, getattr(turn, "error", None))
            self.assertEqual((turn.result or {}).get("status"), "work_available")
            self.assertEqual(
                ((turn.result or {}).get("turn") or {}).get("event_ids"),
                [source["id"]],
            )
        finally:
            cleanup()

    def test_forced_manual_delivery_reports_later_claim_without_partial_reservation(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.daemon.messaging.chat_ops import handle_message_deliver
        from cccc.daemon.messaging.runtime_delivery import append_delivery_state
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import get_group_state, load_group
        from cccc.kernel.inbox import iter_events
        from cccc.kernel.ledger import append_event

        cleanup = self._with_home()
        try:
            group, _group_id = self._create_group_with_peer()
            add_actor(
                group,
                actor_id="peer2",
                runtime="codex",
                runner="pty",
                enabled=True,
            )
            source = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data=ChatMessageData(
                    text="please answer later",
                    to=["peer1", "peer2"],
                    message_mode="mail",
                ).model_dump(),
            )
            for actor_id, state in (("peer1", "ambiguous"), ("peer2", "claimed")):
                append_delivery_state(
                    group,
                    actor_id=actor_id,
                    actor_created_at="",
                    source_event_id=source["id"],
                    state=state,
                    transport="manual_request",
                )
            group.doc["state"] = "paused"
            group.save()
            queued: list[object] = []

            with patch(
                "cccc.daemon.messaging.chat_ops.run_group_chat_post_commit",
                side_effect=lambda _group_id, _label, fn: queued.append(fn),
            ):
                response = handle_message_deliver(
                    {
                        "group_id": group.group_id,
                        "source_event_id": source["id"],
                        "actor_ids": ["peer1", "peer2"],
                        "by": "user",
                        "force_ambiguous": True,
                    },
                    coerce_bool=bool,
                    effective_runner_kind=lambda _runtime: "pty",
                    auto_wake_recipients=lambda _group, _actors, _by: [],
                )

            self.assertFalse(response.ok)
            self.assertEqual(getattr(response.error, "code", ""), "delivery_in_progress")
            self.assertEqual(getattr(response.error, "details", {}).get("actor_id"), "peer2")
            self.assertEqual(queued, [])
            reloaded = load_group(group.group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            self.assertEqual(get_group_state(reloaded), "paused")
            self.assertEqual(
                sum(
                    1
                    for event in iter_events(group.ledger_path)
                    if str(event.get("kind") or "") == "runtime.delivery"
                ),
                2,
            )
        finally:
            cleanup()

    def test_manual_delivery_claim_resumes_a_paused_or_stopped_group(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_message_deliver
        from cccc.kernel.group import get_group_state, load_group
        from cccc.kernel.inbox import iter_events

        cleanup = self._with_home()
        try:
            for state in ("paused", "stopped"):
                with self.subTest(state=state):
                    group, _group_id = self._create_group_with_peer()
                    source = self._append_request(group)
                    group.doc["state"] = state
                    group.doc["running"] = state != "stopped"
                    group.save()

                    with patch("cccc.daemon.messaging.chat_ops.run_group_chat_post_commit"):
                        response = handle_message_deliver(
                            {
                                "group_id": group.group_id,
                                "source_event_id": source["id"],
                                "actor_ids": ["peer1"],
                                "by": "user",
                            },
                            coerce_bool=bool,
                            effective_runner_kind=lambda _runtime: "pty",
                            auto_wake_recipients=lambda _group, _actors, _by: [],
                        )

                    self.assertTrue(response.ok, getattr(response, "error", None))
                    self.assertEqual((response.result or {}).get("delivery_state"), "claimed")
                    reloaded = load_group(group.group_id)
                    self.assertIsNotNone(reloaded)
                    assert reloaded is not None
                    self.assertEqual(get_group_state(reloaded), "active")
                    self.assertIn(
                        "runtime.delivery",
                        {str(event.get("kind") or "") for event in iter_events(group.ledger_path)},
                    )
        finally:
            cleanup()

    def test_manual_delivery_is_blocked_without_a_claim_for_disabled_actor(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_message_deliver
        from cccc.kernel.actors import update_actor
        from cccc.kernel.inbox import iter_events

        cleanup = self._with_home()
        try:
            group, _group_id = self._create_group_with_peer()
            source = self._append_request(group)
            update_actor(group, "peer1", {"enabled": False})

            response = handle_message_deliver(
                {
                    "group_id": group.group_id,
                    "source_event_id": source["id"],
                    "actor_ids": ["peer1"],
                    "by": "user",
                },
                coerce_bool=bool,
                effective_runner_kind=lambda _runtime: "pty",
                auto_wake_recipients=lambda _group, _actors, _by: [],
            )

            self.assertFalse(response.ok)
            self.assertEqual(getattr(response.error, "code", ""), "delivery_blocked")
            self.assertEqual(
                getattr(response.error, "details", {}).get("reason"),
                "actor_disabled",
            )
            self.assertNotIn(
                "runtime.delivery",
                {str(event.get("kind") or "") for event in iter_events(group.ledger_path)},
            )
        finally:
            cleanup()

    def test_control_events_canonicalize_a_unique_source_event_prefix(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_message_deliver
        from cccc.kernel.inbox import iter_events

        cleanup = self._with_home()
        try:
            group, group_id = self._create_group_with_peer()
            request = self._append_request(group)
            prefix = str(request["id"])[:16]

            cancelled, _ = self._call(
                "reply_request_cancel",
                {
                    "group_id": group_id,
                    "source_event_id": prefix,
                    "by": "user",
                },
            )
            self.assertTrue(cancelled.ok, getattr(cancelled, "error", None))
            cancel_event = (cancelled.result or {}).get("event") or {}
            self.assertEqual(
                cancel_event.get("data", {}).get("source_event_id"), request["id"]
            )

            queued: list[object] = []
            with patch(
                "cccc.daemon.messaging.chat_ops.run_group_chat_post_commit",
                side_effect=lambda _group_id, _label, fn: queued.append(fn),
            ):
                delivered = handle_message_deliver(
                    {
                        "group_id": group_id,
                        "source_event_id": prefix,
                        "actor_ids": ["peer1"],
                        "by": "user",
                    },
                    coerce_bool=bool,
                    effective_runner_kind=lambda _runner: "pty",
                    auto_wake_recipients=lambda _group, _actors, _by: [],
                )
            self.assertTrue(delivered.ok, getattr(delivered, "error", None))
            delivery = next(
                event
                for event in reversed(list(iter_events(group.ledger_path)))
                if str(event.get("kind") or "") == "runtime.delivery"
            )
            self.assertEqual(
                delivery.get("data", {}).get("source_event_id"), request["id"]
            )
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
