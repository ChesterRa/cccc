from cccc.kernel.runtime_state_source import default_runtime_state_source


def test_codex_pty_defaults_to_app_server_state_source() -> None:
    assert default_runtime_state_source(runtime="codex", runner="pty") == "app_server"


def test_non_codex_pty_defaults_to_terminal_state_source() -> None:
    assert default_runtime_state_source(runtime="claude", runner="pty") == "terminal"


def test_explicit_runtime_state_source_is_preserved_when_valid() -> None:
    assert default_runtime_state_source(runtime="codex", runner="pty", requested_source="terminal") == "terminal"


def test_add_actor_applies_codex_pty_app_server_default(tmp_path, monkeypatch) -> None:
    from cccc.kernel.actors import add_actor
    from cccc.kernel.group import create_group, load_group
    from cccc.kernel.registry import load_registry

    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    group_id = create_group(load_registry(), title="runtime-source", topic="").group_id
    group = load_group(group_id)
    assert group is not None

    actor = add_actor(group, actor_id="peer1", runtime="codex", runner="pty")

    assert actor["runtime_state_source"] == "app_server"
