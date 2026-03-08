"""Context v3 focused tests: CAS, coordination brief, task lifecycle, permissions, panorama, meta validation."""

import os
import tempfile
import unittest


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

    def test_panorama_mermaid_includes_coordination_tasks_and_agents(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._sync(gid, [{"op": "coordination.brief.update", "objective": "Ship B3", "current_focus": "Context panel"}])
            self._sync(gid, [{"op": "task.create", "title": "Modal", "outcome": "Redesign modal", "assignee": "foreman"}])
            task_id = self._tasks(gid)[0].result["tasks"][0]["id"]
            self._sync(gid, [{"op": "task.move", "task_id": task_id, "status": "active"}])
            ctx, _ = self._context(gid)
            mermaid = ctx.result["panorama"]["mermaid"]
            self.assertIn("Coordination", mermaid)
            self.assertIn("Ship B3", mermaid)
            self.assertIn("Modal", mermaid)
            self.assertIn("foreman", mermaid)
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
