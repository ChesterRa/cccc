from __future__ import annotations

from types import SimpleNamespace
from unittest.mock import patch


def _actor() -> dict[str, object]:
    return {
        "id": "deepseek-1",
        "runtime": "deepseek",
        "runner": "headless",
        "enabled": True,
        "command": ["dsh-acp-demo"],
        "env": {},
    }


def _group(tmp_path):
    return SimpleNamespace(
        group_id="g-deepseek",
        path=tmp_path,
        doc={
            "group_id": "g-deepseek",
            "state": "active",
            "active_scope_key": "main",
            "title": "DeepSeek",
            "actors": [_actor()],
        },
    )


class _Storage:
    def __init__(self, group) -> None:
        self.group = group

    def load_agents(self):
        return SimpleNamespace(agents=[])


def test_stale_headless_marker_never_overrides_deepseek_supervisor_truth(tmp_path) -> None:
    from cccc.daemon.actors import actor_ops
    from cccc.daemon.context import context_ops
    from cccc.daemon.ops import diagnostics_ops
    from cccc.ports.web.routes import actors as web_actors
    from cccc.ports.web.routes import groups as web_groups

    group = _group(tmp_path)
    actor = _actor()
    with (
        patch.object(actor_ops, "load_group", return_value=group),
        patch.object(actor_ops, "get_actor_list_projection", return_value=[dict(actor)]),
        patch.object(actor_ops, "get_group_runtime", return_value={}),
        patch.object(actor_ops, "ContextStorage", _Storage),
        patch.object(actor_ops.deepseek_runtime, "running", return_value=False),
        patch.object(actor_ops, "derive_effective_working_state", return_value={}),
        patch.object(actor_ops, "runtime_hook_working_projection", return_value=None),
        patch.object(actor_ops, "current_session_capability", return_value=None),
    ):
        listed = actor_ops.handle_actor_list(
            {"group_id": group.group_id, "include_unread": False},
            effective_runner_kind=lambda runner: runner,
        )
    assert listed.ok
    assert listed.result["actors"][0]["running"] is False

    with (
        patch.object(context_ops.deepseek_runtime, "running", return_value=False),
        patch.object(context_ops, "derive_effective_working_state", return_value={}),
        patch.object(context_ops, "runtime_hook_working_projection", return_value=None),
        patch.object(context_ops, "current_session_capability", return_value=None),
        patch.object(context_ops, "ensure_home", return_value=tmp_path),
    ):
        context = context_ops._actor_runtime_state_to_dict(
            group_id=group.group_id,
            actor_doc=dict(actor),
            agent_state_by_id={},
            runtime_snapshot_by_id={},
        )
    assert context["running"] is False

    with patch.object(web_groups.deepseek_runtime, "running", return_value=False):
        assert web_groups._actor_running_local(group.group_id, actor) is False

    with (
        patch.object(web_actors, "load_group", return_value=group),
        patch.object(web_actors, "get_actor_list_projection", return_value=[dict(actor)]),
        patch.object(web_actors, "ContextStorage", _Storage),
        patch.object(web_actors.deepseek_runtime, "running", return_value=False),
        patch.object(web_actors, "derive_effective_working_state", return_value={}),
        patch.object(web_actors, "runtime_hook_working_projection", return_value=None),
        patch.object(web_actors, "current_session_capability", return_value=None),
    ):
        web_list = web_actors._read_actor_list_local(group.group_id, include_unread=False)
    assert web_list["ok"] is True
    assert web_list["result"]["actors"][0]["running"] is False

    with (
        patch.object(diagnostics_ops, "load_group", return_value=group),
        patch.object(diagnostics_ops, "list_actors", return_value=[dict(actor)]),
        patch.object(diagnostics_ops, "ensure_home", return_value=tmp_path),
        patch.object(diagnostics_ops, "_build_web_debug_snapshot", return_value={}),
        patch.object(diagnostics_ops.deepseek_runtime, "running", return_value=False),
    ):
        debug = diagnostics_ops.handle_debug_snapshot(
            {"group_id": group.group_id, "by": "user"},
            developer_mode_enabled=lambda: True,
            get_observability=lambda: {},
            effective_runner_kind=lambda runner: runner,
            throttle_debug_summary=lambda _gid: {},
        )
    assert debug.ok
    assert debug.result["actors"][0]["running"] is False


def test_deepseek_projection_turns_running_only_for_live_supervisor(tmp_path) -> None:
    from cccc.daemon.context import context_ops
    from cccc.ports.web.routes import groups as web_groups

    actor = _actor()
    with patch.object(web_groups.deepseek_runtime, "running", return_value=True):
        assert web_groups._actor_running_local("g-deepseek", actor) is True
    with (
        patch.object(context_ops.deepseek_runtime, "running", return_value=True),
        patch.object(context_ops, "derive_effective_working_state", return_value={}),
        patch.object(context_ops, "runtime_hook_working_projection", return_value=None),
        patch.object(context_ops, "current_session_capability", return_value=None),
        patch.object(context_ops, "ensure_home", return_value=tmp_path),
    ):
        context = context_ops._actor_runtime_state_to_dict(
            group_id="g-deepseek",
            actor_doc=dict(actor),
            agent_state_by_id={},
            runtime_snapshot_by_id={},
        )
    assert context["running"] is True
