from types import SimpleNamespace
from unittest.mock import ANY, patch

from cccc.daemon.messaging.actor_delivery_planner import (
    TRANSPORT_CLAUDE_HEADLESS,
    TRANSPORT_CODEX_HEADLESS,
    TRANSPORT_PTY,
    TRANSPORT_SKIP,
    TRANSPORT_WEB_MODEL_BROWSER,
    event_with_effective_to,
    plan_actor_chat_delivery,
)


def _group(*actors: dict):
    return SimpleNamespace(group_id="g-test", doc={"actors": list(actors)})


def _event(to: list[str] | None = None) -> dict:
    return {
        "kind": "chat.message",
        "id": "evt-1",
        "data": {"text": "hello", "to": list(to or [])},
    }


def _runner(value: str) -> str:
    return value or "pty"


def test_event_with_effective_to_does_not_mutate_original_event() -> None:
    event = _event(["old"])
    out = event_with_effective_to(event, ["new"])

    assert event["data"]["to"] == ["old"]
    assert out["data"]["to"] == ["new"]


def test_planner_routes_targeted_pty_actor() -> None:
    actor = {"id": "peer1", "runner": "pty", "runtime": "codex"}
    decision = plan_actor_chat_delivery(
        group=_group({"id": "foreman"}, actor),
        actor=actor,
        event=_event(),
        by="user",
        effective_to=["peer1"],
        effective_runner_kind=_runner,
        codex_headless_running=lambda _group_id, _actor_id: False,
        claude_headless_running=lambda _group_id, _actor_id: False,
    )

    assert decision.actor_id == "peer1"
    assert decision.transport == TRANSPORT_PTY
    assert decision.reason == "pty_runner"


def test_planner_skips_sender_and_non_targeted_actor() -> None:
    actor = {"id": "peer1", "runner": "pty", "runtime": "codex"}
    sender_decision = plan_actor_chat_delivery(
        group=_group(actor),
        actor=actor,
        event=_event(),
        by="peer1",
        effective_to=["peer1"],
        effective_runner_kind=_runner,
        codex_headless_running=lambda _group_id, _actor_id: False,
        claude_headless_running=lambda _group_id, _actor_id: False,
    )
    other_decision = plan_actor_chat_delivery(
        group=_group({"id": "foreman"}, actor),
        actor=actor,
        event=_event(),
        by="user",
        effective_to=["foreman"],
        effective_runner_kind=_runner,
        codex_headless_running=lambda _group_id, _actor_id: False,
        claude_headless_running=lambda _group_id, _actor_id: False,
    )

    assert sender_decision.transport == TRANSPORT_SKIP
    assert sender_decision.reason == "sender"
    assert other_decision.transport == TRANSPORT_SKIP
    assert other_decision.reason == "not_targeted"


def test_planner_routes_running_codex_headless_actor() -> None:
    actor = {"id": "peer1", "runner": "headless", "runtime": "codex"}
    seen: list[tuple[str, str]] = []

    def running(group_id: str, actor_id: str) -> bool:
        seen.append((group_id, actor_id))
        return True

    decision = plan_actor_chat_delivery(
        group=_group(actor),
        actor=actor,
        event=_event(),
        by="user",
        effective_to=["peer1"],
        effective_runner_kind=_runner,
        codex_headless_running=running,
        claude_headless_running=lambda _group_id, _actor_id: False,
    )

    assert decision.transport == TRANSPORT_CODEX_HEADLESS
    assert decision.reason == "codex_headless_running"
    assert seen == [("g-test", "peer1")]


def test_planner_routes_running_claude_headless_actor() -> None:
    actor = {"id": "peer1", "runner": "headless", "runtime": "claude"}
    decision = plan_actor_chat_delivery(
        group=_group(actor),
        actor=actor,
        event=_event(),
        by="user",
        effective_to=["peer1"],
        effective_runner_kind=_runner,
        codex_headless_running=lambda _group_id, _actor_id: False,
        claude_headless_running=lambda _group_id, _actor_id: True,
    )

    assert decision.transport == TRANSPORT_CLAUDE_HEADLESS
    assert decision.reason == "claude_headless_running"


def test_planner_keeps_stopped_headless_actor_out_of_direct_delivery() -> None:
    actor = {"id": "peer1", "runner": "headless", "runtime": "codex"}
    decision = plan_actor_chat_delivery(
        group=_group(actor),
        actor=actor,
        event=_event(),
        by="user",
        effective_to=["peer1"],
        effective_runner_kind=_runner,
        codex_headless_running=lambda _group_id, _actor_id: False,
        claude_headless_running=lambda _group_id, _actor_id: False,
    )

    assert decision.transport == TRANSPORT_SKIP
    assert decision.reason == "codex_headless_not_running"


def test_planner_leaves_web_model_delivery_for_pull_runtime() -> None:
    actor = {"id": "peer1", "runner": "headless", "runtime": "web_model"}
    decision = plan_actor_chat_delivery(
        group=_group(actor),
        actor=actor,
        event=_event(),
        by="user",
        effective_to=["peer1"],
        effective_runner_kind=_runner,
        codex_headless_running=lambda _group_id, _actor_id: False,
        claude_headless_running=lambda _group_id, _actor_id: False,
    )

    assert decision.transport == TRANSPORT_SKIP
    assert decision.reason == "web_model_pull_runtime"


