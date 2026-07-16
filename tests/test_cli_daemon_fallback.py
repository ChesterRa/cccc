from __future__ import annotations

import os
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path
from unittest.mock import patch


class TestCliDaemonFallback(unittest.TestCase):
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

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title="cli-daemon-fallback", topic="")
        return group.group_id

    def test_attach_sends_absolute_path_to_daemon(self) -> None:
        from cccc import cli

        old_cwd = os.getcwd()
        with tempfile.TemporaryDirectory(prefix="cccc_cli_attach_") as project_dir:
            try:
                os.chdir(project_dir)
                resp = {"ok": True, "result": {"group_id": "g_cli"}}
                args = Namespace(path=".", group_id="")

                with patch.object(cli, "_ensure_daemon_running", return_value=True), \
                     patch.object(cli, "call_daemon", return_value=resp) as call_daemon, \
                     patch.object(cli, "set_active_group_id"), \
                     patch.object(cli, "_print_json"):
                    code = cli.cmd_attach(args)

                self.assertEqual(code, 0)
                req = call_daemon.call_args.args[0]
                self.assertEqual(req.get("op"), "attach")
                self.assertEqual(str((req.get("args") or {}).get("path") or ""), str(Path(project_dir).resolve()))
            finally:
                os.chdir(old_cwd)

    def test_group_use_sends_absolute_path_to_daemon(self) -> None:
        from cccc import cli

        old_cwd = os.getcwd()
        with tempfile.TemporaryDirectory(prefix="cccc_cli_group_use_") as project_dir:
            try:
                os.chdir(project_dir)
                resp = {"ok": True, "result": {"group_id": "g_cli", "active_scope_key": "s_cli"}}
                args = Namespace(group_id="g_cli", path=".")

                with patch.object(cli, "_ensure_daemon_running", return_value=True), \
                     patch.object(cli, "call_daemon", return_value=resp) as call_daemon, \
                     patch.object(cli, "_print_json"):
                    code = cli.cmd_group_use(args)

                self.assertEqual(code, 0)
                req = call_daemon.call_args.args[0]
                self.assertEqual(req.get("op"), "group_use")
                self.assertEqual(str((req.get("args") or {}).get("path") or ""), str(Path(project_dir).resolve()))
            finally:
                os.chdir(old_cwd)

    def test_send_does_not_fallback_after_daemon_rejection(self) -> None:
        from cccc import cli

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            resp = {"ok": False, "error": {"code": "no_enabled_recipients", "message": "blocked by daemon"}}
            args = Namespace(
                group=group_id,
                text="hello",
                by="user",
                path="",
                to=[],
                priority="normal",
                reply_required=False,
            )

            with patch.object(cli, "_ensure_daemon_running", return_value=True), \
                 patch.object(cli, "call_daemon", return_value=resp), \
                 patch.object(cli, "append_event", side_effect=AssertionError("local append should not run")) as append_event, \
                 patch.object(cli, "_print_json") as print_json:
                code = cli.cmd_send(args)

            self.assertEqual(code, 2)
            append_event.assert_not_called()
            print_json.assert_called_once_with(resp)
        finally:
            cleanup()

    def test_send_keeps_local_fallback_for_daemon_unavailable(self) -> None:
        from cccc import cli

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            resp = {"ok": False, "error": {"code": "daemon_unavailable", "message": "daemon unavailable"}}
            args = Namespace(
                group=group_id,
                text="offline fallback",
                by="user",
                path="",
                to=[],
                priority="normal",
                reply_required=False,
            )

            with patch.object(cli, "_ensure_daemon_running", return_value=True), \
                 patch.object(cli, "call_daemon", return_value=resp), \
                 patch.object(cli, "_print_json") as print_json:
                code = cli.cmd_send(args)

            self.assertEqual(code, 0)
            printed = print_json.call_args.args[0]
            self.assertTrue(bool(printed.get("ok")))
            self.assertEqual(str(((printed.get("result") or {}).get("event") or {}).get("kind") or ""), "chat.message")
        finally:
            cleanup()

    def test_send_defaults_to_actor_env_inside_runtime(self) -> None:
        from cccc import cli

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            args = Namespace(
                group=group_id,
                text="actor fallback",
                by=None,
                path="",
                to=[],
                priority="normal",
                reply_required=False,
            )

            with patch.dict(os.environ, {"CCCC_ACTOR_ID": "小抠"}, clear=False), \
                 patch.object(cli, "_ensure_daemon_running", return_value=True), \
                 patch.object(cli, "call_daemon", return_value={"ok": True, "result": {}}) as call_daemon, \
                 patch.object(cli, "_print_json"):
                code = cli.cmd_send(args)

            self.assertEqual(code, 0)
            req = call_daemon.call_args.args[0]
            self.assertEqual(req.get("op"), "send")
            self.assertEqual((req.get("args") or {}).get("by"), "小抠")
        finally:
            cleanup()

    def test_reply_defaults_to_actor_env_inside_runtime(self) -> None:
        from cccc import cli
        from cccc.contracts.v1 import ChatMessageData
        from cccc.kernel.group import load_group
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            original = append_event(
                group.ledger_path,  # type: ignore[union-attr]
                kind="chat.message",
                group_id=group_id,
                scope_key="",
                by="管理员",
                data=ChatMessageData(text="please report", to=["小抠"], reply_required=True).model_dump(),
            )
            args = Namespace(
                group=group_id,
                event_id=original["id"],
                text="done",
                by=None,
                to=[],
                priority="normal",
                reply_required=False,
            )

            with patch.dict(os.environ, {"CCCC_ACTOR_ID": "小抠"}, clear=False), \
                 patch.object(cli, "_ensure_daemon_running", return_value=True), \
                 patch.object(cli, "call_daemon", return_value={"ok": True, "result": {}}) as call_daemon, \
                 patch.object(cli, "_print_json"):
                code = cli.cmd_reply(args)

            self.assertEqual(code, 0)
            req = call_daemon.call_args.args[0]
            self.assertEqual(req.get("op"), "reply")
            self.assertEqual((req.get("args") or {}).get("by"), "小抠")
        finally:
            cleanup()

    def test_tracked_send_defaults_to_actor_env_inside_runtime(self) -> None:
        from cccc import cli

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            args = Namespace(
                group=group_id,
                text="please implement",
                title="Implement task",
                by=None,
                to=["peer1"],
                outcome="done",
                checklist="",
                assignee="",
                waiting_on="",
                handoff_to="",
                notes="",
                priority="normal",
                no_reply_required=False,
                idempotency_key="",
            )

            with patch.dict(os.environ, {"CCCC_ACTOR_ID": "小抠"}, clear=False), \
                 patch.object(cli, "_ensure_daemon_running", return_value=True), \
                 patch.object(cli, "call_daemon", return_value={"ok": True, "result": {}}) as call_daemon, \
                 patch.object(cli, "_print_json"):
                code = cli.cmd_tracked_send(args)

            self.assertEqual(code, 0)
            req = call_daemon.call_args.args[0]
            self.assertEqual(req.get("op"), "tracked_send")
            self.assertEqual((req.get("args") or {}).get("by"), "小抠")
        finally:
            cleanup()

    def test_actor_add_does_not_fallback_after_daemon_rejection(self) -> None:
        from cccc import cli

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            resp = {"ok": False, "error": {"code": "permission_denied", "message": "blocked by daemon"}}
            args = Namespace(
                group=group_id,
                actor_id="peer1",
                title="Peer 1",
                by="user",
                submit="enter",
                runner="pty",
                runtime="codex",
                command="",
                env=[],
                scope="",
            )

            with patch.object(cli, "_ensure_daemon_running", return_value=True), \
                 patch.object(cli, "call_daemon", return_value=resp), \
                 patch.object(cli, "append_event", side_effect=AssertionError("local append should not run")) as append_event, \
                 patch.object(cli, "_print_json") as print_json:
                code = cli.cmd_actor_add(args)

            self.assertEqual(code, 2)
            append_event.assert_not_called()
            print_json.assert_called_once_with(resp)
        finally:
            cleanup()

    def test_group_update_does_not_fallback_after_daemon_rejection(self) -> None:
        from cccc import cli

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            resp = {"ok": False, "error": {"code": "permission_denied", "message": "blocked by daemon"}}
            args = Namespace(group=group_id, by="user", title="new-title", topic=None)

            with patch.object(cli, "_ensure_daemon_running", return_value=True), \
                 patch.object(cli, "call_daemon", return_value=resp), \
                 patch.object(cli, "append_event", side_effect=AssertionError("local append should not run")) as append_event, \
                 patch.object(cli, "_print_json") as print_json:
                code = cli.cmd_group_update(args)

            self.assertEqual(code, 2)
            append_event.assert_not_called()
            print_json.assert_called_once_with(resp)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
