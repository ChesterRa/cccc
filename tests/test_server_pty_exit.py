import os
import json
import tempfile
import unittest
from pathlib import Path


class TestServerPtyExit(unittest.TestCase):
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

    def test_pty_process_exit_persists_visible_actor_as_stopped(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.runner_state_ops import pty_state_path, write_pty_state
            from cccc.daemon.server import _handle_pty_session_exit
            from cccc.kernel.actors import add_actor, find_actor, update_actor
            from cccc.kernel.group import load_group

            create, _ = self._call("group_create", {"title": "pty-exit", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="peer1", runner="pty", runtime="codex", enabled=True)
            update_actor(group, "peer1", {"runtime_state_source": "terminal"})
            group.doc["running"] = True
            group.save()
            write_pty_state(group_id, "peer1", pid=12345)

            session = type("_Session", (), {"group_id": group_id, "actor_id": "peer1", "pid": 12345})()
            _handle_pty_session_exit(session)  # type: ignore[arg-type]

            reloaded = load_group(group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            actor = find_actor(reloaded, "peer1")
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertFalse(bool(actor.get("enabled")))
            self.assertFalse(bool(reloaded.doc.get("running")))
            self.assertFalse(pty_state_path(group_id, "peer1").exists())
            ledger_events = [
                json.loads(line)
                for line in reloaded.ledger_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            stop_events = [event for event in ledger_events if event.get("kind") == "actor.stop"]
            self.assertEqual(len(stop_events), 1)
            self.assertEqual(stop_events[0].get("by"), "system")
        finally:
            cleanup()

    def test_codex_app_server_backed_pty_exit_does_not_disable_actor(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.runner_state_ops import pty_state_path, write_pty_state
            from cccc.daemon.server import _handle_pty_session_exit
            from cccc.kernel.actors import add_actor, find_actor, update_actor
            from cccc.kernel.group import load_group

            create, _ = self._call("group_create", {"title": "pty-app-server-exit", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="peer1", runner="pty", runtime="codex", enabled=True)
            update_actor(group, "peer1", {"runtime_state_source": "app_server"})
            group.doc["running"] = True
            group.save()
            write_pty_state(group_id, "peer1", pid=45678)

            session = type("_Session", (), {"group_id": group_id, "actor_id": "peer1", "pid": 45678})()
            _handle_pty_session_exit(session)  # type: ignore[arg-type]

            reloaded = load_group(group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            actor = find_actor(reloaded, "peer1")
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertTrue(bool(actor.get("enabled")))
            self.assertTrue(bool(reloaded.doc.get("running")))
            self.assertFalse(pty_state_path(group_id, "peer1").exists())
            ledger_events = [
                json.loads(line)
                for line in reloaded.ledger_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            stop_events = [event for event in ledger_events if event.get("kind") == "actor.stop"]
            self.assertEqual(stop_events, [])
        finally:
            cleanup()

    def test_stale_pty_process_exit_does_not_disable_restarted_actor(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.runner_state_ops import pty_state_path, write_pty_state
            from cccc.daemon.server import _handle_pty_session_exit
            from cccc.kernel.actors import add_actor, find_actor
            from cccc.kernel.group import load_group

            create, _ = self._call("group_create", {"title": "pty-stale-exit", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="peer1", runner="pty", runtime="codex", enabled=True)
            group.doc["running"] = True
            group.save()

            write_pty_state(group_id, "peer1", pid=22222)
            stale_session = type("_Session", (), {"group_id": group_id, "actor_id": "peer1", "pid": 11111})()
            _handle_pty_session_exit(stale_session)  # type: ignore[arg-type]

            reloaded = load_group(group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            actor = find_actor(reloaded, "peer1")
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertTrue(bool(actor.get("enabled")))
            self.assertTrue(bool(reloaded.doc.get("running")))
            state_path = pty_state_path(group_id, "peer1")
            self.assertTrue(state_path.exists())
            state_doc = json.loads(state_path.read_text(encoding="utf-8"))
            self.assertEqual(state_doc.get("pid"), 22222)
            runtime_session_path = (
                Path(os.environ["CCCC_HOME"])
                / "groups"
                / group_id
                / "state"
                / "runtime_sessions"
                / "peer1.json"
            )
            self.assertFalse(runtime_session_path.exists())
            ledger_events = [
                json.loads(line)
                for line in reloaded.ledger_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            stop_events = [event for event in ledger_events if event.get("kind") == "actor.stop"]
            self.assertEqual(stop_events, [])
        finally:
            cleanup()

    def test_pty_process_exit_does_not_disable_internal_assistant_actor(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.server import _handle_pty_session_exit
            from cccc.kernel.actors import INTERNAL_KIND_VOICE_SECRETARY, add_actor, find_actor
            from cccc.kernel.group import load_group

            create, _ = self._call("group_create", {"title": "pty-internal-exit", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(
                group,
                actor_id="voice-secretary",
                runner="pty",
                runtime="codex",
                enabled=True,
                internal_kind=INTERNAL_KIND_VOICE_SECRETARY,
            )

            session = type("_Session", (), {"group_id": group_id, "actor_id": "voice-secretary", "pid": 23456})()
            _handle_pty_session_exit(session)  # type: ignore[arg-type]

            reloaded = load_group(group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            actor = find_actor(reloaded, "voice-secretary")
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertTrue(bool(actor.get("enabled")))
        finally:
            cleanup()

    def test_daemon_shutdown_pty_cleanup_does_not_disable_actor(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.runner_state_ops import pty_state_path, write_pty_state
            from cccc.daemon.server import _handle_pty_session_exit
            from cccc.kernel.actors import add_actor, find_actor
            from cccc.kernel.group import load_group

            create, _ = self._call("group_create", {"title": "pty-shutdown", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="peer1", runner="pty", runtime="codex", enabled=True)
            group.doc["running"] = True
            group.save()
            write_pty_state(group_id, "peer1", pid=34567)

            session = type("_Session", (), {"group_id": group_id, "actor_id": "peer1", "pid": 34567})()
            _handle_pty_session_exit(session, persist_actor_stopped=False)  # type: ignore[arg-type]

            reloaded = load_group(group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            actor = find_actor(reloaded, "peer1")
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertTrue(bool(actor.get("enabled")))
            self.assertTrue(bool(reloaded.doc.get("running")))
            self.assertFalse(pty_state_path(group_id, "peer1").exists())
        finally:
            cleanup()

    def test_headless_process_exit_uses_same_persistent_stop_semantics(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.actors.actor_exit_ops import persist_actor_process_exit_stopped
            from cccc.kernel.actors import add_actor, find_actor
            from cccc.kernel.group import load_group

            create, _ = self._call("group_create", {"title": "headless-exit", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="peer1", runner="headless", runtime="codex", enabled=True)
            group.doc["running"] = True
            group.save()

            changed = persist_actor_process_exit_stopped(group_id=group_id, actor_id="peer1", runner="headless")

            self.assertTrue(changed)
            reloaded = load_group(group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            actor = find_actor(reloaded, "peer1")
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertFalse(bool(actor.get("enabled")))
            self.assertFalse(bool(reloaded.doc.get("running")))
        finally:
            cleanup()
