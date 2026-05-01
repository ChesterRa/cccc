import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestActorLifecycleOps(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            try:
                from cccc.daemon.claude_app_sessions import SUPERVISOR as claude_app_supervisor
                from cccc.daemon.codex_app_sessions import SUPERVISOR as codex_app_supervisor
                from cccc.runners import headless as headless_runner
                from cccc.runners import pty as pty_runner

                codex_app_supervisor.stop_all()
                claude_app_supervisor.stop_all()
                headless_runner.SUPERVISOR.stop_all()
                pty_runner.SUPERVISOR.stop_all()
            except Exception:
                pass
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

    def _global_event_kinds(self, home_path: str) -> list[str]:
        path = Path(home_path) / "daemon" / "ccccd.events.jsonl"
        if not path.exists():
            return []
        kinds: list[str] = []
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            raw = line.strip()
            if not raw:
                continue
            try:
                payload = json.loads(raw)
            except Exception:
                continue
            kind = payload.get("kind")
            if isinstance(kind, str) and kind:
                kinds.append(kind)
        return kinds

    def test_actor_start_stop_transitions_group_running(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-lifecycle", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            stop, _ = self._call("actor_stop", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))
            actor_after_stop = (stop.result or {}).get("actor") if isinstance(stop.result, dict) else {}
            self.assertIsInstance(actor_after_stop, dict)
            assert isinstance(actor_after_stop, dict)
            self.assertFalse(bool(actor_after_stop.get("enabled", True)))

            show_after_stop, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_stop.ok, getattr(show_after_stop, "error", None))
            group_doc_after_stop = (show_after_stop.result or {}).get("group") if isinstance(show_after_stop.result, dict) else {}
            self.assertIsInstance(group_doc_after_stop, dict)
            assert isinstance(group_doc_after_stop, dict)
            self.assertFalse(bool(group_doc_after_stop.get("running")))

            start, _ = self._call("actor_start", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))
            actor_after_start = (start.result or {}).get("actor") if isinstance(start.result, dict) else {}
            self.assertIsInstance(actor_after_start, dict)
            assert isinstance(actor_after_start, dict)
            self.assertTrue(bool(actor_after_start.get("enabled", False)))

            show_after_start, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_start.ok, getattr(show_after_start, "error", None))
            group_doc_after_start = (show_after_start.result or {}).get("group") if isinstance(show_after_start.result, dict) else {}
            self.assertIsInstance(group_doc_after_start, dict)
            assert isinstance(group_doc_after_start, dict)
            self.assertTrue(bool(group_doc_after_start.get("running")))
        finally:
            cleanup()

    def test_actor_stop_keeps_internal_voice_secretary_enabled(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "voice-secretary-stop", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            lead, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "lead",
                    "title": "Lead",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(lead.ok, getattr(lead, "error", None))

            enable, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {"enabled": True},
                },
            )
            self.assertTrue(enable.ok, getattr(enable, "error", None))

            stop, _ = self._call("actor_stop", {"group_id": group_id, "actor_id": "voice-secretary", "by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))
            actor_after_stop = (stop.result or {}).get("actor") if isinstance(stop.result, dict) else {}
            self.assertIsInstance(actor_after_stop, dict)
            assert isinstance(actor_after_stop, dict)
            self.assertTrue(bool(actor_after_stop.get("enabled", False)))

            from cccc.kernel.actors import find_actor
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            stored_actor = find_actor(group, "voice-secretary")
            self.assertIsInstance(stored_actor, dict)
            assert isinstance(stored_actor, dict)
            self.assertTrue(bool(stored_actor.get("enabled", False)))
        finally:
            cleanup()

    def test_actor_restart_keeps_actor_enabled(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-restart", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            restart, _ = self._call("actor_restart", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(restart.ok, getattr(restart, "error", None))
            actor = (restart.result or {}).get("actor") if isinstance(restart.result, dict) else {}
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertTrue(bool(actor.get("enabled", False)))

            event = (restart.result or {}).get("event") if isinstance(restart.result, dict) else {}
            self.assertIsInstance(event, dict)
            assert isinstance(event, dict)
            self.assertEqual(str(event.get("kind") or ""), "actor.restart")
        finally:
            cleanup()

    def test_actor_start_clears_stale_execution_state(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-start-clear", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            from cccc.kernel.group import load_group
            from cccc.kernel.context import ContextStorage
            group = load_group(group_id)
            self.assertIsNotNone(group)
            storage = ContextStorage(group)  # type: ignore[arg-type]
            storage.update_agent_state("peer1", "Old focus", active_task_id="T999")

            start, _ = self._call("actor_start", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))

            refreshed = load_group(group_id)
            self.assertIsNotNone(refreshed)
            refreshed_storage = ContextStorage(refreshed)  # type: ignore[arg-type]
            agents = refreshed_storage.load_agents().agents
            agent = next((item for item in agents if getattr(item, "id", "") == "peer1"), None)
            self.assertIsNotNone(agent)
            hot = agent.hot if agent is not None else None
            self.assertEqual(str(getattr(hot, "active_task_id", "") or ""), "")
            self.assertEqual(str(getattr(hot, "focus", "") or ""), "")
        finally:
            cleanup()

    def test_actor_restart_clears_stale_execution_state(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-restart-clear", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            from cccc.kernel.group import load_group
            from cccc.kernel.context import ContextStorage
            group = load_group(group_id)
            self.assertIsNotNone(group)
            storage = ContextStorage(group)  # type: ignore[arg-type]
            storage.update_agent_state("peer1", "Old focus", active_task_id="T999")

            restart, _ = self._call("actor_restart", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(restart.ok, getattr(restart, "error", None))

            refreshed = load_group(group_id)
            self.assertIsNotNone(refreshed)
            refreshed_storage = ContextStorage(refreshed)  # type: ignore[arg-type]
            agents = refreshed_storage.load_agents().agents
            agent = next((item for item in agents if getattr(item, "id", "") == "peer1"), None)
            self.assertIsNotNone(agent)
            hot = agent.hot if agent is not None else None
            self.assertEqual(str(getattr(hot, "active_task_id", "") or ""), "")
            self.assertEqual(str(getattr(hot, "focus", "") or ""), "")
        finally:
            cleanup()

    def test_actor_remove_stops_group_when_last_actor_removed(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-remove", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            remove, _ = self._call("actor_remove", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(remove.ok, getattr(remove, "error", None))
            self.assertEqual(str((remove.result or {}).get("actor_id") or ""), "peer1")

            show, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show.ok, getattr(show, "error", None))
            group_doc = (show.result or {}).get("group") if isinstance(show.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertFalse(bool(group_doc.get("running")))
        finally:
            cleanup()

    def test_actor_remove_stops_codex_session_and_publishes_global_event(self) -> None:
        home, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-remove-publish", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            with patch("cccc.daemon.actors.actor_membership_ops.codex_app_supervisor.stop_actor") as stop_actor:
                remove, _ = self._call("actor_remove", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(remove.ok, getattr(remove, "error", None))
            stop_actor.assert_called_once_with(group_id=group_id, actor_id="peer1")
            self.assertIn("actor.remove", self._global_event_kinds(home))
        finally:
            cleanup()

    def test_web_model_actor_recreate_resets_browser_binding_state(self) -> None:
        from cccc.ports.web_model_browser_sidecar import (
            chatgpt_browser_profile_dir,
            read_chatgpt_browser_state,
            record_chatgpt_browser_state,
        )

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "web-model-recreate", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            profile_dir = chatgpt_browser_profile_dir(group_id, "peer1")
            marker = profile_dir / "login-marker"
            marker.write_text("keep login profile", encoding="utf-8")
            record_chatgpt_browser_state(
                group_id,
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/old-chat",
                    "bootstrap_seed_delivered_at": "2026-04-29T00:00:00Z",
                    "bootstrap_seed_version": "old",
                    "bootstrap_seed_digest": "old-digest",
                    "bootstrap_seed_conversation_url": "https://chatgpt.com/c/old-chat",
                    "last_turn_id": "old-turn",
                    "last_event_ids": ["old-event"],
                },
            )

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Web Model",
                    "runtime": "web_model",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))
            state_after_add = read_chatgpt_browser_state(group_id, "peer1")
            self.assertEqual(state_after_add.get("conversation_url"), "")
            self.assertEqual(state_after_add.get("bootstrap_seed_delivered_at"), "")
            self.assertEqual(state_after_add.get("bootstrap_seed_digest"), "")
            self.assertEqual(state_after_add.get("last_turn_id"), "")
            self.assertEqual(state_after_add.get("last_event_ids"), [])
            self.assertTrue(marker.exists())

            record_chatgpt_browser_state(
                group_id,
                "peer1",
                {
                    "conversation_url": "https://chatgpt.com/c/current-chat",
                    "bootstrap_seed_delivered_at": "2026-04-29T01:00:00Z",
                    "bootstrap_seed_version": "current",
                    "bootstrap_seed_digest": "current-digest",
                    "bootstrap_seed_conversation_url": "https://chatgpt.com/c/current-chat",
                    "last_turn_id": "current-turn",
                    "last_event_ids": ["current-event"],
                },
            )

            remove, _ = self._call("actor_remove", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(remove.ok, getattr(remove, "error", None))
            state_after_remove = read_chatgpt_browser_state(group_id, "peer1")
            self.assertEqual(state_after_remove.get("conversation_url"), "")
            self.assertEqual(state_after_remove.get("bootstrap_seed_delivered_at"), "")
            self.assertEqual(state_after_remove.get("bootstrap_seed_digest"), "")
            self.assertEqual(state_after_remove.get("last_turn_id"), "")
            self.assertEqual(state_after_remove.get("last_event_ids"), [])
            self.assertTrue(marker.exists())
        finally:
            cleanup()

    def test_actor_add_and_remove_schedule_summary_snapshot_rebuild(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-summary-rebuild", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            with patch(
                "cccc.daemon.actors.actor_add_ops._schedule_summary_snapshot_rebuild",
                return_value=True,
            ) as add_schedule:
                add, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "title": "Peer 1",
                        "runtime": "codex",
                        "runner": "headless",
                        "by": "user",
                    },
                )
            self.assertTrue(add.ok, getattr(add, "error", None))
            add_schedule.assert_called_once_with(group_id)

            with patch(
                "cccc.daemon.actors.actor_membership_ops._schedule_summary_snapshot_rebuild",
                return_value=True,
            ) as remove_schedule:
                remove, _ = self._call("actor_remove", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(remove.ok, getattr(remove, "error", None))
            remove_schedule.assert_called_once_with(group_id)
        finally:
            cleanup()

    def test_actor_update_enabled_toggle_preserves_running_semantics(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-update", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            disable, _ = self._call(
                "actor_update",
                {"group_id": group_id, "actor_id": "peer1", "by": "user", "patch": {"enabled": False}},
            )
            self.assertTrue(disable.ok, getattr(disable, "error", None))
            actor_after_disable = (disable.result or {}).get("actor") if isinstance(disable.result, dict) else {}
            self.assertIsInstance(actor_after_disable, dict)
            assert isinstance(actor_after_disable, dict)
            self.assertFalse(bool(actor_after_disable.get("enabled", True)))

            show_after_disable, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_disable.ok, getattr(show_after_disable, "error", None))
            group_doc_after_disable = (show_after_disable.result or {}).get("group") if isinstance(show_after_disable.result, dict) else {}
            self.assertIsInstance(group_doc_after_disable, dict)
            assert isinstance(group_doc_after_disable, dict)
            self.assertFalse(bool(group_doc_after_disable.get("running")))

            enable, _ = self._call(
                "actor_update",
                {"group_id": group_id, "actor_id": "peer1", "by": "user", "patch": {"enabled": True}},
            )
            self.assertTrue(enable.ok, getattr(enable, "error", None))
            actor_after_enable = (enable.result or {}).get("actor") if isinstance(enable.result, dict) else {}
            self.assertIsInstance(actor_after_enable, dict)
            assert isinstance(actor_after_enable, dict)
            self.assertTrue(bool(actor_after_enable.get("enabled", False)))

            show_after_enable, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show_after_enable.ok, getattr(show_after_enable, "error", None))
            group_doc_after_enable = (show_after_enable.result or {}).get("group") if isinstance(show_after_enable.result, dict) else {}
            self.assertIsInstance(group_doc_after_enable, dict)
            assert isinstance(group_doc_after_enable, dict)
            self.assertFalse(bool(group_doc_after_enable.get("running")))
        finally:
            cleanup()

    def test_actor_lifecycle_global_events_use_contract_kinds(self) -> None:
        home, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "actor-events", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            start, _ = self._call("actor_start", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))

            restart, _ = self._call("actor_restart", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(restart.ok, getattr(restart, "error", None))

            stop, _ = self._call("actor_stop", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))

            kinds = self._global_event_kinds(home)
            self.assertIn("actor.start", kinds)
            self.assertIn("actor.restart", kinds)
            self.assertIn("actor.stop", kinds)
            self.assertNotIn("actor.started", kinds)
            self.assertNotIn("actor.restarted", kinds)
            self.assertNotIn("actor.stopped", kinds)
        finally:
            cleanup()

    def test_actor_start_does_not_resume_paused_group(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "paused-start", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            set_state, _ = self._call("group_set_state", {"group_id": group_id, "state": "paused", "by": "user"})
            self.assertTrue(set_state.ok, getattr(set_state, "error", None))

            start, _ = self._call("actor_start", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))

            show, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show.ok, getattr(show, "error", None))
            group_doc = (show.result or {}).get("group") if isinstance(show.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertEqual(str(group_doc.get("state") or ""), "paused")
            self.assertTrue(bool(group_doc.get("running")))
        finally:
            cleanup()

    def test_actor_start_resumes_stopped_group_to_active(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "stopped-start", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            stop, _ = self._call("group_stop", {"group_id": group_id, "by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))

            start, _ = self._call("actor_start", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))

            show, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show.ok, getattr(show, "error", None))
            group_doc = (show.result or {}).get("group") if isinstance(show.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertEqual(str(group_doc.get("state") or ""), "active")
            self.assertTrue(bool(group_doc.get("running")))
        finally:
            cleanup()

    def test_actor_restart_does_not_resume_paused_group(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "paused-restart", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            set_state, _ = self._call("group_set_state", {"group_id": group_id, "state": "paused", "by": "user"})
            self.assertTrue(set_state.ok, getattr(set_state, "error", None))

            restart, _ = self._call("actor_restart", {"group_id": group_id, "actor_id": "peer1", "by": "user"})
            self.assertTrue(restart.ok, getattr(restart, "error", None))

            show, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show.ok, getattr(show, "error", None))
            group_doc = (show.result or {}).get("group") if isinstance(show.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertEqual(str(group_doc.get("state") or ""), "paused")
        finally:
            cleanup()

if __name__ == "__main__":
    unittest.main()
