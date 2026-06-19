"""Context v3 focused tests: CAS, coordination brief, task lifecycle, permissions, and meta validation."""

import json
import os
import tempfile
import unittest
from unittest.mock import patch

class TestContextV2Ops(unittest.TestCase):
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

    def _create_group(self, title: str = "v3-test") -> str:
        resp, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        gid = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _sync(self, gid: str, ops: list[dict], by: str = "user"):
        return self._call("context_sync", {"group_id": gid, "by": by, "ops": ops})

    def _context(self, gid: str):
        return self._call("context_get", {"group_id": gid})

    def _summary_context(self, gid: str):
        return self._call("context_get", {"group_id": gid, "detail": "summary"})

    def _tasks(self, gid: str):
        return self._call("task_list", {"group_id": gid})

    def test_cas_version_conflict_rejects_batch(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            get_resp, _ = self._context(gid)
            version = (get_resp.result or {}).get("version")
            ok_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "coordination.brief.update", "objective": "Ship B3"}],
                    "if_version": version,
                },
            )
            self.assertTrue(ok_resp.ok, getattr(ok_resp, "error", None))
            fail_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "coordination.brief.update", "objective": "Ship B4"}],
                    "if_version": version,
                },
            )
            self.assertFalse(fail_resp.ok)
            self.assertEqual(fail_resp.error.code, "version_conflict")
        finally:
            cleanup()

    def test_coordination_brief_update(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            resp, _ = self._sync(
                gid,
                [
                    {
                        "op": "coordination.brief.update",
                        "objective": "Ship context redesign",
                        "current_focus": "Board-first B3 rollout",
                        "constraints": ["No backward-compat ballast", "Keep bootstrap lean"],
                        "project_brief": "Replace fragmented context with coordination + agent_state.",
                    }
                ],
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            ctx, _ = self._context(gid)
            brief = ctx.result["coordination"]["brief"]
            self.assertEqual(brief["objective"], "Ship context redesign")
            self.assertEqual(brief["current_focus"], "Board-first B3 rollout")
            self.assertEqual(brief["constraints"], ["No backward-compat ballast", "Keep bootstrap lean"])
            self.assertIn("coordination + agent_state", brief["project_brief"])
        finally:
            cleanup()

    def test_context_get_marks_running_pty_without_prompt_as_waiting(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            from cccc.kernel.actors import add_actor
            from cccc.kernel.context import ContextStorage
            from cccc.kernel.group import load_group

            group = load_group(gid)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", runtime="codex", runner="pty")  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            storage = ContextStorage(group)  # type: ignore[arg-type]
            storage.update_agent_state("peer1", "Implement feature", active_task_id="T123")

            with patch("cccc.daemon.context.context_ops.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
                "cccc.daemon.context.context_ops.pty_runner.SUPERVISOR.idle_seconds",
                return_value=8.0,
            ):
                ctx, _ = self._context(gid)

            self.assertTrue(ctx.ok, getattr(ctx, "error", None))
            actors_runtime = ctx.result.get("actors_runtime") if isinstance(ctx.result, dict) else []
            self.assertEqual(len(actors_runtime), 1)
            self.assertEqual(actors_runtime[0]["id"], "peer1")
            self.assertEqual(actors_runtime[0]["effective_working_state"], "waiting")
            self.assertEqual(actors_runtime[0]["effective_working_reason"], "pty_no_prompt_waiting")
            self.assertEqual(actors_runtime[0]["effective_active_task_id"], "T123")
        finally:
            cleanup()

    def test_context_get_does_not_mark_fresh_running_pty_without_output_as_working(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group

            group = load_group(gid)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", runtime="codex", runner="pty")  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            with (
                patch("cccc.daemon.context.context_ops.pty_runner.SUPERVISOR.actor_running", return_value=True),
                patch("cccc.daemon.context.context_ops.pty_runner.SUPERVISOR.idle_seconds", return_value=0.6),
                patch("cccc.daemon.context.context_ops.pty_runner.SUPERVISOR.tail_output", return_value=b""),
            ):
                ctx, _ = self._context(gid)

            self.assertTrue(ctx.ok, getattr(ctx, "error", None))
            actors_runtime = ctx.result.get("actors_runtime") if isinstance(ctx.result, dict) else []
            self.assertEqual(len(actors_runtime), 1)
            self.assertEqual(actors_runtime[0]["effective_working_state"], "waiting")
            self.assertEqual(actors_runtime[0]["effective_working_reason"], "pty_no_prompt_waiting")
        finally:
            cleanup()

    def test_summary_context_hit_skips_rebuild_work_on_second_read(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            ok_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [
                        {"op": "coordination.brief.update", "objective": "Ship snapshot"},
                        {"op": "task.create", "title": "T1", "outcome": "G1"},
                    ],
                },
            )
            self.assertTrue(ok_resp.ok, getattr(ok_resp, "error", None))

            from cccc.daemon.context.context_ops import _rebuild_summary_snapshot

            from cccc.kernel.context import ContextStorage

            _rebuild_summary_snapshot(gid)

            original_list_tasks = ContextStorage.list_tasks
            original_load_agents = ContextStorage.load_agents
            original_load_context = ContextStorage.load_context
            original_compute_version = ContextStorage.compute_version

            with (
                patch.object(ContextStorage, "list_tasks", autospec=True, side_effect=original_list_tasks) as mock_list_tasks,
                patch.object(ContextStorage, "load_agents", autospec=True, side_effect=original_load_agents) as mock_load_agents,
                patch.object(ContextStorage, "load_context", autospec=True, side_effect=original_load_context) as mock_load_context,
                patch.object(ContextStorage, "compute_version", autospec=True, side_effect=original_compute_version) as mock_compute_version,
            ):
                first, _ = self._summary_context(gid)
                self.assertTrue(first.ok, getattr(first, "error", None))
                first_counts = (
                    mock_list_tasks.call_count,
                    mock_load_agents.call_count,
                    mock_load_context.call_count,
                    mock_compute_version.call_count,
                )
                self.assertEqual(first_counts, (0, 0, 0, 0))

                second, _ = self._summary_context(gid)
                self.assertTrue(second.ok, getattr(second, "error", None))
                self.assertEqual(second.result["version"], first.result["version"])
                self.assertEqual(
                    (
                        mock_list_tasks.call_count,
                        mock_load_agents.call_count,
                        mock_load_context.call_count,
                        mock_compute_version.call_count,
                    ),
                    first_counts,
                )
        finally:
            cleanup()

    def test_summary_context_rebuilds_after_context_sync(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            seed_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "coordination.brief.update", "objective": "Initial"}],
                },
            )
            self.assertTrue(seed_resp.ok, getattr(seed_resp, "error", None))

            from cccc.daemon.context.context_ops import _rebuild_summary_snapshot

            from cccc.kernel.context import ContextStorage

            _rebuild_summary_snapshot(gid)

            original_list_tasks = ContextStorage.list_tasks
            original_load_agents = ContextStorage.load_agents
            original_load_context = ContextStorage.load_context
            original_compute_version = ContextStorage.compute_version

            with (
                patch.object(ContextStorage, "list_tasks", autospec=True, side_effect=original_list_tasks) as mock_list_tasks,
                patch.object(ContextStorage, "load_agents", autospec=True, side_effect=original_load_agents) as mock_load_agents,
                patch.object(ContextStorage, "load_context", autospec=True, side_effect=original_load_context) as mock_load_context,
                patch.object(ContextStorage, "compute_version", autospec=True, side_effect=original_compute_version) as mock_compute_version,
                patch("cccc.daemon.context.context_ops._schedule_summary_snapshot_rebuild", return_value=True) as mock_schedule,
            ):
                first, _ = self._summary_context(gid)
                self.assertTrue(first.ok, getattr(first, "error", None))
                hit_counts = (
                    mock_list_tasks.call_count,
                    mock_load_agents.call_count,
                    mock_load_context.call_count,
                    mock_compute_version.call_count,
                )

                sync_resp, _ = self._call(
                    "context_sync",
                    {
                        "group_id": gid,
                        "by": "user",
                        "ops": [{"op": "coordination.brief.update", "objective": "Updated"}],
                    },
                )
                self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))
                counts_after_sync = (
                    mock_list_tasks.call_count,
                    mock_load_agents.call_count,
                    mock_load_context.call_count,
                    mock_compute_version.call_count,
                )

                stale, _ = self._summary_context(gid)
                self.assertTrue(stale.ok, getattr(stale, "error", None))
                self.assertEqual(stale.result["version"], first.result["version"])
                self.assertEqual(((stale.result or {}).get("meta") or {}).get("summary_snapshot", {}).get("state"), "stale")
                rebuilt_counts = (
                    mock_list_tasks.call_count,
                    mock_load_agents.call_count,
                    mock_load_context.call_count,
                    mock_compute_version.call_count,
                )
                self.assertEqual(
                    rebuilt_counts,
                    counts_after_sync,
                )
                self.assertEqual(mock_schedule.call_count, 2)
                mock_schedule.assert_any_call(gid)
        finally:
            cleanup()

    def test_summary_context_miss_returns_empty_snapshot_and_schedules_rebuild(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()

            from cccc.kernel.context import ContextStorage

            original_list_tasks = ContextStorage.list_tasks
            original_load_agents = ContextStorage.load_agents
            original_load_context = ContextStorage.load_context

            with (
                patch.object(ContextStorage, "list_tasks", autospec=True, side_effect=original_list_tasks) as mock_list_tasks,
                patch.object(ContextStorage, "load_agents", autospec=True, side_effect=original_load_agents) as mock_load_agents,
                patch.object(ContextStorage, "load_context", autospec=True, side_effect=original_load_context) as mock_load_context,
                patch("cccc.daemon.context.context_ops._schedule_summary_snapshot_rebuild", return_value=True) as mock_schedule,
            ):
                summary, _ = self._summary_context(gid)
                self.assertTrue(summary.ok, getattr(summary, "error", None))
                self.assertEqual((summary.result or {}).get("coordination", {}).get("tasks"), [])
                self.assertEqual((summary.result or {}).get("agent_states"), [])
                self.assertEqual(((summary.result or {}).get("meta") or {}).get("summary_snapshot", {}).get("state"), "missing")
                self.assertEqual(mock_list_tasks.call_count, 0)
                self.assertEqual(mock_load_agents.call_count, 0)
                self.assertEqual(mock_load_context.call_count, 0)
                mock_schedule.assert_called_once_with(gid)
        finally:
            cleanup()

    def test_context_sync_schedules_summary_snapshot_rebuild_after_write(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            with patch("cccc.daemon.context.context_ops._schedule_summary_snapshot_rebuild", return_value=True) as mock_schedule:
                resp, _ = self._sync(
                    gid,
                    [{"op": "coordination.brief.update", "objective": "Schedule rebuild"}],
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            mock_schedule.assert_called_once_with(gid)
        finally:
            cleanup()

    def test_context_sync_dry_run_does_not_schedule_summary_snapshot_rebuild(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            with patch("cccc.daemon.context.context_ops._schedule_summary_snapshot_rebuild", return_value=True) as mock_schedule:
                resp, _ = self._call(
                    "context_sync",
                    {
                        "group_id": gid,
                        "by": "user",
                        "dry_run": True,
                        "ops": [{"op": "coordination.brief.update", "objective": "Dry run only"}],
                    },
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            mock_schedule.assert_not_called()
        finally:
            cleanup()

    def test_rebuild_summary_snapshot_retries_until_basis_and_version_stabilize(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "coordination.brief.update", "objective": "Stable snapshot"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))

            from cccc.daemon.context.context_ops import _rebuild_summary_snapshot
            from cccc.kernel.context import ContextStorage

            stable_basis = {"context_rev": 1, "tasks_rev": 0, "agents_rev": 0, "actors_rev": 0}
            newer_basis = {"context_rev": 2, "tasks_rev": 0, "agents_rev": 0, "actors_rev": 0}

            original_load_context = ContextStorage.load_context
            original_list_tasks = ContextStorage.list_tasks
            original_load_agents = ContextStorage.load_agents
            original_compute_version = ContextStorage.compute_version

            with (
                patch.object(ContextStorage, "load_context", autospec=True, side_effect=original_load_context) as mock_load_context,
                patch.object(ContextStorage, "list_tasks", autospec=True, side_effect=original_list_tasks) as mock_list_tasks,
                patch.object(ContextStorage, "load_agents", autospec=True, side_effect=original_load_agents) as mock_load_agents,
                patch.object(
                    ContextStorage,
                    "summary_basis",
                    autospec=True,
                    side_effect=[stable_basis, newer_basis, newer_basis, newer_basis],
                ),
                patch.object(
                    ContextStorage,
                    "compute_version",
                    autospec=True,
                    side_effect=["ctxv:1", "ctxv:2", "ctxv:2", "ctxv:2", "ctxv:2", "ctxv:2"],
                ),
                patch.object(ContextStorage, "save_summary_snapshot", autospec=True) as mock_save,
            ):
                _rebuild_summary_snapshot(gid)

            self.assertGreaterEqual(mock_load_context.call_count, 2)
            self.assertGreaterEqual(mock_list_tasks.call_count, 2)
            self.assertGreaterEqual(mock_load_agents.call_count, 2)
            mock_save.assert_called_once()
            _, kwargs = mock_save.call_args
            self.assertEqual(kwargs["basis"], newer_basis)
            self.assertEqual(kwargs["version"], "ctxv:2")
            self.assertEqual(str(kwargs["result"].get("version") or ""), "ctxv:2")
        finally:
            cleanup()

    def test_task_create_with_parent_id_and_update_parent_cycle_detection(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._sync(gid, [{"op": "task.create", "title": "Root", "outcome": "Root outcome"}])
            root_id = self._tasks(gid)[0].result["tasks"][0]["id"]
            self._sync(gid, [{"op": "task.create", "title": "Child", "outcome": "Child outcome", "parent_id": root_id}])
            tasks = self._tasks(gid)[0].result["tasks"]
            child = [task for task in tasks if task["parent_id"] == root_id][0]
            self.assertEqual(child["title"], "Child")

            cycle_resp, _ = self._sync(gid, [{"op": "task.update", "task_id": root_id, "parent_id": child["id"]}])
            self.assertFalse(cycle_resp.ok)
            self.assertIn("cycle", str(cycle_resp.error.message).lower())
        finally:
            cleanup()

    def test_task_type_round_trips_through_task_surfaces(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            create_resp, _ = self._sync(
                gid,
                [
                    {
                        "op": "task.create",
                        "title": "Optimize cold start",
                        "outcome": "Keep best branch visible",
                        "task_type": "optimization",
                        "notes": "Custom optimization note",
                    }
                ],
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))

            task_list_resp, _ = self._tasks(gid)
            self.assertTrue(task_list_resp.ok, getattr(task_list_resp, "error", None))
            tasks = (task_list_resp.result or {}).get("tasks") if isinstance(task_list_resp.result, dict) else []
            self.assertEqual(len(tasks), 1)
            self.assertEqual(tasks[0]["task_type"], "optimization")
            task_id = tasks[0]["id"]

            ctx_resp, _ = self._context(gid)
            self.assertTrue(ctx_resp.ok, getattr(ctx_resp, "error", None))
            ctx_tasks = (ctx_resp.result or {}).get("coordination", {}).get("tasks", [])
            self.assertEqual(len(ctx_tasks), 1)
            self.assertEqual(ctx_tasks[0]["task_type"], "optimization")

            single_resp, _ = self._call("task_list", {"group_id": gid, "task_id": task_id})
            self.assertTrue(single_resp.ok, getattr(single_resp, "error", None))
            single_task = (single_resp.result or {}).get("task") if isinstance(single_resp.result, dict) else None
            self.assertIsInstance(single_task, dict)
            self.assertEqual(single_task["task_type"], "optimization")
            self.assertEqual(single_task["notes"], "Custom optimization note")
            self.assertEqual(single_task.get("checklist") or [], [])
        finally:
            cleanup()

    def test_task_create_defaults_structural_task_type(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            root_resp, _ = self._sync(
                gid,
                [{"op": "task.create", "title": "Root task"}],
            )
            self.assertTrue(root_resp.ok, getattr(root_resp, "error", None))
            root_task = self._tasks(gid)[0].result["tasks"][0]
            self.assertEqual(root_task["task_type"], "standard")

            child_resp, _ = self._sync(
                gid,
                [{"op": "task.create", "title": "Child task", "parent_id": root_task["id"]}],
            )
            self.assertTrue(child_resp.ok, getattr(child_resp, "error", None))
            tasks = self._tasks(gid)[0].result["tasks"]
            child_task = next(task for task in tasks if task["id"] != root_task["id"])
            self.assertEqual(child_task["task_type"], "free")
        finally:
            cleanup()

    def test_task_create_with_type_keeps_notes_and_checklist_plain_when_blank(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            create_resp, _ = self._sync(
                gid,
                [
                    {
                        "op": "task.create",
                        "title": "Optimize runtime",
                        "task_type": "optimization",
                    }
                ],
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))

            task = self._tasks(gid)[0].result["tasks"][0]
            self.assertEqual(task["task_type"], "optimization")
            self.assertEqual(str(task.get("notes") or ""), "")
            self.assertEqual(task.get("checklist") or [], [])
        finally:
            cleanup()

    def test_task_create_with_type_preserves_explicit_notes_and_checklist(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            create_resp, _ = self._sync(
                gid,
                [
                    {
                        "op": "task.create",
                        "title": "Ship release",
                        "task_type": "standard",
                        "notes": "Custom release framing",
                        "checklist": [{"text": "Run smoke verification", "status": "pending"}],
                    }
                ],
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))

            task = self._tasks(gid)[0].result["tasks"][0]
            self.assertEqual(task["notes"], "Custom release framing")
            self.assertEqual(task["task_type"], "standard")
            checklist = task.get("checklist") or []
            self.assertTrue(any(str(item.get("text") or "") == "Run smoke verification" for item in checklist))
        finally:
            cleanup()

    def test_task_update_validates_task_type_and_does_not_realign_on_reparent(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            create_resp, _ = self._sync(
                gid,
                [{"op": "task.create", "title": "Ship release", "task_type": "standard"}],
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            task_id = self._tasks(gid)[0].result["tasks"][0]["id"]

            update_resp, _ = self._sync(
                gid,
                [{"op": "task.update", "task_id": task_id, "task_type": "optimization"}],
            )
            self.assertTrue(update_resp.ok, getattr(update_resp, "error", None))
            task = self._tasks(gid)[0].result["tasks"][0]
            self.assertEqual(task["task_type"], "optimization")

            invalid_null_resp, _ = self._sync(
                gid,
                [{"op": "task.update", "task_id": task_id, "task_type": None}],
            )
            self.assertFalse(invalid_null_resp.ok)
            self.assertIn("task_type", str(invalid_null_resp.error.message).lower())

            invalid_resp, _ = self._sync(
                gid,
                [{"op": "task.update", "task_id": task_id, "task_type": "invalid"}],
            )
            self.assertFalse(invalid_resp.ok)
            self.assertIn("task_type", str(invalid_resp.error.message).lower())

            root_resp, _ = self._sync(
                gid,
                [{"op": "task.create", "title": "Parent"}],
            )
            self.assertTrue(root_resp.ok, getattr(root_resp, "error", None))
            parent_task = next(task for task in self._tasks(gid)[0].result["tasks"] if task["title"] == "Parent")

            child_resp, _ = self._sync(
                gid,
                [{"op": "task.create", "title": "Child"}],
            )
            self.assertTrue(child_resp.ok, getattr(child_resp, "error", None))
            child_task = next(task for task in self._tasks(gid)[0].result["tasks"] if task["title"] == "Child")
            self.assertEqual(child_task["task_type"], "standard")

            reparent_resp, _ = self._sync(
                gid,
                [{"op": "task.update", "task_id": child_task["id"], "parent_id": parent_task["id"]}],
            )
            self.assertTrue(reparent_resp.ok, getattr(reparent_resp, "error", None))
            child_after_reparent = next(task for task in self._tasks(gid)[0].result["tasks"] if task["id"] == child_task["id"])
            self.assertEqual(child_after_reparent["task_type"], "standard")

            detach_resp, _ = self._sync(
                gid,
                [{"op": "task.update", "task_id": child_task["id"], "parent_id": None}],
            )
            self.assertTrue(detach_resp.ok, getattr(detach_resp, "error", None))
            child_after_detach = next(task for task in self._tasks(gid)[0].result["tasks"] if task["id"] == child_task["id"])
            self.assertEqual(child_after_detach["task_type"], "standard")
        finally:
            cleanup()

    def test_task_move_status_restore_and_invalid_status(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._sync(gid, [{"op": "task.create", "title": "T", "outcome": "G"}])
            task_id = self._tasks(gid)[0].result["tasks"][0]["id"]

            for status in ("active", "done", "archived"):
                resp, _ = self._sync(gid, [{"op": "task.move", "task_id": task_id, "status": status}])
                self.assertTrue(resp.ok, getattr(resp, "error", None))

            task = self._tasks(gid)[0].result["tasks"][0]
            self.assertEqual(task["status"], "archived")
            self.assertEqual(task["archived_from"], "done")

            restore_resp, _ = self._sync(gid, [{"op": "task.restore", "task_id": task_id}])
            self.assertTrue(restore_resp.ok, getattr(restore_resp, "error", None))
            task = self._tasks(gid)[0].result["tasks"][0]
            self.assertEqual(task["status"], "done")
            self.assertIsNone(task.get("archived_from"))

            bad_resp, _ = self._sync(gid, [{"op": "task.move", "task_id": task_id, "status": "pending"}])
            self.assertFalse(bad_resp.ok)
            self.assertIn("invalid task status", str(bad_resp.error.message).lower())
        finally:
            cleanup()

    def test_task_move_rejects_patch_fields(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._sync(gid, [{"op": "task.create", "title": "T", "outcome": "G"}])
            task_id = self._tasks(gid)[0].result["tasks"][0]["id"]

            bad_resp, _ = self._sync(
                gid,
                [{"op": "task.move", "task_id": task_id, "status": "done", "outcome": "ship it"}],
            )
            self.assertFalse(bad_resp.ok)
            self.assertIn("task.move only accepts task_id and status", str(bad_resp.error.message).lower())
            self.assertIn("task.update", str(bad_resp.error.message).lower())
        finally:
            cleanup()

    def test_task_restore_requires_archived_status(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._sync(gid, [{"op": "task.create", "title": "T", "outcome": "G"}])
            task_id = self._tasks(gid)[0].result["tasks"][0]["id"]

            restore_resp, _ = self._sync(gid, [{"op": "task.restore", "task_id": task_id}])
            self.assertFalse(restore_resp.ok)
            self.assertIn("archived", str(restore_resp.error.message).lower())
        finally:
            cleanup()

    def test_task_delete_removes_unexecuted_task_or_subtree(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._sync(gid, [{"op": "task.create", "title": "Draft", "outcome": "Scratch"}])
            planned_id = self._tasks(gid)[0].result["tasks"][0]["id"]

            delete_planned, _ = self._sync(gid, [{"op": "task.delete", "task_id": planned_id}])
            self.assertTrue(delete_planned.ok, getattr(delete_planned, "error", None))
            self.assertEqual(self._tasks(gid)[0].result["tasks"], [])

            self._sync(gid, [{"op": "task.create", "title": "Archived draft", "outcome": "Scratch"}])
            archived_id = self._tasks(gid)[0].result["tasks"][0]["id"]
            archive_resp, _ = self._sync(gid, [{"op": "task.move", "task_id": archived_id, "status": "archived"}])
            self.assertTrue(archive_resp.ok, getattr(archive_resp, "error", None))

            delete_archived, _ = self._sync(gid, [{"op": "task.delete", "task_id": archived_id}])
            self.assertTrue(delete_archived.ok, getattr(delete_archived, "error", None))
            self.assertEqual(self._tasks(gid)[0].result["tasks"], [])

            self._sync(gid, [{"op": "task.create", "title": "Parent", "outcome": "Scratch", "assignee": "peer1"}])
            parent_id = self._tasks(gid)[0].result["tasks"][0]["id"]
            self._sync(gid, [{"op": "task.create", "title": "Child", "outcome": "Scratch", "parent_id": parent_id, "blocked_by": ["note"]}])

            delete_parent, _ = self._sync(gid, [{"op": "task.delete", "task_id": parent_id}])
            self.assertTrue(delete_parent.ok, getattr(delete_parent, "error", None))
            self.assertEqual(self._tasks(gid)[0].result["tasks"], [])
        finally:
            cleanup()

    def test_task_delete_rejects_tasks_with_execution_history_in_self_or_subtree(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()

            self._sync(gid, [{"op": "task.create", "title": "Historical", "outcome": "Keep history"}])
            historical_id = self._tasks(gid)[0].result["tasks"][0]["id"]
            self._sync(gid, [{"op": "task.move", "task_id": historical_id, "status": "done"}])
            self._sync(gid, [{"op": "task.move", "task_id": historical_id, "status": "archived"}])
            delete_historical, _ = self._sync(gid, [{"op": "task.delete", "task_id": historical_id}])
            self.assertFalse(delete_historical.ok)
            self.assertIn("never moved past planned", str(delete_historical.error.message).lower())

            self._sync(gid, [{"op": "task.create", "title": "Parent", "outcome": "Has child"}])
            parent_id = next(task["id"] for task in self._tasks(gid)[0].result["tasks"] if task["title"] == "Parent")
            self._sync(gid, [{"op": "task.create", "title": "Child", "outcome": "Nested", "parent_id": parent_id}])
            child_id = next(task["id"] for task in self._tasks(gid)[0].result["tasks"] if task["title"] == "Child")
            self._sync(gid, [{"op": "task.move", "task_id": child_id, "status": "active"}])
            delete_parent, _ = self._sync(gid, [{"op": "task.delete", "task_id": parent_id}])
            self.assertFalse(delete_parent.ok)
            self.assertIn("subtree", str(delete_parent.error.message).lower())
        finally:
            cleanup()

    def test_task_delete_clears_pointing_agent_state_for_deleted_subtree(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._sync(gid, [{"op": "task.create", "title": "Parent", "outcome": "Scratch"}])
            parent_id = self._tasks(gid)[0].result["tasks"][0]["id"]
            self._sync(gid, [{"op": "task.create", "title": "Child", "outcome": "Scratch", "parent_id": parent_id}])
            tasks = self._tasks(gid)[0].result["tasks"]
            child_id = next(task["id"] for task in tasks if task["title"] == "Child")

            self._sync(
                gid,
                [
                    {
                        "op": "agent_state.update",
                        "actor_id": "peer1",
                        "active_task_id": child_id,
                        "focus": "Child",
                        "what_changed": f"{child_id} -> planned",
                    }
                ],
                by="peer1",
            )

            delete_resp, _ = self._sync(gid, [{"op": "task.delete", "task_id": parent_id}])
            self.assertTrue(delete_resp.ok, getattr(delete_resp, "error", None))

            ctx, _ = self._context(gid)
            state = [item for item in ctx.result["agent_states"] if item["id"] == "peer1"][0]
            self.assertIsNone(state["hot"]["active_task_id"])
            self.assertEqual(state["hot"]["focus"], "")
            self.assertEqual(state["warm"]["what_changed"], "")
        finally:
            cleanup()

    def test_task_move_autosyncs_assignee_agent_state(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._sync(
                gid,
                [{"op": "task.create", "title": "Auth", "outcome": "Ship auth", "assignee": "foreman"}],
            )
            task_id = self._tasks(gid)[0].result["tasks"][0]["id"]
            self._sync(gid, [{"op": "task.move", "task_id": task_id, "status": "active"}])
            ctx, _ = self._context(gid)
            state = [item for item in ctx.result["agent_states"] if item["id"] == "foreman"][0]
            self.assertEqual(state["hot"]["active_task_id"], task_id)
            self.assertEqual(state["hot"]["focus"], "Auth")
            self.assertEqual(state["warm"]["what_changed"], f"{task_id} -> active")

            self._sync(gid, [{"op": "task.move", "task_id": task_id, "status": "done"}])
            ctx2, _ = self._context(gid)
            state2 = [item for item in ctx2.result["agent_states"] if item["id"] == "foreman"][0]
            self.assertIsNone(state2["hot"]["active_task_id"])
            self.assertEqual(state2["warm"]["what_changed"], f"{task_id} -> done")
        finally:
            cleanup()

    def test_context_get_agent_states_follow_group_actor_order(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group

            gid = self._create_group()
            group = load_group(gid)
            assert group is not None

            add_actor(group, actor_id="PeerA", runtime="codex")
            add_actor(group, actor_id="管理员", runtime="codex")
            add_actor(group, actor_id="alpha", runtime="codex")

            self._sync(gid, [{"op": "agent_state.update", "actor_id": "alpha", "focus": "alpha work"}], by="alpha")
            self._sync(gid, [{"op": "agent_state.update", "actor_id": "PeerA", "focus": "peer work"}], by="PeerA")
            self._sync(gid, [{"op": "agent_state.update", "actor_id": "管理员", "focus": "manager work"}], by="管理员")

            ctx, _ = self._context(gid)
            agent_ids = [str(item.get("id") or "") for item in ctx.result["agent_states"]]
            self.assertEqual(agent_ids, ["PeerA", "管理员", "alpha"])
        finally:
            cleanup()

    def test_agent_clear(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            resp, _ = self._sync(
                gid,
                [{
                    "op": "agent_state.update",
                    "actor_id": "peer1",
                    "focus": "coding",
                    "blockers": ["dep waiting"],
                    "next_action": "write tests",
                    "what_changed": "context reset pending",
                    "resume_hint": "open failing test first",
                }],
                by="peer1",
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            clear_resp, _ = self._sync(gid, [{"op": "agent_state.clear", "actor_id": "peer1"}], by="peer1")
            self.assertTrue(clear_resp.ok, getattr(clear_resp, "error", None))
            ctx, _ = self._context(gid)
            state = [item for item in ctx.result["agent_states"] if item["id"] == "peer1"][0]
            self.assertEqual(state["hot"], {"active_task_id": None, "focus": "", "next_action": "", "blockers": []})
            self.assertEqual(state["warm"], {
                "what_changed": "",
                "open_loops": [],
                "commitments": [],
                "environment_summary": "",
                "user_model": "",
                "persona_notes": "",
                "resume_hint": "",
            })
        finally:
            cleanup()

    def test_agent_state_update_tracks_mind_context_touch_and_hot_only_churn(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group

            gid = self._create_group()
            initial_resp, _ = self._sync(
                gid,
                [{
                    "op": "agent_state.update",
                    "actor_id": "peer1",
                    "focus": "triage the bug",
                    "next_action": "capture evidence",
                    "what_changed": "picked up the bug pass",
                    "environment_summary": "workspace has one active bugfix branch",
                    "user_model": "prefers concrete evidence",
                    "persona_notes": "avoid speculative fixes",
                }],
                by="peer1",
            )
            self.assertTrue(initial_resp.ok, getattr(initial_resp, "error", None))
            first_ctx, _ = self._context(gid)
            first_state = [item for item in first_ctx.result["agent_states"] if item["id"] == "peer1"][0]
            first_updated_at = str(first_state.get("updated_at") or "")
            self.assertTrue(first_updated_at)

            second_resp, _ = self._sync(
                gid,
                [{
                    "op": "agent_state.update",
                    "actor_id": "peer1",
                    "focus": "reproduce the bug precisely",
                    "next_action": "inspect current logs",
                    "what_changed": "refined the active debug step",
                }],
                by="peer1",
            )
            self.assertTrue(second_resp.ok, getattr(second_resp, "error", None))

            group = load_group(gid)
            assert group is not None
            automation_state = json.loads((group.path / "state" / "automation.json").read_text(encoding="utf-8"))
            actor_state = automation_state["actors"]["peer1"]

            self.assertEqual(str(actor_state.get("mind_context_touched_at") or ""), first_updated_at)
            self.assertEqual(int(actor_state.get("hot_only_updates_since_mind_touch") or 0), 1)
            self.assertTrue(str(actor_state.get("mind_context_hash") or "").strip())
        finally:
            cleanup()

    def test_agent_state_clear_resets_mind_context_touch_runtime_state(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group

            gid = self._create_group()
            seed_resp, _ = self._sync(
                gid,
                [{
                    "op": "agent_state.update",
                    "actor_id": "peer1",
                    "focus": "prepare handoff",
                    "next_action": "summarize current state",
                    "what_changed": "handoff prep started",
                    "environment_summary": "repo has one scoped patch",
                    "user_model": "wants direct closure",
                    "persona_notes": "keep the handoff crisp",
                }],
                by="peer1",
            )
            self.assertTrue(seed_resp.ok, getattr(seed_resp, "error", None))

            clear_resp, _ = self._sync(gid, [{"op": "agent_state.clear", "actor_id": "peer1"}], by="peer1")
            self.assertTrue(clear_resp.ok, getattr(clear_resp, "error", None))

            group = load_group(gid)
            assert group is not None
            automation_state = json.loads((group.path / "state" / "automation.json").read_text(encoding="utf-8"))
            actor_state = automation_state["actors"]["peer1"]

            self.assertEqual(str(actor_state.get("mind_context_hash") or ""), "")
            self.assertEqual(str(actor_state.get("mind_context_touched_at") or ""), "")
            self.assertEqual(int(actor_state.get("hot_only_updates_since_mind_touch") or 0), 0)
            self.assertTrue(str(actor_state.get("agent_state_last_seen_updated_at") or "").strip())
        finally:
            cleanup()

    def test_foreman_cannot_mutate_peer_agent_state(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group

            gid = self._create_group()
            group = load_group(gid)
            assert group is not None
            add_actor(group, actor_id="lead1", runtime="codex")
            add_actor(group, actor_id="peer1", runtime="codex")

            update_resp, _ = self._sync(
                gid,
                [{"op": "agent_state.update", "actor_id": "peer1", "focus": "take over peer state"}],
                by="lead1",
            )
            self.assertFalse(update_resp.ok)
            self.assertIn("permission denied", str(update_resp.error.message).lower())

            clear_resp, _ = self._sync(
                gid,
                [{"op": "agent_state.clear", "actor_id": "peer1"}],
                by="lead1",
            )
            self.assertFalse(clear_resp.ok)
            self.assertIn("permission denied", str(clear_resp.error.message).lower())
        finally:
            cleanup()

    def test_peer_cannot_update_coordination_brief(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            resp, _ = self._sync(gid, [{"op": "coordination.brief.update", "objective": "hijacked"}], by="peer1")
            self.assertFalse(resp.ok)
            self.assertIn("require foreman or user", str(resp.error.message).lower())
        finally:
            cleanup()

    def test_peer_can_create_self_task_but_not_reassign_other_actor(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            ok_resp, _ = self._sync(
                gid,
                [{"op": "task.create", "title": "Peer task", "outcome": "Own it", "assignee": "peer1"}],
                by="peer1",
            )
            self.assertTrue(ok_resp.ok, getattr(ok_resp, "error", None))
            bad_resp, _ = self._sync(
                gid,
                [{"op": "task.create", "title": "Bad task", "outcome": "Nope", "assignee": "foreman"}],
                by="peer1",
            )
            self.assertFalse(bad_resp.ok)
            self.assertIn("assigned to foreman", str(bad_resp.error.message).lower())
        finally:
            cleanup()

    def test_peer_cannot_mutate_unassigned_task_but_can_restore_own_archived_task(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group

            gid = self._create_group()
            group = load_group(gid)
            assert group is not None
            add_actor(group, actor_id="lead1", runtime="codex")
            add_actor(group, actor_id="peer1", runtime="codex")

            self._sync(gid, [{"op": "task.create", "title": "Unassigned", "outcome": "Guard it"}])
            unassigned_id = self._tasks(gid)[0].result["tasks"][0]["id"]

            update_resp, _ = self._sync(
                gid,
                [{"op": "task.update", "task_id": unassigned_id, "notes": "peer should not edit this"}],
                by="peer1",
            )
            self.assertFalse(update_resp.ok)
            self.assertIn("permission denied", str(update_resp.error.message).lower())

            move_resp, _ = self._sync(
                gid,
                [{"op": "task.move", "task_id": unassigned_id, "status": "active"}],
                by="peer1",
            )
            self.assertFalse(move_resp.ok)
            self.assertIn("permission denied", str(move_resp.error.message).lower())

            self._sync(gid, [{"op": "task.move", "task_id": unassigned_id, "status": "archived"}])
            restore_denied, _ = self._sync(
                gid,
                [{"op": "task.restore", "task_id": unassigned_id}],
                by="peer1",
            )
            self.assertFalse(restore_denied.ok)
            self.assertIn("permission denied", str(restore_denied.error.message).lower())

            self._sync(
                gid,
                [{"op": "task.create", "title": "Assigned", "outcome": "Peer can restore", "assignee": "peer1"}],
            )
            assigned_id = [
                item["id"]
                for item in self._tasks(gid)[0].result["tasks"]
                if item["title"] == "Assigned"
            ][0]

            archive_resp, _ = self._sync(
                gid,
                [{"op": "task.move", "task_id": assigned_id, "status": "archived"}],
                by="peer1",
            )
            self.assertTrue(archive_resp.ok, getattr(archive_resp, "error", None))

            restore_ok, _ = self._sync(
                gid,
                [{"op": "task.restore", "task_id": assigned_id}],
                by="peer1",
            )
            self.assertTrue(restore_ok.ok, getattr(restore_ok, "error", None))
            assigned_task = [
                item
                for item in self._tasks(gid)[0].result["tasks"]
                if item["id"] == assigned_id
            ][0]
            self.assertEqual(assigned_task["status"], "planned")
            self.assertIsNone(assigned_task.get("archived_from"))
        finally:
            cleanup()

    def test_legacy_role_notes_set_updates_help_actor_block_without_touching_persona_notes(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group
            from cccc.kernel.prompt_files import HELP_FILENAME, read_group_prompt_file

            gid = self._create_group()
            group = load_group(gid)
            assert group is not None
            add_actor(group, actor_id="lead1", runtime="codex")
            add_actor(group, actor_id="peer1", runtime="codex")
            seed_resp, _ = self._sync(
                gid,
                [{"op": "agent_state.update", "actor_id": "peer1", "focus": "seed"}],
                by="peer1",
            )
            self.assertTrue(seed_resp.ok, getattr(seed_resp, "error", None))

            resp, _ = self._sync(
                gid,
                [{
                    "op": "role_notes.set",
                    "actor_id": "peer1",
                    "persona_notes": "Stay skeptical.\nUse receipts.",
                }],
                by="user",
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))

            prompt_file = read_group_prompt_file(group, HELP_FILENAME)
            self.assertTrue(prompt_file.found)
            help_content = str(prompt_file.content or "")
            self.assertIn("## @actor: peer1", help_content)
            self.assertIn("Stay skeptical.\nUse receipts.", help_content)

            ctx, _ = self._context(gid)
            peer_state = [item for item in ctx.result["agent_states"] if item["id"] == "peer1"][0]
            self.assertEqual(peer_state["warm"]["persona_notes"], "")
        finally:
            cleanup()

    def test_task_list_single_includes_children(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._sync(gid, [{"op": "task.create", "title": "Root", "outcome": "root"}])
            root_id = self._tasks(gid)[0].result["tasks"][0]["id"]
            self._sync(gid, [{"op": "task.create", "title": "Child", "outcome": "child", "parent_id": root_id}])
            task_resp, _ = self._call("task_list", {"group_id": gid, "task_id": root_id})
            self.assertTrue(task_resp.ok)
            self.assertEqual(len(task_resp.result["task"]["children"]), 1)
            self.assertEqual(task_resp.result["task"]["children"][0]["title"], "Child")
        finally:
            cleanup()

    def test_legacy_presence_ops_rejected(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            resp, _ = self._sync(gid, [{"op": "agent.update", "actor_id": "peer1", "focus": "old"}])
            self.assertFalse(resp.ok)
            self.assertIn("unknown operation", str(resp.error.message).lower())
        finally:
            cleanup()

    def test_meta_merge_project_status_and_peer_permission(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            bad_type, _ = self._sync(gid, [{"op": "meta.merge", "data": {"project_status": 123}}])
            self.assertFalse(bad_type.ok)
            self.assertIn("string or null", str(bad_type.error.message).lower())

            too_long, _ = self._sync(gid, [{"op": "meta.merge", "data": {"project_status": "x" * 101}}])
            self.assertFalse(too_long.ok)
            self.assertIn("exceeds 100", str(too_long.error.message).lower())

            peer_denied, _ = self._sync(gid, [{"op": "meta.merge", "data": {"project_status": "ok"}}], by="peer1")
            self.assertFalse(peer_denied.ok)
            self.assertIn("requires foreman or user", str(peer_denied.error.message).lower())

            ok_resp, _ = self._sync(gid, [{"op": "meta.merge", "data": {"project_status": None}}])
            self.assertTrue(ok_resp.ok, getattr(ok_resp, "error", None))
        finally:
            cleanup()

if __name__ == "__main__":
    unittest.main()
