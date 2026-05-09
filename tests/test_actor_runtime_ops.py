import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestActorRuntimeOps(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
