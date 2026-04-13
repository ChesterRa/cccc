import os
import tempfile
import threading
import time
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebContextRoutesCache(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        old_mode = os.environ.get("CCCC_WEB_MODE")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            if old_mode is None:
                os.environ.pop("CCCC_WEB_MODE", None)
            else:
                os.environ["CCCC_WEB_MODE"] = old_mode

        return td, cleanup

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        return create_group(reg, title="context-cache-test", topic="").group_id

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_summary_context_get_reads_local_snapshot_without_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            summary_payload = {"coordination": {"tasks": []}, "meta": {"summary_snapshot": {"state": "hit"}}}

            with patch(
                "cccc.ports.web.routes.groups._read_context_summary_local",
                return_value={"ok": True, "result": summary_payload},
            ) as mock_local, patch(
                "cccc.ports.web.app.call_daemon",
                side_effect=AssertionError("summary context should not call daemon"),
            ):
                with self._client() as client:
                    resp = client.get(f"/api/v1/groups/{group_id}/context")

            self.assertEqual(resp.status_code, 200)
            self.assertEqual(resp.json()["result"]["meta"]["summary_snapshot"]["state"], "hit")
            mock_local.assert_called_once_with(group_id)
        finally:
            cleanup()

    def test_normal_mode_full_context_get_uses_inflight_without_ttl(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            barrier = threading.Barrier(3)

            def fake_call_daemon(req: dict):
                nonlocal context_get_calls
                if str(req.get("op") or "") == "context_get":
                    with call_lock:
                        context_get_calls += 1
                    time.sleep(0.12)
                    return {"ok": True, "result": {"coordination": {"tasks": []}, "agent_states": [], "meta": {}}}
                return {"ok": True, "result": {}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    path = f"/api/v1/groups/{group_id}/context?detail=full"

                    def do_get() -> int:
                        barrier.wait(timeout=2)
                        resp = client.get(path)
                        return resp.status_code

                    with ThreadPoolExecutor(max_workers=2) as executor:
                        fut1 = executor.submit(do_get)
                        fut2 = executor.submit(do_get)
                        barrier.wait(timeout=2)
                        self.assertEqual(fut1.result(timeout=3), 200)
                        self.assertEqual(fut2.result(timeout=3), 200)

                    self.assertEqual(context_get_calls, 1)

                    follow_up = client.get(path)
                    self.assertEqual(follow_up.status_code, 200)
                    self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()

    def test_context_sync_invalidates_server_inflight(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            first_read_release = threading.Event()

            def fake_call_daemon(req: dict):
                nonlocal context_get_calls
                op = str(req.get("op") or "")
                if op == "context_get":
                    with call_lock:
                        context_get_calls += 1
                        current = context_get_calls
                    if current == 1:
                        first_read_release.wait(timeout=2)
                        return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "stale"}}}
                    return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "fresh"}}}
                if op == "context_sync":
                    return {"ok": True, "result": {"version": "fresh"}}
                return {"ok": True, "result": {}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    path = f"/api/v1/groups/{group_id}/context?detail=full"

                    with ThreadPoolExecutor(max_workers=1) as executor:
                        stale_future = executor.submit(client.get, path)
                        time.sleep(0.05)

                        sync_resp = client.post(path, json={"ops": [{"op": "coordination.brief.update", "current_focus": "fresh"}], "by": "user"})
                        self.assertEqual(sync_resp.status_code, 200)

                        fresh_resp = client.get(path)
                        self.assertEqual(fresh_resp.status_code, 200)
                        self.assertEqual(fresh_resp.json()["result"]["meta"]["version"], "fresh")

                        first_read_release.set()
                        stale_resp = stale_future.result(timeout=3)
                        self.assertEqual(stale_resp.status_code, 200)
                        self.assertEqual(stale_resp.json()["result"]["meta"]["version"], "stale")

                    self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()

    def test_project_md_put_invalidates_server_inflight(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            first_read_release = threading.Event()

            from cccc.kernel.group import attach_scope_to_group, load_group
            from cccc.kernel.registry import load_registry
            from cccc.kernel.scope import detect_scope

            with tempfile.TemporaryDirectory() as scope_root:
                reg = load_registry()
                group = load_group(group_id)
                assert group is not None
                attach_scope_to_group(reg, group, detect_scope(Path(scope_root)))

                def fake_call_daemon(req: dict):
                    nonlocal context_get_calls
                    op = str(req.get("op") or "")
                    if op == "context_get":
                        with call_lock:
                            context_get_calls += 1
                            current = context_get_calls
                        if current == 1:
                            first_read_release.wait(timeout=2)
                            return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "stale"}}}
                        return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "fresh"}}}
                    if op == "context_sync":
                        return {"ok": True, "result": {"version": "fresh"}}
                    return {"ok": True, "result": {}}

                with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                    with self._client() as client:
                        path = f"/api/v1/groups/{group_id}/context?detail=full"

                        with ThreadPoolExecutor(max_workers=1) as executor:
                            stale_future = executor.submit(client.get, path)
                            time.sleep(0.05)

                            put_resp = client.put(
                                f"/api/v1/groups/{group_id}/project_md",
                                json={"content": "# Fresh PROJECT"},
                            )
                            self.assertEqual(put_resp.status_code, 200)
                            self.assertEqual(put_resp.json()["result"]["content"], "# Fresh PROJECT")

                            fresh_resp = client.get(path)
                            self.assertEqual(fresh_resp.status_code, 200)
                            self.assertEqual(fresh_resp.json()["result"]["meta"]["version"], "fresh")

                            first_read_release.set()
                            stale_resp = stale_future.result(timeout=3)
                            self.assertEqual(stale_resp.status_code, 200)
                            self.assertEqual(stale_resp.json()["result"]["meta"]["version"], "stale")

                        self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()

    def test_learning_route_reads_learning_snapshot_from_separate_domain(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.create", "title": "Phase 1", "outcome": "stabilize recall gate"}],
                },
            )
            task_list_resp, _ = self._call("task_list", {"group_id": group_id})
            tasks = (task_list_resp.result or {}).get("tasks") if isinstance(task_list_resp.result, dict) else []
            task_id = str((tasks[0] if tasks else {}).get("id") or "")
            self.assertTrue(task_id)
            self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.move", "task_id": task_id, "status": "done"}],
                },
            )

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            assert group is not None
            candidates_doc = read_json(Path(group.path) / "state" / "experience_candidates.json")
            candidates = candidates_doc.get("candidates") if isinstance(candidates_doc.get("candidates"), list) else []
            candidate_id = str((candidates[0] if candidates else {}).get("id") or "")
            self.assertTrue(candidate_id)

            self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            report_resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-learning-001",
                    "evidence_type": "missing_constraint",
                    "evidence_payload": {"constraint": "verify recall gate"},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "adjust_constraint",
                    "reason": "runtime drifted without guardrail",
                    "proposed_delta": {"constraint": "Verify recall gate before acting on recalled memory."},
                },
            )
            patch_candidate = (
                report_resp.result.get("patch_candidate")
                if isinstance(report_resp.result, dict) and isinstance(report_resp.result.get("patch_candidate"), dict)
                else {}
            )
            patch_candidate_id = str(patch_candidate.get("candidate_id") or "")
            self.assertTrue(patch_candidate_id)
            self._call(
                "procedural_skill_govern_patch",
                {
                    "group_id": group_id,
                    "candidate_id": patch_candidate_id,
                    "lifecycle_action": "merge",
                    "reason": "merge tested correction",
                    "by": "user",
                },
            )

            with self._client() as client:
                resp = client.get(f"/api/v1/groups/{group_id}/learning")

            self.assertEqual(resp.status_code, 200)
            payload = resp.json().get("result") or {}
            overview = payload.get("overview") if isinstance(payload.get("overview"), dict) else {}
            self.assertEqual(int(overview.get("active_skill_count") or 0), 1)
            self.assertEqual(int(overview.get("merged_patch_count_7d") or 0), 1)
            pending = payload.get("pending_patches") if isinstance(payload.get("pending_patches"), list) else []
            self.assertEqual(pending, [])
            recent = payload.get("recent_learning") if isinstance(payload.get("recent_learning"), list) else []
            self.assertEqual(len(recent), 1)
            self.assertEqual(str(recent[0].get("candidate_id") or ""), patch_candidate_id)
            self.assertEqual(str(recent[0].get("post_merge_status") or ""), "observing")
            self.assertEqual(str(recent[0].get("stability") or ""), "probation")
            self.assertEqual(str(recent[0].get("patch_review_mode") or ""), "auto_merge_eligible")
            funnel = payload.get("funnel") if isinstance(payload.get("funnel"), dict) else {}
            self.assertEqual(int(funnel.get("candidate_created_count") or 0), 1)
            self.assertEqual(int(funnel.get("candidate_ready_count") or 0), 0)
            self.assertEqual(int(funnel.get("pending_review_count") or 0), 0)
        finally:
            cleanup()

    def test_learning_route_exposes_governance_and_regression_lineage(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.create", "title": "Phase 1", "outcome": "stabilize recall gate"}],
                },
            )
            task_list_resp, _ = self._call("task_list", {"group_id": group_id})
            tasks = (task_list_resp.result or {}).get("tasks") if isinstance(task_list_resp.result, dict) else []
            task_id = str((tasks[0] if tasks else {}).get("id") or "")
            self.assertTrue(task_id)
            self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.move", "task_id": task_id, "status": "done"}],
                },
            )

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            assert group is not None
            candidates_doc = read_json(Path(group.path) / "state" / "experience_candidates.json")
            candidates = candidates_doc.get("candidates") if isinstance(candidates_doc.get("candidates"), list) else []
            candidate_id = str((candidates[0] if candidates else {}).get("id") or "")
            self.assertTrue(candidate_id)

            self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            report_resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-learning-201",
                    "evidence_type": "missing_constraint",
                    "evidence_payload": {"constraint": "verify recall gate"},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "adjust_constraint",
                    "reason": "runtime drifted without guardrail",
                    "proposed_delta": {"constraint": "Verify recall gate before acting on recalled memory."},
                },
            )
            patch_candidate = (
                report_resp.result.get("patch_candidate")
                if isinstance(report_resp.result, dict) and isinstance(report_resp.result.get("patch_candidate"), dict)
                else {}
            )
            patch_candidate_id = str(patch_candidate.get("candidate_id") or "")
            self.assertTrue(patch_candidate_id)
            self._call(
                "procedural_skill_govern_patch",
                {
                    "group_id": group_id,
                    "candidate_id": patch_candidate_id,
                    "lifecycle_action": "merge",
                    "reason": "merge tested correction",
                    "by": "user",
                },
            )
            regress_resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-learning-202",
                    "evidence_type": "failure_signal_triggered",
                    "evidence_payload": {"failure_signal": "Recall gate was skipped again."},
                    "outcome": "regressed",
                    "generate_patch": True,
                    "patch_kind": "clarify_failure_signal",
                    "reason": "merged patch still missed runtime failure signal",
                    "proposed_delta": {"failure_signal": "Recall gate skipped again after patch merge."},
                },
            )
            followup_candidate = (
                regress_resp.result.get("patch_candidate")
                if isinstance(regress_resp.result, dict) and isinstance(regress_resp.result.get("patch_candidate"), dict)
                else {}
            )
            followup_candidate_id = str(followup_candidate.get("candidate_id") or "")
            self.assertTrue(followup_candidate_id)

            with self._client() as client:
                resp = client.get(f"/api/v1/groups/{group_id}/learning")

            self.assertEqual(resp.status_code, 200)
            payload = resp.json().get("result") or {}
            funnel = payload.get("funnel") if isinstance(payload.get("funnel"), dict) else {}
            self.assertEqual(int(funnel.get("candidate_created_count") or 0), 2)
            self.assertEqual(int(funnel.get("candidate_ready_count") or 0), 1)
            self.assertEqual(int(funnel.get("pending_review_count") or 0), 1)

            pending = payload.get("pending_patches") if isinstance(payload.get("pending_patches"), list) else []
            self.assertEqual(len(pending), 1)
            self.assertEqual(str(pending[0].get("candidate_id") or ""), followup_candidate_id)
            self.assertEqual(str(pending[0].get("review_mode") or ""), "manual_review_required")
            self.assertEqual(str(pending[0].get("regressed_from_candidate_id") or ""), patch_candidate_id)

            recent = payload.get("recent_learning") if isinstance(payload.get("recent_learning"), list) else []
            self.assertEqual(len(recent), 1)
            self.assertEqual(str(recent[0].get("post_merge_status") or ""), "regressed")
            self.assertEqual(str(recent[0].get("stability") or ""), "unstable")
            self.assertEqual(str(recent[0].get("patch_review_mode") or ""), "manual_review_required")
            self.assertEqual(str(recent[0].get("followup_candidate_id") or ""), followup_candidate_id)
            self.assertEqual(str(recent[0].get("regressed_from_candidate_id") or ""), patch_candidate_id)

            observing = payload.get("observing_skills") if isinstance(payload.get("observing_skills"), list) else []
            self.assertEqual(len(observing), 1)
            self.assertEqual(str(observing[0].get("status") or ""), "regressed")
            self.assertEqual(str(observing[0].get("stability") or ""), "unstable")
            self.assertEqual(str(observing[0].get("patch_review_mode") or ""), "manual_review_required")
            self.assertEqual(str(observing[0].get("followup_candidate_id") or ""), followup_candidate_id)
            self.assertEqual(str(observing[0].get("followup_review_mode") or ""), "manual_review_required")
            self.assertEqual(str(observing[0].get("regressed_from_candidate_id") or ""), patch_candidate_id)
        finally:
            cleanup()

    def test_learning_route_only_returns_latest_merged_patch_per_skill(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.create", "title": "Phase 1", "outcome": "stabilize recall gate"}],
                },
            )
            task_list_resp, _ = self._call("task_list", {"group_id": group_id})
            tasks = (task_list_resp.result or {}).get("tasks") if isinstance(task_list_resp.result, dict) else []
            task_id = str((tasks[0] if tasks else {}).get("id") or "")
            self.assertTrue(task_id)
            self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.move", "task_id": task_id, "status": "done"}],
                },
            )

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            assert group is not None
            candidates_doc = read_json(Path(group.path) / "state" / "experience_candidates.json")
            candidates = candidates_doc.get("candidates") if isinstance(candidates_doc.get("candidates"), list) else []
            candidate_id = str((candidates[0] if candidates else {}).get("id") or "")
            self.assertTrue(candidate_id)

            self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )

            first_report, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-learning-301",
                    "evidence_type": "missing_constraint",
                    "evidence_payload": {"constraint": "verify recall gate"},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "adjust_constraint",
                    "reason": "missing recall guardrail",
                    "proposed_delta": {"constraint": "Verify recall gate before acting on recalled memory."},
                },
            )
            first_candidate = (
                first_report.result.get("patch_candidate")
                if isinstance(first_report.result, dict) and isinstance(first_report.result.get("patch_candidate"), dict)
                else {}
            )
            first_candidate_id = str(first_candidate.get("candidate_id") or "")
            self.assertTrue(first_candidate_id)
            self._call(
                "procedural_skill_govern_patch",
                {
                    "group_id": group_id,
                    "candidate_id": first_candidate_id,
                    "lifecycle_action": "merge",
                    "reason": "merge first correction",
                    "by": "user",
                },
            )
            self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-learning-302",
                    "evidence_type": "note_only",
                    "outcome": "success",
                    "generate_patch": False,
                },
            )

            second_report, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-learning-303",
                    "evidence_type": "failure_signal_triggered",
                    "evidence_payload": {"failure_signal": "Recall gate skipped again."},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "clarify_failure_signal",
                    "reason": "need explicit regression signal",
                    "proposed_delta": {"failure_signal": "Recall gate skipped again after patch merge."},
                },
            )
            second_candidate = (
                second_report.result.get("patch_candidate")
                if isinstance(second_report.result, dict) and isinstance(second_report.result.get("patch_candidate"), dict)
                else {}
            )
            second_candidate_id = str(second_candidate.get("candidate_id") or "")
            self.assertTrue(second_candidate_id)
            self._call(
                "procedural_skill_govern_patch",
                {
                    "group_id": group_id,
                    "candidate_id": second_candidate_id,
                    "lifecycle_action": "merge",
                    "reason": "merge second correction",
                    "by": "user",
                },
            )

            with self._client() as client:
                resp = client.get(f"/api/v1/groups/{group_id}/learning")

            self.assertEqual(resp.status_code, 200)
            payload = resp.json().get("result") or {}
            recent = payload.get("recent_learning") if isinstance(payload.get("recent_learning"), list) else []
            self.assertEqual(len(recent), 1)
            self.assertEqual(str(recent[0].get("candidate_id") or ""), second_candidate_id)
            self.assertNotEqual(str(recent[0].get("candidate_id") or ""), first_candidate_id)
        finally:
            cleanup()

    def test_learning_route_exposes_procedural_skill_list(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()

            with self._client() as client:
                create_resp = client.post(
                    f"/api/v1/groups/{group_id}/learning/skills",
                    json={
                        "skill_id": "manual_peer_policy",
                        "title": "创建 cccc peer",
                        "goal": "新增执行面时默认创建 cccc peer。",
                        "steps": ["创建 cccc peer", "优先并行分配给其他 peer"],
                        "constraints": ["非重要事情不 @all"],
                        "failure_signals": ["错误创建成 worker"],
                        "status": "active",
                        "stability": "stable",
                        "review_mode": "auto_merge_eligible",
                    },
                )
                self.assertEqual(create_resp.status_code, 200)

                resp = client.get(f"/api/v1/groups/{group_id}/learning")

            self.assertEqual(resp.status_code, 200)
            payload = resp.json().get("result") or {}
            skills = payload.get("skills") if isinstance(payload.get("skills"), list) else []
            self.assertEqual(len(skills), 1)
            self.assertEqual(str(skills[0].get("skill_id") or ""), "manual_peer_policy")
            self.assertEqual(str(skills[0].get("status") or ""), "active")
            self.assertEqual(skills[0].get("steps") or [], ["创建 cccc peer", "优先并行分配给其他 peer"])
            overview = payload.get("overview") if isinstance(payload.get("overview"), dict) else {}
            self.assertEqual(int(overview.get("active_skill_count") or 0), 1)
        finally:
            cleanup()

    def test_learning_skill_crud_updates_consumption_visibility(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()

            with self._client() as client:
                create_resp = client.post(
                    f"/api/v1/groups/{group_id}/learning/skills",
                    json={
                        "skill_id": "manual_peer_policy",
                        "title": "创建 cccc peer",
                        "goal": "新增执行面时默认创建 cccc peer。",
                        "steps": ["创建 cccc peer"],
                    },
                )
                self.assertEqual(create_resp.status_code, 200)

                update_resp = client.put(
                    f"/api/v1/groups/{group_id}/learning/skills/manual_peer_policy",
                    json={
                        "title": "创建 cccc peer",
                        "goal": "新增执行面时默认创建 cccc peer，并行分配任务。",
                        "steps": ["创建 cccc peer", "并行分配其他 peer"],
                        "constraints": ["非重要事情不 @all"],
                        "failure_signals": ["错误创建成 worker"],
                        "status": "disabled",
                        "stability": "probation",
                        "review_mode": "manual_review_required",
                    },
                )
                self.assertEqual(update_resp.status_code, 200)

                read_resp = client.get(f"/api/v1/groups/{group_id}/learning")
                self.assertEqual(read_resp.status_code, 200)
                payload = read_resp.json().get("result") or {}
                skills = payload.get("skills") if isinstance(payload.get("skills"), list) else []
                self.assertEqual(len(skills), 1)
                self.assertEqual(str(skills[0].get("status") or ""), "disabled")
                self.assertEqual(str(skills[0].get("review_mode") or ""), "manual_review_required")
                overview = payload.get("overview") if isinstance(payload.get("overview"), dict) else {}
                self.assertEqual(int(overview.get("active_skill_count") or 0), 0)

                from cccc.kernel.group import load_group
                from cccc.kernel.procedural_skills import select_procedural_skills_for_consumption

                group = load_group(group_id)
                assert group is not None
                consumed = select_procedural_skills_for_consumption(group, limit=3)
                self.assertEqual(consumed, [])

                delete_resp = client.delete(f"/api/v1/groups/{group_id}/learning/skills/manual_peer_policy")
                self.assertEqual(delete_resp.status_code, 200)

                final_resp = client.get(f"/api/v1/groups/{group_id}/learning")
                self.assertEqual(final_resp.status_code, 200)
                final_payload = final_resp.json().get("result") or {}
                final_skills = final_payload.get("skills") if isinstance(final_payload.get("skills"), list) else []
                self.assertEqual(final_skills, [])
        finally:
            cleanup()

    def test_fresh_context_get_bypasses_stale_server_inflight(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            first_read_release = threading.Event()

            def fake_call_daemon(req: dict):
                nonlocal context_get_calls
                op = str(req.get("op") or "")
                if op == "context_get":
                    with call_lock:
                        context_get_calls += 1
                        current = context_get_calls
                    if current == 1:
                        first_read_release.wait(timeout=2)
                        return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "stale"}}}
                    return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "fresh"}}}
                return {"ok": True, "result": {}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    path = f"/api/v1/groups/{group_id}/context?detail=full"

                    with ThreadPoolExecutor(max_workers=1) as executor:
                        stale_future = executor.submit(client.get, path)
                        time.sleep(0.05)

                        fresh_resp = client.get(f"{path}&fresh=1")
                        self.assertEqual(fresh_resp.status_code, 200)
                        self.assertEqual(fresh_resp.json()["result"]["meta"]["version"], "fresh")

                        first_read_release.set()
                        stale_resp = stale_future.result(timeout=3)
                        self.assertEqual(stale_resp.status_code, 200)
                        self.assertEqual(stale_resp.json()["result"]["meta"]["version"], "stale")

                    self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()

    def test_actor_delete_invalidates_server_inflight_context_reads(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            first_read_release = threading.Event()

            def fake_call_daemon(req: dict):
                nonlocal context_get_calls
                op = str(req.get("op") or "")
                if op == "context_get":
                    with call_lock:
                        context_get_calls += 1
                        current = context_get_calls
                    if current == 1:
                        first_read_release.wait(timeout=2)
                        return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "stale"}}}
                    return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "fresh"}}}
                if op == "actor_remove":
                    return {"ok": True, "result": {"actor_id": "peer-1"}}
                return {"ok": True, "result": {}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    path = f"/api/v1/groups/{group_id}/context?detail=full"

                    with ThreadPoolExecutor(max_workers=1) as executor:
                        stale_future = executor.submit(client.get, path)
                        time.sleep(0.05)

                        delete_resp = client.delete(f"/api/v1/groups/{group_id}/actors/peer-1")
                        self.assertEqual(delete_resp.status_code, 200)

                        fresh_resp = client.get(path)
                        self.assertEqual(fresh_resp.status_code, 200)
                        self.assertEqual(fresh_resp.json()["result"]["meta"]["version"], "fresh")

                        first_read_release.set()
                        stale_resp = stale_future.result(timeout=3)
                        self.assertEqual(stale_resp.status_code, 200)
                        self.assertEqual(stale_resp.json()["result"]["meta"]["version"], "stale")

                    self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()

    def test_template_import_replace_invalidates_server_inflight_context_reads(self) -> None:
        _, cleanup = self._with_home()
        try:
            os.environ.pop("CCCC_WEB_MODE", None)
            group_id = self._create_group()
            context_get_calls = 0
            call_lock = threading.Lock()
            first_read_release = threading.Event()

            def fake_call_daemon(req: dict):
                nonlocal context_get_calls
                op = str(req.get("op") or "")
                if op == "context_get":
                    with call_lock:
                        context_get_calls += 1
                        current = context_get_calls
                    if current == 1:
                        first_read_release.wait(timeout=2)
                        return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "stale"}}}
                    return {"ok": True, "result": {"coordination": {"tasks": []}, "meta": {"version": "fresh"}}}
                if op == "group_template_import_replace":
                    return {"ok": True, "result": {"group_id": group_id, "applied": True}}
                return {"ok": True, "result": {}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    path = f"/api/v1/groups/{group_id}/context?detail=full"

                    with ThreadPoolExecutor(max_workers=1) as executor:
                        stale_future = executor.submit(client.get, path)
                        time.sleep(0.05)

                        import_resp = client.post(
                            f"/api/v1/groups/{group_id}/template/import_replace",
                            data={"confirm": group_id, "by": "user"},
                            files={"file": ("template.yaml", b"kind: cccc.group_template\nv: 1\nactors: []\nprompts: {}\nautomation:\n  rules: []\n  snippets: {}\n", "text/yaml")},
                        )
                        self.assertEqual(import_resp.status_code, 200)

                        fresh_resp = client.get(path)
                        self.assertEqual(fresh_resp.status_code, 200)
                        self.assertEqual(fresh_resp.json()["result"]["meta"]["version"], "fresh")

                        first_read_release.set()
                        stale_resp = stale_future.result(timeout=3)
                        self.assertEqual(stale_resp.status_code, 200)
                        self.assertEqual(stale_resp.json()["result"]["meta"]["version"], "stale")

                    self.assertEqual(context_get_calls, 2)
        finally:
            cleanup()
