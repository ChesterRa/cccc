from __future__ import annotations

from cccc.kernel.actors import add_actor, update_actor
from cccc.kernel.runtime import (
    DEEPSEEK_ACP_VERSION,
    DEEPSEEK_DSH_VERSION,
    DEEPSEEK_MCP_CLIENT_VERSION,
    KNOWN_RUNTIMES,
    PRIMARY_RUNTIMES,
    deepseek_preflight_error,
    get_runtime_command_with_flags,
)
from cccc.contracts.v1.deepseek import (
    DEEPSEEK_ACP_APP_VERSION,
    DEEPSEEK_ACP_SDK_VERSION,
    DEEPSEEK_LLM_ADAPTER_VERSION,
    DEEPSEEK_NODE_RANGE,
    DEEPSEEK_PROTOCOL_VERSION,
)
from cccc.kernel.deepseek_acp import (
    ACPProtocolError,
    NDJSONSession,
    initialize_request,
    permission_outcome,
    session_new_request,
    terminal_stop_reason,
    validate_initialize_result,
    validate_session_new_result,
    validate_session_update,
)


def test_deepseek_is_a_primary_headless_runtime_contract() -> None:
    assert "deepseek" in PRIMARY_RUNTIMES
    assert KNOWN_RUNTIMES["deepseek"]["command"] == "dsh-acp-demo"
    assert get_runtime_command_with_flags("deepseek") == ["dsh-acp-demo"]


def test_deepseek_release_contract_matches_rust_source() -> None:
    """Keep the Python generated contract and Rust contracts byte-aligned."""
    import re
    from pathlib import Path

    source = (Path(__file__).parents[1] / "crates/cccc-contracts/src/deepseek.rs").read_text(
        encoding="utf-8"
    )
    expected = {
        "DEEPSEEK_DSH_VERSION": DEEPSEEK_DSH_VERSION,
        "DEEPSEEK_ACP_VERSION": DEEPSEEK_ACP_VERSION,
        "DEEPSEEK_MCP_CLIENT_VERSION": DEEPSEEK_MCP_CLIENT_VERSION,
        "DEEPSEEK_ACP_APP_VERSION": DEEPSEEK_ACP_APP_VERSION,
        "DEEPSEEK_LLM_ADAPTER_VERSION": DEEPSEEK_LLM_ADAPTER_VERSION,
        "DEEPSEEK_NODE_RANGE": DEEPSEEK_NODE_RANGE,
        "DEEPSEEK_PROTOCOL_VERSION": str(DEEPSEEK_PROTOCOL_VERSION),
        "DEEPSEEK_ACP_SDK_VERSION": DEEPSEEK_ACP_SDK_VERSION,
    }
    for name, value in expected.items():
        match = re.search(rf"pub const {name}: [^=]+ = ([^;]+);", source)
        assert match is not None
        assert match.group(1).strip().strip('"') == value


def test_deepseek_actor_is_forced_to_headless(tmp_path) -> None:
    from cccc.kernel.group import Group

    path = tmp_path / "group"
    path.mkdir()
    group = Group(
        group_id="g_deepseek",
        path=path,
        doc={"group_id": "g_deepseek", "actors": [], "automation": {}},
    )
    actor = add_actor(group, actor_id="deepseek", runtime="deepseek", runner="pty")

    assert actor["runtime"] == "deepseek"
    assert actor["runner"] == "headless"


def test_actor_update_accepts_deepseek_runtime(tmp_path) -> None:
    from cccc.kernel.group import Group

    path = tmp_path / "group-update"
    path.mkdir()
    group = Group(
        group_id="g_deepseek_update",
        path=path,
        doc={"group_id": "g_deepseek_update", "actors": [], "automation": {}},
    )
    add_actor(group, actor_id="agent", runtime="codex", runner="headless")
    actor = update_actor(group, "agent", {"runtime": "deepseek"})
    assert actor["runtime"] == "deepseek"
    assert actor["runner"] == "headless"
    assert actor["command"] == ["dsh-acp-demo"]


def test_shared_acp_vectors_are_consumed_by_python_parser() -> None:
    import json
    from pathlib import Path

    fixture = json.loads(
        (Path(__file__).parent / "fixtures" / "deepseek_acp_vectors.json").read_text(encoding="utf-8")
    )
    parser = NDJSONSession()
    parser.register(1)
    for frame in fixture["frames"]:
        try:
            parser.feed_line(frame["line"])
            valid = True
        except ACPProtocolError:
            valid = False
        assert valid is frame["valid"], frame["name"]
    cancelled = fixture["cancelled_terminal"]["frame"]
    assert terminal_stop_reason(cancelled) == "cancelled"
    assert terminal_stop_reason(cancelled) != "end_turn"
    assert fixture["update_idempotency"]["dedupe_key"] == "deepseek.update:event-1:{ordinal}"
    assert fixture["update_idempotency"]["expected_durable_updates"] == 2
    assert fixture["protocol_version"] == DEEPSEEK_PROTOCOL_VERSION
    assert fixture["acp_sdk_version"] == DEEPSEEK_ACP_SDK_VERSION


def test_profile_manifest_rejects_an_empty_managed_shell() -> None:
    import json
    from pathlib import Path

    from cccc.contracts.v1.deepseek import is_canonical_profile_manifest

    vectors = json.loads(
        (Path(__file__).parent / "fixtures" / "deepseek_manifest_vectors.json").read_text(
            encoding="utf-8"
        )
    )
    assert is_canonical_profile_manifest(vectors["valid"])
    for name in vectors:
        if name != "valid":
            assert not is_canonical_profile_manifest(vectors[name]), name


def test_acp_handshake_permission_and_bounds() -> None:
    assert initialize_request()["params"]["clientCapabilities"] == {}
    assert session_new_request("/tmp/work")["params"]["mcpServers"] == []
    assert session_new_request(r"C:\work")["params"]["cwd"] == r"C:\work"
    assert session_new_request(r"\\server\share\work")["params"]["cwd"] == r"\\server\share\work"
    assert permission_outcome([{"optionId": "reject-once"}])["outcome"]["outcome"] == "selected"
    assert permission_outcome([])["outcome"]["outcome"] == "cancelled"
    validate_initialize_result({"result": {"protocolVersion": 1, "agentInfo": {"name": "dsh"}}})
    seen = set()
    assert validate_session_new_result({"result": {"sessionId": "session-1"}}, seen=seen) == "session-1"
    try:
        validate_session_new_result({"result": {"sessionId": "session-1"}}, seen=seen)
    except ACPProtocolError:
        pass
    else:
        raise AssertionError("duplicate session id must fail")
    update = {
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {"sessionId": "session-1", "update": {"sessionUpdate": "agent_message_chunk"}},
    }
    assert validate_session_update(update, "session-1")["sessionId"] == "session-1"
    try:
        validate_session_update(update, "stale-session")
    except ACPProtocolError:
        pass
    else:
        raise AssertionError("stale session update must fail")
