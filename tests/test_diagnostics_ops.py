import os
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch


class TestDiagnosticsOps(unittest.TestCase):
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

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group(self, title: str = "diagnostics-test") -> str:
        resp, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        gid = str((resp.result or {}).get("group_id") or "")
        self.assertTrue(gid)
        return gid

    def test_debug_ops_require_developer_mode(self) -> None:
        _, cleanup = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": False}})
            self.assertTrue(update.ok, getattr(update, "error", None))
            resp, _ = self._call("debug_snapshot", {"by": "user"})
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp, "error", None).code), "developer_mode_required")
        finally:
            cleanup()

    def test_debug_tail_logs_invalid_component(self) -> None:
        _, cleanup = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": True}})
            self.assertTrue(update.ok, getattr(update, "error", None))

            tail, _ = self._call("debug_tail_logs", {"by": "user", "component": "unknown"})
            self.assertFalse(tail.ok)
            self.assertEqual(str(getattr(tail, "error", None).code), "invalid_component")
        finally:
            cleanup()

    def test_debug_clear_logs_im_requires_group(self) -> None:
        _, cleanup = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": True}})
            self.assertTrue(update.ok, getattr(update, "error", None))

            clear, _ = self._call("debug_clear_logs", {"by": "user", "component": "im"})
            self.assertFalse(clear.ok)
            self.assertEqual(str(getattr(clear, "error", None).code), "missing_group_id")
        finally:
            cleanup()

    def test_try_handle_unknown_diagnostics_op_returns_none(self) -> None:
        from cccc.daemon.ops.diagnostics_ops import try_handle_diagnostics_op

        resp = try_handle_diagnostics_op(
            "not_diagnostics",
            {},
            developer_mode_enabled=lambda: True,
            get_observability=lambda: {},
            effective_runner_kind=lambda runner: runner,
            throttle_debug_summary=lambda _group_id: {},
            can_read_terminal_transcript=lambda _group, _by, _target: False,
            pty_backlog_bytes=lambda: 1024,
        )
        self.assertIsNone(resp)

    def test_debug_snapshot_includes_web_binding_runtime_evidence(self) -> None:
        from cccc.ports.web.runtime_control import write_web_runtime_state

        td, cleanup = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": True}})
            self.assertTrue(update.ok, getattr(update, "error", None))

            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "web_host": "0.0.0.0",
                    "web_port": 9001,
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))

            write_web_runtime_state(
                home=Path(td),
                pid=os.getpid(),
                host="127.0.0.1",
                port=8848,
                mode="normal",
                supervisor_managed=True,
                supervisor_pid=os.getpid(),
                launcher_pid=os.getpid(),
                launch_source="test",
            )

            resp, _ = self._call("debug_snapshot", {"by": "user"})
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            web = (resp.result or {}).get("web") if isinstance(resp.result, dict) else {}
            configured = web.get("configured") if isinstance(web.get("configured"), dict) else {}
            runtime = web.get("runtime") if isinstance(web.get("runtime"), dict) else {}

            self.assertEqual(str(configured.get("host") or ""), "0.0.0.0")
            self.assertEqual(int(configured.get("port") or 0), 9001)
            self.assertEqual(str(configured.get("exposure_class") or ""), "private")
            self.assertEqual(str(runtime.get("host") or ""), "127.0.0.1")
            self.assertEqual(int(runtime.get("port") or 0), 8848)
            self.assertEqual(bool(runtime.get("pid_alive")), True)
            self.assertEqual(bool(web.get("runtime_matches_configured_binding")), False)
            self.assertIn("binding_apply_pending", web.get("issues") or [])
            self.assertTrue(str(web.get("log_path") or "").endswith("daemon/cccc-web.log"))
        finally:
            cleanup()

    def test_debug_snapshot_marks_stale_web_runtime_pid(self) -> None:
        from cccc.ports.web.runtime_control import write_web_runtime_state

        td, cleanup = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": True}})
            self.assertTrue(update.ok, getattr(update, "error", None))

            write_web_runtime_state(
                home=Path(td),
                pid=2_147_483_647,
                host="127.0.0.1",
                port=8848,
                mode="normal",
                supervisor_managed=True,
                supervisor_pid=os.getpid(),
                launch_source="test",
            )

            resp, _ = self._call("debug_snapshot", {"by": "user"})
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            web = (resp.result or {}).get("web") if isinstance(resp.result, dict) else {}
            runtime = web.get("runtime") if isinstance(web.get("runtime"), dict) else {}

            self.assertEqual(int(runtime.get("pid") or 0), 2_147_483_647)
            self.assertEqual(bool(runtime.get("pid_alive")), False)
            self.assertIn("runtime_pid_stale", web.get("issues") or [])
        finally:
            cleanup()

    def test_feedback_bundle_export_redacts_logs_and_transcripts(self) -> None:
        td, cleanup = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": True}})
            self.assertTrue(update.ok, getattr(update, "error", None))

            gid = self._create_group("feedback-bundle")
            add_actor, _ = self._call(
                "actor_add",
                {
                    "group_id": gid,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "pty",
                    "by": "user",
                },
            )
            self.assertTrue(add_actor.ok, getattr(add_actor, "error", None))

            daemon_log = Path(td) / "daemon" / "ccccd.log"
            daemon_log.parent.mkdir(parents=True, exist_ok=True)
            daemon_log.write_text(
                "Authorization: Bearer sk-live-secret-token\nOPENAI_API_KEY=sk-abcdefghi123456\n",
                encoding="utf-8",
            )

            with patch("cccc.runners.pty.SUPERVISOR.actor_running", return_value=True), patch(
                "cccc.runners.pty.SUPERVISOR.tail_output",
                return_value=b'export OPENAI_API_KEY="sk-terminal-secret"\nCookie: sessionid=abc123\n',
            ):
                resp, _ = self._call("feedback_bundle_export", {"group_id": gid, "by": "user"})

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            attachment = result.get("attachment") if isinstance(result.get("attachment"), dict) else {}
            rel_path = str(attachment.get("path") or "")
            self.assertTrue(rel_path.startswith("state/blobs/"))

            from cccc.kernel.group import load_group

            group = load_group(gid)
            self.assertIsNotNone(group)
            bundle_path = Path(str(group.path)) / rel_path
            self.assertTrue(bundle_path.exists())

            with zipfile.ZipFile(bundle_path) as zf:
                daemon_text = zf.read("logs/daemon.log").decode("utf-8")
                terminal_text = zf.read("terminals/peer1.txt").decode("utf-8")
                manifest = zf.read("manifest.json").decode("utf-8")

            self.assertIn("[REDACTED]", daemon_text)
            self.assertNotIn("sk-live-secret-token", daemon_text)
            self.assertNotIn("sk-abcdefghi123456", daemon_text)
            self.assertIn("[REDACTED]", terminal_text)
            self.assertNotIn("sk-terminal-secret", terminal_text)
            self.assertNotIn("sessionid=abc123", terminal_text)
            self.assertIn('"kind": "feedback_bundle"', manifest)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
