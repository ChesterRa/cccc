import pytest

from cccc.kernel.runtime_state_source import default_runtime_state_source


@pytest.mark.parametrize(
    ("command", "expected"),
    [
        (None, "app_server"),
        (["codex", "--profile", "local", "--search"], "terminal"),
        (["codex", "-c", "model_provider=local", "--search"], "terminal"),
        (["codex", "-c", "shell_environment_policy.inherit=all", "--search"], "app_server"),
    ],
)
def test_codex_pty_command_aware_state_source_defaults(command, expected) -> None:
    assert default_runtime_state_source(runtime="codex", runner="pty", command=command) == expected


def test_non_codex_pty_defaults_to_terminal_state_source() -> None:
    assert default_runtime_state_source(runtime="claude", runner="pty") == "terminal"


def test_explicit_runtime_state_source_is_preserved_when_valid() -> None:
    assert default_runtime_state_source(runtime="codex", runner="pty", requested_source="terminal") == "terminal"


def _runtime_source_group(tmp_path, monkeypatch):
    from cccc.kernel.group import create_group, load_group
    from cccc.kernel.registry import load_registry

    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    group_id = create_group(load_registry(), title="runtime-source", topic="").group_id
    group = load_group(group_id)
    assert group is not None
    return group


def test_add_actor_applies_codex_pty_app_server_default(tmp_path, monkeypatch) -> None:
    from cccc.kernel.actors import add_actor

    group = _runtime_source_group(tmp_path, monkeypatch)
    actor = add_actor(group, actor_id="peer1", runtime="codex", runner="pty")

    assert actor["runtime_state_source"] == "app_server"


def test_add_actor_applies_codex_profile_terminal_default(tmp_path, monkeypatch) -> None:
    from cccc.kernel.actors import add_actor

    group = _runtime_source_group(tmp_path, monkeypatch)
    actor = add_actor(
        group,
        actor_id="peer1",
        runtime="codex",
        runner="pty",
        command=["codex", "--profile", "local", "--search"],
    )

    assert actor["runtime_state_source"] == "terminal"


def test_update_actor_switches_codex_provider_command_to_terminal_default(tmp_path, monkeypatch) -> None:
    from cccc.kernel.actors import add_actor, update_actor

    group = _runtime_source_group(tmp_path, monkeypatch)
    actor = add_actor(group, actor_id="peer1", runtime="codex", runner="pty")
    assert actor["runtime_state_source"] == "app_server"

    actor = update_actor(
        group,
        "peer1",
        {"command": ["codex", "--profile", "local", "--search"]},
    )

    assert actor["runtime_state_source"] == "terminal"
