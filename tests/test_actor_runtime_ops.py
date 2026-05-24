import tempfile
import unittest
import json
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestActorRuntimeOps(unittest.TestCase):
    def test_resolve_launch_spec_uses_user_hermes_home_by_default(self) -> None:
        import os

        from cccc.daemon.actors.actor_runtime_ops import resolve_actor_launch_spec

        old_home = os.environ.get("CCCC_HOME")
        with tempfile.TemporaryDirectory() as td:
            os.environ["CCCC_HOME"] = td
            group = SimpleNamespace(
                group_id="g-test",
                doc={
                    "active_scope_key": "scope1",
                    "actors": [
                        {
                            "id": "hermes-1",
                            "default_scope_key": "scope1",
                            "runner": "pty",
                            "runtime": "hermes",
                            "command": ["hermes", "--tui", "--yolo"],
                            "env": {"A": "1"},
                        }
                    ],
                },
            )
            try:
                spec = resolve_actor_launch_spec(
                    group,
                    "hermes-1",
                    command=[],
                    env={},
                    runner="pty",
                    runtime="hermes",
                    find_scope_url=lambda _group, _scope_key: td,
                    effective_runner_kind=lambda runner: runner,
                    normalize_runtime_command=lambda _runtime, command: list(command),
                    supported_runtimes=("hermes",),
                )
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home

        self.assertEqual(spec["merged_env"]["A"], "1")
        self.assertNotIn("HERMES_HOME", spec["merged_env"])

    def test_resolve_launch_spec_preserves_explicit_hermes_profile(self) -> None:
        import os

        from cccc.daemon.actors.actor_runtime_ops import resolve_actor_launch_spec

        old_home = os.environ.get("CCCC_HOME")
        with tempfile.TemporaryDirectory() as td:
            os.environ["CCCC_HOME"] = td
            group = SimpleNamespace(
                group_id="g-test",
                doc={
                    "active_scope_key": "scope1",
                    "actors": [
                        {
                            "id": "hermes-1",
                            "default_scope_key": "scope1",
                            "runner": "pty",
                            "runtime": "hermes",
                            "command": ["hermes", "--profile", "other", "--tui", "--yolo"],
                            "env": {"A": "1"},
                        }
                    ],
                },
            )
            try:
                spec = resolve_actor_launch_spec(
                    group,
                    "hermes-1",
                    command=[],
                    env={},
                    runner="pty",
                    runtime="hermes",
                    find_scope_url=lambda _group, _scope_key: td,
                    effective_runner_kind=lambda runner: runner,
                    normalize_runtime_command=lambda _runtime, command: list(command),
                    supported_runtimes=("hermes",),
                )
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home

        self.assertEqual(spec["merged_env"]["A"], "1")
        self.assertNotIn("HERMES_HOME", spec["merged_env"])
        self.assertEqual(spec["effective_command"], ["hermes", "--profile", "other", "--tui", "--yolo"])

    def test_web_model_actor_start_schedules_chatgpt_browser_warmup(self) -> None:
        from cccc.daemon.actors import actor_runtime_ops

        with tempfile.TemporaryDirectory() as td:
            ledger_path = Path(td) / "ledger.jsonl"
            group = SimpleNamespace(
                group_id="g-test",
                doc={"active_scope_key": "scope1", "state": "active", "running": False},
                save=lambda: None,
                ledger_path=ledger_path,
            )
            actor = {
                "id": "chatgpt-web-1",
                "default_scope_key": "scope1",
                "runner": "headless",
                "runtime": "web_model",
                "command": [],
                "env": {"CCCC_WEB_MODEL_DELIVERY_MODE": "browser"},
            }

            with (
                patch.object(actor_runtime_ops, "find_actor", return_value=actor),
                patch.object(actor_runtime_ops, "runtime_start_preflight_error", return_value=""),
                patch.object(actor_runtime_ops, "request_pet_review"),
                patch(
                    "cccc.daemon.actors.web_model_browser_delivery.web_model_browser_delivery_enabled",
                    return_value=True,
                ) as delivery_enabled,
                patch(
                    "cccc.daemon.actors.web_model_browser_session.schedule_web_model_chatgpt_browser_session_warmup",
                    return_value=True,
                ) as warmup,
            ):
                result = actor_runtime_ops.start_actor_process(
                    group,
                    "chatgpt-web-1",
                    command=[],
                    env={},
                    runner="headless",
                    runtime="web_model",
                    by="user",
                    find_scope_url=lambda _group, _scope_key: ".",
                    effective_runner_kind=lambda runner: runner,
                    merge_actor_env_with_private=lambda _gid, _aid, env: dict(env),
                    normalize_runtime_command=lambda _runtime, command: list(command),
                    ensure_mcp_installed=lambda _runtime, _cwd, **_kwargs: True,
                    inject_actor_context_env=lambda env, _gid, _aid: dict(env),
                    prepare_pty_env=lambda env: dict(env),
                    pty_backlog_bytes=lambda: 1024,
                    write_headless_state=lambda _gid, _aid: None,
                    write_pty_state=lambda _gid, _aid, _pid: None,
                    clear_preamble_sent=lambda _group, _aid: None,
                    throttle_reset_actor=lambda _gid, _aid: None,
                    supported_runtimes=("web_model",),
                )

        self.assertTrue(bool(result.get("success")), result.get("error"))
        delivery_enabled.assert_called_once()
        warmup.assert_called_once_with(
            group_id="g-test",
            actor_id="chatgpt-web-1",
            reason="actor_start",
            retry_seconds=0.0,
        )

    def test_opencode_actor_start_injects_inline_mcp_config_into_pty_env(self) -> None:
        from cccc.daemon.actors import actor_runtime_ops

        captured: dict[str, object] = {}

        def fake_start_pty_actor_with_runtime_resume(**kwargs):
            captured.update(kwargs)
            return SimpleNamespace(pid=1234)

        def inject_context(env, gid, aid):
            out = dict(env)
            out["CCCC_HOME"] = "/tmp/cccc-home"
            out["CCCC_GROUP_ID"] = gid
            out["CCCC_ACTOR_ID"] = aid
            return out

        with tempfile.TemporaryDirectory() as td:
            ledger_path = Path(td) / "ledger.jsonl"
            group = SimpleNamespace(
                group_id="g-test",
                doc={"active_scope_key": "scope1", "state": "active", "running": False},
                save=lambda: None,
                ledger_path=ledger_path,
            )
            actor = {
                "id": "open-1",
                "default_scope_key": "scope1",
                "runner": "pty",
                "runtime": "opencode",
                "command": ["opencode"],
                "env": {"OPENCODE_CONFIG_CONTENT": json.dumps({"mcp": {"other": {"type": "local", "command": ["other"]}}})},
            }

            with (
                patch.object(actor_runtime_ops, "find_actor", return_value=actor),
                patch.object(actor_runtime_ops.pty_runner, "PTY_SUPPORTED", True),
                patch.object(actor_runtime_ops, "runtime_start_preflight_error", return_value=""),
                patch.object(actor_runtime_ops, "start_pty_actor_with_runtime_resume", side_effect=fake_start_pty_actor_with_runtime_resume),
                patch.object(actor_runtime_ops, "request_pet_review"),
                patch("cccc.daemon.mcp_install.get_cccc_mcp_stdio_command", return_value=["/abs/cccc", "mcp"]),
            ):
                result = actor_runtime_ops.start_actor_process(
                    group,
                    "open-1",
                    command=[],
                    env={},
                    runner="pty",
                    runtime="opencode",
                    by="user",
                    find_scope_url=lambda _group, _scope_key: td,
                    effective_runner_kind=lambda runner: runner,
                    merge_actor_env_with_private=lambda _gid, _aid, env: dict(env),
                    normalize_runtime_command=lambda _runtime, command: list(command),
                    ensure_mcp_installed=lambda _runtime, _cwd, **kwargs: "OPENCODE_CONFIG_CONTENT" in kwargs.get("env", {}),
                    inject_actor_context_env=inject_context,
                    prepare_pty_env=lambda env: dict(env),
                    pty_backlog_bytes=lambda: 1024,
                    write_headless_state=lambda _gid, _aid: None,
                    write_pty_state=lambda _gid, _aid, _pid: None,
                    clear_preamble_sent=lambda _group, _aid: None,
                    throttle_reset_actor=lambda _gid, _aid: None,
                    supported_runtimes=("opencode",),
                )

        self.assertTrue(bool(result.get("success")), result.get("error"))
        env = captured.get("env") if isinstance(captured.get("env"), dict) else {}
        doc = json.loads(str(env.get("OPENCODE_CONFIG_CONTENT") or "{}"))
        self.assertEqual(doc["mcp"]["other"]["command"], ["other"])
        self.assertEqual(doc["mcp"]["cccc"]["command"], ["/abs/cccc", "mcp"])
        self.assertEqual(doc["mcp"]["cccc"]["environment"]["CCCC_GROUP_ID"], "g-test")
        self.assertEqual(doc["mcp"]["cccc"]["environment"]["CCCC_ACTOR_ID"], "open-1")


if __name__ == "__main__":
    unittest.main()
