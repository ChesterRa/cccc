from __future__ import annotations

from fastapi.testclient import TestClient


def test_standard_web_allows_deepseek_headless_create_update_and_start(
    tmp_path, monkeypatch
) -> None:
    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    from cccc.kernel.group import create_group
    from cccc.kernel.registry import load_registry
    from cccc.ports.web.app import create_app

    group_id = create_group(load_registry(), title="deepseek-web", topic="").group_id
    actor = {
        "id": "deepseek",
        "runtime": "deepseek",
        "runner": "headless",
        "enabled": True,
    }

    def call_daemon(request: dict) -> dict:
        operation = str(request.get("op") or "")
        if operation == "observability_get":
            return {"ok": True, "result": {"observability": {"developer_mode": False}}}
        if operation == "actor_list":
            return {"ok": True, "result": {"actors": [actor]}}
        return {"ok": True, "result": {"actor": actor}}

    monkeypatch.setattr("cccc.ports.web.app.call_daemon", call_daemon)
    client = TestClient(create_app())

    created = client.post(
        f"/api/v1/groups/{group_id}/actors",
        json={
            "actor_id": "deepseek",
            "runtime": "deepseek",
            "runner": "headless",
        },
    )
    updated = client.post(
        f"/api/v1/groups/{group_id}/actors/deepseek",
        json={"runtime": "deepseek", "runner": "headless"},
    )
    started = client.post(f"/api/v1/groups/{group_id}/actors/deepseek/start")

    assert created.status_code == 200
    assert updated.status_code == 200
    assert started.status_code == 200
    assert created.json()["ok"] is True
    assert updated.json()["ok"] is True
    assert started.json()["ok"] is True
