from concurrent.futures import ThreadPoolExecutor
from threading import Barrier


def _group(tmp_path):
    from cccc.kernel.group import Group

    path = tmp_path / "groups" / "g_runtime_delivery"
    path.mkdir(parents=True)
    return Group(
        group_id="g_runtime_delivery",
        path=path,
        doc={
            "actors": [
                {
                    "id": "peer1",
                    "enabled": True,
                    "created_at": "2026-08-22T00:00:00Z",
                    "runtime": "codex",
                    "runner": "headless",
                }
            ]
        },
    )


def test_concurrent_claims_are_unique_and_restart_settles_stranded_claim(tmp_path):
    from cccc.daemon.messaging.runtime_delivery import (
        claim_delivery,
        latest_delivery_state,
        settle_stranded_claims,
    )
    from cccc.kernel.inbox import iter_events

    group = _group(tmp_path)
    barrier = Barrier(2)

    def claim():
        barrier.wait()
        return claim_delivery(
            group,
            actor_id="peer1",
            actor_created_at="2026-08-22T00:00:00Z",
            source_event_id="source-1",
            transport="pty",
        )

    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(lambda _: claim(), range(2)))

    assert sorted(results) == [(False, "claimed"), (True, "claimed")]
    states = [
        event["data"]["state"]
        for event in iter_events(group.ledger_path)
        if event.get("kind") == "runtime.delivery"
    ]
    assert states == ["claimed"]

    assert settle_stranded_claims(group) == 1
    assert latest_delivery_state(
        group, actor_id="peer1", source_event_id="source-1"
    )["data"]["state"] == "ambiguous"
    assert claim_delivery(
        group,
        actor_id="peer1",
        actor_created_at="2026-08-22T00:00:00Z",
        source_event_id="source-1",
        transport="pty",
    ) == (False, "ambiguous")
    assert claim_delivery(
        group,
        actor_id="peer1",
        actor_created_at="2026-08-22T00:00:00Z",
        source_event_id="source-1",
        transport="pty",
        force_ambiguous=True,
    ) == (True, "claimed")


def test_headless_recovery_does_not_overtake_a_failed_event(tmp_path, monkeypatch):
    from cccc.daemon.codex_app_sessions import SUPERVISOR
    from cccc.daemon.messaging.runtime_delivery import latest_delivery_state
    from cccc.daemon.messaging.runtime_pending_recovery import (
        refill_unread_runtime_messages,
    )
    from cccc.kernel.inbox import get_cursor
    from cccc.kernel.ledger import append_event

    group = _group(tmp_path)
    first = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key="",
        by="user",
        data={"to": ["peer1"], "text": "first", "message_mode": "send"},
    )
    second = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key="",
        by="user",
        data={"to": ["peer1"], "text": "second", "message_mode": "send"},
    )

    attempted: list[str] = []
    monkeypatch.setattr(SUPERVISOR, "actor_running", lambda *_args: True)
    monkeypatch.setattr(
        SUPERVISOR,
        "submit_user_message",
        lambda **kwargs: attempted.append(str(kwargs["event_id"])) is None and False,
    )

    assert refill_unread_runtime_messages(group, actor_id="peer1") == 0
    assert attempted == [first["id"]]
    assert latest_delivery_state(
        group, actor_id="peer1", source_event_id=first["id"]
    )["data"]["state"] == "failed"
    assert latest_delivery_state(
        group, actor_id="peer1", source_event_id=second["id"]
    ) is None

    attempted.clear()
    monkeypatch.setattr(
        SUPERVISOR,
        "submit_user_message",
        lambda **kwargs: attempted.append(str(kwargs["event_id"])) is None,
    )
    assert refill_unread_runtime_messages(group, actor_id="peer1") == 2
    assert attempted == [first["id"], second["id"]]
    assert get_cursor(group, "peer1") == ("", "")
