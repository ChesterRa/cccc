from cccc.daemon import server
from cccc.daemon.actors import runner_ops
from cccc.kernel.group import Group


def _deepseek_group(path) -> Group:
    path.mkdir(parents=True, exist_ok=True)
    return Group(
        group_id="deepseek-group",
        path=path,
        doc={
            "group_id": "deepseek-group",
            "actors": [
                {
                    "id": "deepseek",
                    "runtime": "deepseek",
                    "runner": "headless",
                    "enabled": True,
                }
            ],
        },
    )


def test_auto_wake_running_deepseek_uses_dedicated_registry(tmp_path, monkeypatch) -> None:
    group = _deepseek_group(tmp_path / "group")
    monkeypatch.setattr(server.deepseek_runtime, "running", lambda **_kwargs: True)
    generic_calls: list[tuple[str, str]] = []
    monkeypatch.setattr(
        server.headless_runner.SUPERVISOR,
        "actor_running",
        lambda group_id, actor_id: generic_calls.append((group_id, actor_id)) or False,
    )
    assert server._auto_wake_actor_running(group, "deepseek") is True
    assert generic_calls == []


def test_automation_group_running_includes_deepseek_registry(monkeypatch) -> None:
    monkeypatch.setattr(runner_ops.codex_app_supervisor, "group_running", lambda _group_id: False)
    monkeypatch.setattr(runner_ops.claude_app_supervisor, "group_running", lambda _group_id: False)
    monkeypatch.setattr(runner_ops.pty_runner.SUPERVISOR, "group_running", lambda _group_id: False)
    monkeypatch.setattr(runner_ops.headless_runner.SUPERVISOR, "group_running", lambda _group_id: False)
    monkeypatch.setattr(runner_ops, "web_model_group_running", lambda _group_id: False)
    monkeypatch.setattr(runner_ops.deepseek_runtime, "group_running", lambda group_id: group_id == "g-deepseek")
    assert server.runner_group_running("g-deepseek") is True
