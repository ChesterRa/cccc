from types import SimpleNamespace
from unittest.mock import ANY, patch

import pytest

from cccc.daemon.messaging.actor_delivery_planner import (
    TRANSPORT_DEEPSEEK_HEADLESS,
    TRANSPORT_SKIP,
    plan_actor_chat_delivery,
)


def _planner_decision(*, running: bool):
    actor = {"id": "deepseek-1", "runner": "headless", "runtime": "deepseek"}
    return plan_actor_chat_delivery(
        group=SimpleNamespace(group_id="g-test", doc={"actors": [actor]}),
        actor=actor,
        event={
            "kind": "chat.message",
            "id": "evt-1",
            "data": {"text": "hello", "to": []},
        },
        by="user",
        effective_to=["deepseek-1"],
        effective_runner_kind=lambda value: value or "pty",
        codex_headless_running=lambda _group_id, _actor_id: False,
        claude_headless_running=lambda _group_id, _actor_id: False,
        deepseek_headless_running=lambda _group_id, _actor_id: running,
    )


def test_planner_routes_only_running_deepseek_headless_actor() -> None:
    running = _planner_decision(running=True)
    stopped = _planner_decision(running=False)

    assert running.transport == TRANSPORT_DEEPSEEK_HEADLESS
    assert running.reason == "deepseek_headless_running"
    assert stopped.transport == TRANSPORT_SKIP
    assert stopped.reason == "deepseek_headless_not_running"


def _create_deepseek_group(tmp_path, monkeypatch) -> str:
    from cccc.contracts.v1 import DaemonRequest
    from cccc.daemon.server import handle_request

    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    created, _ = handle_request(
        DaemonRequest.model_validate(
            {
                "op": "group_create",
                "args": {"title": "planner-deepseek", "topic": "", "by": "user"},
            }
        )
    )
    assert created.ok
    group_id = str((created.result or {}).get("group_id") or "").strip()
    added, _ = handle_request(
        DaemonRequest.model_validate(
            {
                "op": "actor_add",
                "args": {
                    "group_id": group_id,
                    "actor_id": "deepseek-1",
                    "title": "DeepSeek",
                    "runtime": "deepseek",
                    "runner": "headless",
                    "by": "user",
                },
            }
        )
    )
    assert added.ok
    return group_id


@pytest.mark.parametrize(
    ("running", "woken", "text"),
    [(True, False, "hello deepseek"), (False, True, "wake and deliver")],
)
def test_send_queues_deepseek_once_and_suppresses_generic_notify(
    tmp_path,
    monkeypatch,
    running: bool,
    woken: bool,
    text: str,
) -> None:
    from cccc.daemon.messaging.chat_ops import handle_send

    group_id = _create_deepseek_group(tmp_path, monkeypatch)
    with (
        patch(
            "cccc.daemon.messaging.chat_delivery_ops.deepseek_runtime.running",
            return_value=running,
        ),
        patch(
            "cccc.daemon.messaging.chat_delivery_ops.queue_chat_message"
        ) as queue_chat_message,
        patch(
            "cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages"
        ) as request_flush,
        patch(
            "cccc.daemon.messaging.chat_delivery_ops.emit_system_notify"
        ) as emit_notify,
    ):
        response = handle_send(
            {
                "group_id": group_id,
                "by": "user",
                "text": text,
                "to": ["deepseek-1"],
                "client_id": f"planner-deepseek-{int(woken)}",
            },
            coerce_bool=bool,
            normalize_attachments=lambda _group, _attachments: [],
            effective_runner_kind=lambda runner: str(runner or "pty"),
            auto_wake_recipients=lambda _group, _to, _by: (
                ["deepseek-1"] if woken else []
            ),
            automation_on_resume=lambda _group: None,
            automation_on_new_message=lambda _group: None,
            clear_pending_system_notifies=lambda _group_id, _reasons: None,
        )

    assert response.ok
    event = (
        (response.result or {}).get("event")
        if isinstance(response.result, dict)
        else {}
    )
    queue_chat_message.assert_called_once()
    assert queue_chat_message.call_args.kwargs["actor_id"] == "deepseek-1"
    assert queue_chat_message.call_args.kwargs["event_id"] == event.get("id")
    assert text in str(queue_chat_message.call_args.kwargs["text"])
    assert queue_chat_message.call_args.kwargs["deduplicate_by_event_id"] is True
    request_flush.assert_called_once_with(ANY, actor_id="deepseek-1")
    emit_notify.assert_not_called()


def test_deepseek_chat_prompt_includes_mcp_reply_reminder(tmp_path, monkeypatch) -> None:
    from cccc.daemon.actors import deepseek_runtime
    from cccc.daemon.messaging.deepseek_delivery import deliver_messages
    from cccc.daemon.messaging.delivery import MCP_REMINDER_LINE, PendingMessage

    class FakeSupervisor:
        session_id = "fake-session"

        def __init__(self) -> None:
            self.prompts: list[str] = []

        def submit(self, prompt: str) -> int:
            self.prompts.append(prompt)
            return 3

        def next_frame(self, *, timeout: float):
            del timeout
            return {
                "jsonrpc": "2.0",
                "id": 3,
                "result": {"stopReason": "end_turn"},
            }

    supervisor = FakeSupervisor()
    monkeypatch.setattr(deepseek_runtime, "get", lambda **_kwargs: supervisor)
    group = SimpleNamespace(group_id="g-test", path=tmp_path)
    message = PendingMessage(
        event_id="event-1",
        by="user",
        to=["deepseek-1"],
        text="hello deepseek",
        ts="2026-08-19T00:00:00Z",
    )

    assert deliver_messages(group, actor_id="deepseek-1", messages=[message]) is True
    assert len(supervisor.prompts) == 1
    assert "hello deepseek" in supervisor.prompts[0]
    assert MCP_REMINDER_LINE in supervisor.prompts[0]