def test_planner_routes_web_model_when_browser_delivery_enabled() -> None:
    actor = {"id": "peer1", "runner": "headless", "runtime": "web_model"}
    decision = plan_actor_chat_delivery(
        group=_group(actor),
        actor=actor,
        event=_event(),
        by="user",
        effective_to=["peer1"],
        effective_runner_kind=_runner,
        codex_headless_running=lambda _group_id, _actor_id: False,
        claude_headless_running=lambda _group_id, _actor_id: False,
        web_model_browser_delivery_enabled=lambda group_id, actor_arg: group_id == "g-test" and actor_arg is actor,
    )

    assert decision.transport == TRANSPORT_WEB_MODEL_BROWSER
    assert decision.reason == "web_model_browser_delivery"


def test_handle_send_uses_same_planner_for_claude_headless_actor(monkeypatch, tmp_path) -> None:
    from cccc.contracts.v1 import DaemonRequest
    from cccc.daemon.messaging.chat_ops import handle_send
    from cccc.daemon.server import handle_request

    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    create_resp, _ = handle_request(
        DaemonRequest.model_validate({"op": "group_create", "args": {"title": "planner", "topic": "", "by": "user"}})
    )
    assert create_resp.ok
    group_id = str((create_resp.result or {}).get("group_id") or "").strip()
    assert group_id

    add_resp, _ = handle_request(
        DaemonRequest.model_validate(
            {
                "op": "actor_add",
                "args": {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "claude",
                    "runner": "headless",
                    "by": "user",
                },
            }
        )
    )
    assert add_resp.ok

    with (
        patch("cccc.daemon.messaging.chat_ops.claude_app_supervisor.actor_running", return_value=True),
        patch("cccc.daemon.messaging.chat_ops.claude_app_supervisor.submit_user_message", return_value=True) as submit,
        patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
        patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush,
    ):
        resp = handle_send(
            {
                "group_id": group_id,
                "by": "user",
                "text": "hello claude",
                "to": ["peer1"],
                "client_id": "planner-claude",
            },
            coerce_bool=bool,
            normalize_attachments=lambda _group, _attachments: [],
            effective_runner_kind=lambda runner: str(runner or "pty"),
            auto_wake_recipients=lambda _group, _to, _by: [],
            automation_on_resume=lambda _group: None,
            automation_on_new_message=lambda _group: None,
            clear_pending_system_notifies=lambda _group_id, _reasons: None,
        )

    assert resp.ok
    submit.assert_called_once()
    submitted_text = str(submit.call_args.kwargs.get("text") or "")
    assert "[cccc] user → peer1:" in submitted_text
    assert "hello claude" in submitted_text
    queue_chat_message.assert_not_called()
    request_flush.assert_not_called()


def test_handle_send_schedules_browser_delivery_for_web_model_actor(monkeypatch, tmp_path) -> None:
    from cccc.contracts.v1 import DaemonRequest
    from cccc.daemon.messaging.chat_ops import handle_send
    from cccc.daemon.server import handle_request

    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    create_resp, _ = handle_request(
        DaemonRequest.model_validate(
            {"op": "group_create", "args": {"title": "planner-web-model", "topic": "", "by": "user"}}
        )
    )
    assert create_resp.ok
    group_id = str((create_resp.result or {}).get("group_id") or "").strip()
    assert group_id

    add_resp, _ = handle_request(
        DaemonRequest.model_validate(
            {
                "op": "actor_add",
                "args": {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Web Model",
                    "runtime": "web_model",
                    "runner": "headless",
                    "by": "user",
                },
            }
        )
    )
    assert add_resp.ok

    with (
        patch("cccc.daemon.messaging.chat_ops.web_model_browser_delivery_enabled", return_value=True),
        patch("cccc.daemon.messaging.chat_ops.schedule_web_model_browser_delivery", return_value=True) as schedule,
        patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
        patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush,
    ):
        resp = handle_send(
            {
                "group_id": group_id,
                "by": "user",
                "text": "hello web model",
                "to": ["peer1"],
                "client_id": "planner-web-model",
            },
            coerce_bool=bool,
            normalize_attachments=lambda _group, _attachments: [],
            effective_runner_kind=lambda runner: str(runner or "pty"),
            auto_wake_recipients=lambda _group, _to, _by: [],
            automation_on_resume=lambda _group: None,
            automation_on_new_message=lambda _group: None,
            clear_pending_system_notifies=lambda _group_id, _reasons: None,
        )

    assert resp.ok
    event = (resp.result or {}).get("event") if isinstance(resp.result, dict) else {}
    event_id = str((event or {}).get("id") or "").strip()
    assert event_id
    schedule.assert_called_once_with(group_id=group_id, actor_id="peer1", trigger_event_id=event_id, logger=ANY)
    queue_chat_message.assert_not_called()
    request_flush.assert_not_called()
