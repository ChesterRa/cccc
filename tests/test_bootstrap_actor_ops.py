import os
import tempfile
import unittest
from pathlib import Path

from cccc.daemon.bootstrap_actor_ops import autostart_running_groups


class TestBootstrapActorOps(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return Path(td), cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_no_groups_is_noop(self) -> None:
        home, cleanup = self._with_home()
        try:
            autostart_running_groups(
                home,
                effective_runner_kind=lambda runner: runner,
                find_scope_url=lambda _group, _scope_key: "",
                supported_runtimes=("codex",),
                ensure_mcp_installed=lambda _runtime, _cwd: True,
                auto_mcp_runtimes=("codex",),
                pty_supported=lambda: True,
                merge_actor_env_with_private=lambda _gid, _aid, env: dict(env),
                inject_actor_context_env=lambda env, _gid, _aid: dict(env),
                prepare_pty_env=lambda env: dict(env),
                normalize_runtime_command=lambda _runtime, command: list(command),
                pty_backlog_bytes=lambda: 1024,
                write_headless_state=lambda _gid, _aid: None,
                write_pty_state=lambda _gid, _aid, _pid: None,
                clear_preamble_sent=lambda _group, _aid: None,
                throttle_reset_actor=lambda _gid, _aid: None,
                automation_on_resume=lambda _group: None,
                get_group_state=lambda _group: "idle",
            )
        finally:
            cleanup()

    def test_running_group_without_active_scope_is_cleared(self) -> None:
        home, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "autostart", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = True
            group.doc["active_scope_key"] = ""
            group.save()

            autostart_running_groups(
                home,
                effective_runner_kind=lambda runner: runner,
                find_scope_url=lambda _group, _scope_key: "",
                supported_runtimes=("codex",),
                ensure_mcp_installed=lambda _runtime, _cwd: True,
                auto_mcp_runtimes=("codex",),
                pty_supported=lambda: True,
                merge_actor_env_with_private=lambda _gid, _aid, env: dict(env),
                inject_actor_context_env=lambda env, _gid, _aid: dict(env),
                prepare_pty_env=lambda env: dict(env),
                normalize_runtime_command=lambda _runtime, command: list(command),
                pty_backlog_bytes=lambda: 1024,
                write_headless_state=lambda _gid, _aid: None,
                write_pty_state=lambda _gid, _aid, _pid: None,
                clear_preamble_sent=lambda _group, _aid: None,
                throttle_reset_actor=lambda _gid, _aid: None,
                automation_on_resume=lambda _group: None,
                get_group_state=lambda _group: "idle",
            )

            reloaded = load_group(group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            self.assertFalse(bool(reloaded.doc.get("running")))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
