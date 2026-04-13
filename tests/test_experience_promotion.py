from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock


class TestExperiencePromotion(unittest.TestCase):
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

    def _create_group_with_candidate(self) -> tuple[str, str]:
        create_resp, _ = self._call("group_create", {"title": "experience-promotion", "topic": "", "by": "user"})
        self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
        group_id = str((create_resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)
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
        done_resp, _ = self._call(
            "context_sync",
            {
                "group_id": group_id,
                "by": "user",
                "ops": [{"op": "task.move", "task_id": task_id, "status": "done"}],
            },
        )
        self.assertTrue(done_resp.ok, getattr(done_resp, "error", None))

        from cccc.kernel.group import load_group
        from cccc.util.fs import read_json

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        raw = read_json(Path(group.path) / "state" / "experience_candidates.json")
        candidates = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
        self.assertTrue(candidates)
        candidate_id = str((candidates[0] if isinstance(candidates[0], dict) else {}).get("id") or "")
        self.assertTrue(candidate_id)
        return group_id, candidate_id

    def _create_group_with_two_candidates(self) -> tuple[str, list[str]]:
        create_resp, _ = self._call("group_create", {"title": "experience-governance", "topic": "", "by": "user"})
        self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
        group_id = str((create_resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)
        resp, _ = self._call(
            "context_sync",
            {
                "group_id": group_id,
                "by": "user",
                "ops": [
                    {
                        "op": "coordination.note.add",
                        "kind": "decision",
                        "summary": "Use immutable execution receipts for retry-safe follow-up scheduling.",
                    },
                    {
                        "op": "coordination.note.add",
                        "kind": "decision",
                        "summary": "Keep experience governance in asset layer only, separate from context truth.",
                    },
                ],
            },
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))

        from cccc.kernel.group import load_group
        from cccc.util.fs import read_json

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        raw = read_json(Path(group.path) / "state" / "experience_candidates.json")
        candidates = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
        candidate_ids = [str(item.get("id") or "") for item in candidates if isinstance(item, dict)]
        self.assertGreaterEqual(len(candidate_ids), 2)
        return group_id, candidate_ids[:2]

    def _rewrite_candidate(self, group_id: str, candidate_id: str, mutate):
        from cccc.kernel.group import load_group
        from cccc.util.fs import atomic_write_json, read_json

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        path = Path(group.path) / "state" / "experience_candidates.json"
        raw = read_json(path)
        candidates = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
        updated = []
        for item in candidates:
            if isinstance(item, dict) and str(item.get("id") or "") == candidate_id:
                updated.append(mutate(dict(item)))
            else:
                updated.append(item)
        raw["candidates"] = updated
        atomic_write_json(path, raw, indent=2)

    def _candidate_path(self, group_id: str) -> Path:
        from cccc.kernel.group import load_group

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        return Path(group.path) / "state" / "experience_candidates.json"

    def _memory_path(self, group_id: str) -> Path:
        from cccc.kernel.group import load_group

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        return Path(group.path) / "state" / "memory" / "MEMORY.md"

    def _add_headless_codex_actor(self, group_id: str, *, actor_id: str = "peer1") -> None:
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import load_group

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        add_actor(group, actor_id=actor_id, runtime="codex", runner="headless", enabled=True)
        group.save()

    def _deliver_runtime_prompt(self, group_id: str, candidate_id: str, *, actor_id: str = "peer1") -> tuple[dict, str]:
        deliveries: list[dict] = []

        def _capture_submit_control_message(**kwargs):
            deliveries.append(dict(kwargs))
            return True

        with mock.patch(
            "cccc.daemon.codex_app_sessions.SUPERVISOR.actor_running",
            return_value=True,
        ), mock.patch(
            "cccc.daemon.codex_app_sessions.SUPERVISOR.submit_control_message",
            side_effect=_capture_submit_control_message,
        ):
            resp, _ = self._call(
                "experience_runtime_prompt_delivery",
                {"group_id": group_id, "candidate_id": candidate_id},
            )
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        result = resp.result if isinstance(resp.result, dict) else {}
        runtime_prompt_delivery = (
            result.get("runtime_prompt_delivery")
            if isinstance(result.get("runtime_prompt_delivery"), dict)
            else {}
        )
        self.assertEqual(str(runtime_prompt_delivery.get("status") or ""), "queued")
        self.assertEqual(len(deliveries), 1)
        kwargs = deliveries[0]
        self.assertEqual(str(kwargs.get("group_id") or ""), group_id)
        self.assertEqual(str(kwargs.get("actor_id") or ""), actor_id)
        self.assertEqual(str(kwargs.get("control_kind") or ""), "bootstrap")
        return runtime_prompt_delivery, str(kwargs.get("text") or "")

    def test_daemon_promote_writes_memory_and_updates_candidate(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))
            result = promote_resp.result if isinstance(promote_resp.result, dict) else {}
            candidate = result.get("candidate") if isinstance(result.get("candidate"), dict) else {}
            self.assertEqual(str(candidate.get("status") or ""), "promoted_to_memory")
            self.assertEqual(str(result.get("commit_state") or ""), "disk_committed")
            promotion = candidate.get("promotion") if isinstance(candidate.get("promotion"), dict) else {}
            self.assertEqual(str(promotion.get("by") or ""), "user")
            memory_entry = promotion.get("memory_entry") if isinstance(promotion.get("memory_entry"), dict) else {}
            self.assertTrue(str(memory_entry.get("entry_id") or "").startswith("expmem_"))
            asset_write = result.get("asset_write") if isinstance(result.get("asset_write"), dict) else {}
            self.assertEqual(str(asset_write.get("status") or ""), "written")
            asset_payload = asset_write.get("asset") if isinstance(asset_write.get("asset"), dict) else {}
            self.assertEqual(str(asset_payload.get("candidate_id") or ""), candidate_id)
            self.assertEqual(str(asset_payload.get("memory_entry_id") or ""), str(memory_entry.get("entry_id") or ""))
            skill_write = result.get("skill_write") if isinstance(result.get("skill_write"), dict) else {}
            self.assertEqual(str(skill_write.get("status") or ""), "written")
            skill_payload = skill_write.get("asset") if isinstance(skill_write.get("asset"), dict) else {}
            self.assertEqual(str(skill_payload.get("source_experience_candidate_id") or ""), candidate_id)
            self.assertEqual(str(skill_payload.get("memory_entry_id") or ""), str(memory_entry.get("entry_id") or ""))
            self.assertNotIn("runtime_prompt_delivery", result)

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_text = (Path(group.path) / "state" / "memory" / "MEMORY.md").read_text(encoding="utf-8")
            self.assertIn("experience_record", memory_text)
            self.assertIn(candidate_id, memory_text)
            self.assertIn(str(memory_entry.get("entry_id") or ""), memory_text)
            self.assertIn(f"experience:{candidate_id}", memory_text)
            self.assertIn('"lifecycle_state": "active"', memory_text)
            self.assertEqual(memory_text.count(f"## {memory_entry.get('entry_id')} [experience]"), 1)
            self.assertNotIn("推荐动作", memory_text)
            self.assertNotIn("失败信号", memory_text)
            self.assertNotIn("Root task completed", memory_text)
            asset_path = Path(group.path) / "state" / "experience_assets" / f"{candidate_id}.json"
            self.assertTrue(asset_path.exists())
            asset_doc = read_json(asset_path)
            self.assertEqual(str(asset_doc.get("candidate_id") or ""), candidate_id)
            self.assertEqual(str(asset_doc.get("memory_entry_id") or ""), str(memory_entry.get("entry_id") or ""))
            self.assertEqual(str(asset_doc.get("recommended_action") or ""), "")
            self.assertEqual(asset_doc.get("failure_signals"), [])
            skill_path = Path(group.path) / "state" / "procedural_skills" / f"procskill_{candidate_id}.json"
            self.assertTrue(skill_path.exists())
            skill_doc = read_json(skill_path)
            self.assertEqual(str(skill_doc.get("source_experience_candidate_id") or ""), candidate_id)
            self.assertEqual(str(skill_doc.get("memory_entry_id") or ""), str(memory_entry.get("entry_id") or ""))
            self.assertIn("stabilize recall gate", " ".join(str(x) for x in (skill_doc.get("steps") or [])))

            raw = read_json(Path(group.path) / "state" / "experience_candidates.json")
            stored = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
            stored_candidate = next((item for item in stored if isinstance(item, dict) and str(item.get("id") or "") == candidate_id), {})
            self.assertEqual(str(stored_candidate.get("status") or ""), "promoted_to_memory")
        finally:
            cleanup()

    def test_peer_cannot_promote_experience(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="foreman1", runtime="codex", runner="pty", enabled=True)
            add_actor(group, actor_id="peer2", runtime="codex", runner="pty", enabled=True)
            group.save()

            resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "peer2"},
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(resp.error.code or ""), "permission_denied")
        finally:
            cleanup()

    def test_mcp_memory_action_promote_experience_routes_to_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group
            from cccc.ports.mcp import server as mcp_server
            from cccc.ports.mcp.common import _RuntimeContext
            from cccc.daemon.server import handle_request
            from cccc.contracts.v1 import DaemonRequest

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="foreman1", runtime="codex", runner="pty", enabled=True)
            add_actor(group, actor_id="peer1", runtime="codex", runner="headless", enabled=True)
            group.save()

            def _fake_call_daemon(req, *args, **kwargs):
                resp, _ = handle_request(DaemonRequest.model_validate(req))
                if resp.ok:
                    return {"ok": True, "result": resp.result}
                return {
                    "ok": False,
                    "error": {
                        "code": str(resp.error.code or ""),
                        "message": str(resp.error.message or ""),
                        "details": resp.error.details if isinstance(resp.error.details, dict) else {},
                    },
                }

            with mock.patch.dict(os.environ, {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "foreman1"}, clear=False), mock.patch(
                "cccc.ports.mcp.common._runtime_context",
                return_value=_RuntimeContext(home=os.environ["CCCC_HOME"], group_id=group_id, actor_id="foreman1"),
            ), mock.patch(
                "cccc.ports.mcp.common.call_daemon",
                side_effect=_fake_call_daemon,
            ), mock.patch(
                "cccc.daemon.codex_app_sessions.SUPERVISOR.actor_running",
                return_value=True,
            ), mock.patch(
                "cccc.daemon.codex_app_sessions.SUPERVISOR.submit_control_message",
                return_value=True,
            ) as submit_control:
                out = mcp_server.handle_tool_call(
                    "cccc_memory",
                    {"action": "promote_experience", "group_id": group_id, "candidate_id": candidate_id, "by": "foreman1"},
                )
            self.assertEqual(str((out.get("candidate") or {}).get("status") or ""), "promoted_to_memory")
            runtime_prompt_delivery = out.get("runtime_prompt_delivery") if isinstance(out.get("runtime_prompt_delivery"), dict) else {}
            self.assertEqual(str(runtime_prompt_delivery.get("status") or ""), "queued")
            deliveries = runtime_prompt_delivery.get("deliveries") if isinstance(runtime_prompt_delivery.get("deliveries"), list) else []
            self.assertEqual(len(deliveries), 1)
            self.assertEqual(str(deliveries[0].get("actor_id") or ""), "peer1")
            self.assertTrue(bool(deliveries[0].get("delivered")))
            submit_control.assert_called_once()
            kwargs = submit_control.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), group_id)
            self.assertEqual(kwargs.get("actor_id"), "peer1")
            self.assertEqual(kwargs.get("control_kind"), "bootstrap")
            text = str(kwargs.get("text") or "")
            self.assertIn("Active Experience Assets:", text)
            self.assertIn(candidate_id, text)
        finally:
            cleanup()

    def test_promote_dry_run_has_no_write_side_effect(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            dry_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user", "dry_run": True},
            )
            self.assertTrue(dry_resp.ok, getattr(dry_resp, "error", None))
            result = dry_resp.result if isinstance(dry_resp.result, dict) else {}
            self.assertTrue(bool(result.get("dry_run")))
            self.assertEqual(str(result.get("commit_state") or ""), "dry_run")

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_file = Path(group.path) / "state" / "memory" / "MEMORY.md"
            if memory_file.exists():
                memory_text = memory_file.read_text(encoding="utf-8")
                self.assertNotIn(candidate_id, memory_text)
            raw = read_json(Path(group.path) / "state" / "experience_candidates.json")
            stored = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
            stored_candidate = next((item for item in stored if isinstance(item, dict) and str(item.get("id") or "") == candidate_id), {})
            self.assertEqual(str(stored_candidate.get("status") or ""), "proposed")
        finally:
            cleanup()

    def test_govern_reject_updates_candidate_without_memory_side_effect(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            resp, _ = self._call(
                "experience_govern",
                {
                    "group_id": group_id,
                    "candidate_id": candidate_id,
                    "lifecycle_action": "reject",
                    "reason": "same pattern was invalidated by later evidence",
                    "by": "user",
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(result.get("changed_candidate_ids"), [candidate_id])
            governed = (result.get("candidates") or [])[0]
            self.assertEqual(str(governed.get("status") or ""), "rejected")
            governance = governed.get("governance") if isinstance(governed.get("governance"), dict) else {}
            self.assertEqual(str((governance.get("rejected") or {}).get("by") or ""), "user")

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            raw = read_json(Path(group.path) / "state" / "experience_candidates.json")
            stored = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
            stored_candidate = next((item for item in stored if isinstance(item, dict) and str(item.get("id") or "") == candidate_id), {})
            self.assertEqual(str(stored_candidate.get("status") or ""), "rejected")
            asset_path = Path(group.path) / "state" / "experience_assets" / f"{candidate_id}.json"
            self.assertFalse(asset_path.exists())
            review = stored_candidate.get("review") if isinstance(stored_candidate.get("review"), dict) else {}
            self.assertEqual(str(review.get("rejected_reason") or ""), "same pattern was invalidated by later evidence")
        finally:
            cleanup()

    def test_runtime_prompt_delivery_consumes_procedural_skill_and_govern_reject_removes_it(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            self._add_headless_codex_actor(group_id)

            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            _, before_text = self._deliver_runtime_prompt(group_id, candidate_id)
            self.assertIn("Active Procedural Skills:", before_text)
            self.assertIn(f"procskill_{candidate_id}", before_text)
            self.assertIn(candidate_id, before_text)

            reject_resp, _ = self._call(
                "experience_govern",
                {
                    "group_id": group_id,
                    "candidate_id": candidate_id,
                    "lifecycle_action": "reject",
                    "reason": "runtime output proved this skill path is stale",
                    "by": "user",
                },
            )
            self.assertTrue(reject_resp.ok, getattr(reject_resp, "error", None))

            _, after_text = self._deliver_runtime_prompt(group_id, candidate_id)
            self.assertNotIn(f"procskill_{candidate_id}", after_text)
            self.assertNotIn("Active Procedural Skills:", after_text)
            self.assertNotIn("Active Experience Assets:", after_text)
            self.assertNotIn(candidate_id, after_text)
        finally:
            cleanup()

    def test_runtime_prompt_delivery_reflects_repair_when_procedural_skill_mirror_is_restored(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            self._add_headless_codex_actor(group_id)

            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            _, before_text = self._deliver_runtime_prompt(group_id, candidate_id)
            self.assertIn(f"procskill_{candidate_id}", before_text)

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            skill_path = Path(group.path) / "state" / "procedural_skills" / f"procskill_{candidate_id}.json"
            self.assertTrue(skill_path.exists())
            skill_path.unlink()

            _, broken_text = self._deliver_runtime_prompt(group_id, candidate_id)
            self.assertNotIn(f"procskill_{candidate_id}", broken_text)
            self.assertNotIn("Active Procedural Skills:", broken_text)

            repair_resp, _ = self._call(
                "experience_repair_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(repair_resp.ok, getattr(repair_resp, "error", None))
            asset_write = (
                repair_resp.result.get("asset_write")
                if isinstance(repair_resp.result, dict) and isinstance(repair_resp.result.get("asset_write"), dict)
                else {}
            )
            self.assertEqual(str(asset_write.get("status") or ""), "written")
            skill_write = (
                repair_resp.result.get("skill_write")
                if isinstance(repair_resp.result, dict) and isinstance(repair_resp.result.get("skill_write"), dict)
                else {}
            )
            self.assertEqual(str(skill_write.get("status") or ""), "written")

            _, repaired_text = self._deliver_runtime_prompt(group_id, candidate_id)
            self.assertIn("Active Procedural Skills:", repaired_text)
            self.assertIn(f"procskill_{candidate_id}", repaired_text)
            self.assertIn("Active Experience Assets:", repaired_text)
            self.assertIn(candidate_id, repaired_text)
        finally:
            cleanup()

    def test_report_skill_usage_generates_patch_candidate(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-001",
                    "evidence_type": "tool_mismatch",
                    "evidence_payload": {"tool": "search", "missed_step": "verify recall gate"},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "add_step",
                    "reason": "runtime skipped the verification step",
                    "proposed_delta": {"step": "Verify recall gate before answering."},
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            evidence = result.get("usage_evidence") if isinstance(result.get("usage_evidence"), dict) else {}
            self.assertEqual(str(evidence.get("skill_id") or ""), f"procskill_{candidate_id}")
            self.assertEqual(str(evidence.get("turn_id") or ""), "turn-001")
            patch_candidate = result.get("patch_candidate") if isinstance(result.get("patch_candidate"), dict) else {}
            self.assertEqual(str(patch_candidate.get("status") or ""), "pending")
            self.assertEqual(str(patch_candidate.get("patch_kind") or ""), "add_step")
            patch_gate = result.get("patch_gate") if isinstance(result.get("patch_gate"), dict) else {}
            self.assertEqual(str(patch_gate.get("status") or ""), "candidate_ready")
            self.assertGreaterEqual(float(patch_gate.get("score") or 0.0), float(patch_gate.get("threshold") or 1.0))

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            usage_doc = read_json(Path(group.path) / "state" / "procedural_skill_usage.json")
            events = usage_doc.get("events") if isinstance(usage_doc.get("events"), list) else []
            self.assertEqual(len(events), 1)
            candidates_doc = read_json(Path(group.path) / "state" / "procedural_skill_patch_candidates.json")
            candidates = candidates_doc.get("candidates") if isinstance(candidates_doc.get("candidates"), list) else []
            self.assertEqual(len(candidates), 1)
            self.assertEqual(str(candidates[0].get("skill_id") or ""), f"procskill_{candidate_id}")
        finally:
            cleanup()

    def test_report_skill_usage_below_threshold_only_persists_evidence(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-low-score-001",
                    "evidence_type": "note_only",
                    "outcome": "ok",
                    "generate_patch": True,
                    "patch_kind": "add_step",
                    "reason": "weak signal should stay as evidence only",
                    "proposed_delta": {"step": "Do a speculative follow-up."},
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            evidence = result.get("usage_evidence") if isinstance(result.get("usage_evidence"), dict) else {}
            self.assertEqual(str(evidence.get("turn_id") or ""), "turn-low-score-001")
            self.assertLess(float(evidence.get("score") or 0.0), 0.6)
            self.assertEqual(result.get("patch_candidate"), {})
            patch_gate = result.get("patch_gate") if isinstance(result.get("patch_gate"), dict) else {}
            self.assertEqual(str(patch_gate.get("status") or ""), "below_threshold")
            self.assertLess(float(patch_gate.get("score") or 0.0), float(patch_gate.get("threshold") or 0.0))

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            usage_doc = read_json(Path(group.path) / "state" / "procedural_skill_usage.json")
            events = usage_doc.get("events") if isinstance(usage_doc.get("events"), list) else []
            self.assertEqual(len(events), 1)
            candidates_doc = read_json(Path(group.path) / "state" / "procedural_skill_patch_candidates.json")
            candidates = candidates_doc.get("candidates") if isinstance(candidates_doc.get("candidates"), list) else []
            self.assertEqual(candidates, [])
        finally:
            cleanup()

    def test_report_skill_usage_dedupes_same_patch_candidate(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            payload = {
                "group_id": group_id,
                "skill_id": f"procskill_{candidate_id}",
                "by": "user",
                "actor_id": "peer1",
                "evidence_type": "missing_constraint",
                "evidence_payload": {"constraint": "must verify recall gate first"},
                "outcome": "failed",
                "generate_patch": True,
                "patch_kind": "adjust_constraint",
                "reason": "same failure repeated",
                "proposed_delta": {"constraint": "Verify recall gate before acting on recalled memory."},
            }
            first_resp, _ = self._call(
                "procedural_skill_report_usage",
                dict(payload, turn_id="turn-dedupe-001"),
            )
            second_resp, _ = self._call(
                "procedural_skill_report_usage",
                dict(payload, turn_id="turn-dedupe-002"),
            )
            self.assertTrue(first_resp.ok, getattr(first_resp, "error", None))
            self.assertTrue(second_resp.ok, getattr(second_resp, "error", None))

            first_result = first_resp.result if isinstance(first_resp.result, dict) else {}
            second_result = second_resp.result if isinstance(second_resp.result, dict) else {}
            first_candidate = first_result.get("patch_candidate") if isinstance(first_result.get("patch_candidate"), dict) else {}
            second_candidate = second_result.get("patch_candidate") if isinstance(second_result.get("patch_candidate"), dict) else {}
            self.assertEqual(
                str(first_candidate.get("candidate_id") or ""),
                str(second_candidate.get("candidate_id") or ""),
            )

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            candidates_doc = read_json(Path(group.path) / "state" / "procedural_skill_patch_candidates.json")
            candidates = candidates_doc.get("candidates") if isinstance(candidates_doc.get("candidates"), list) else []
            self.assertEqual(len(candidates), 1)
            evidence_refs = list((candidates[0] if candidates else {}).get("evidence_refs") or [])
            self.assertEqual(len(evidence_refs), 2)
        finally:
            cleanup()

    def test_govern_skill_patch_merge_updates_procedural_skill(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            report_resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-002",
                    "evidence_type": "missing_constraint",
                    "evidence_payload": {"constraint": "must verify recall gate first"},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "adjust_constraint",
                    "reason": "runtime drifted without explicit guardrail",
                    "proposed_delta": {"constraint": "Verify recall gate before acting on recalled memory."},
                },
            )
            self.assertTrue(report_resp.ok, getattr(report_resp, "error", None))
            patch_candidate = (
                report_resp.result.get("patch_candidate")
                if isinstance(report_resp.result, dict) and isinstance(report_resp.result.get("patch_candidate"), dict)
                else {}
            )
            patch_candidate_id = str(patch_candidate.get("candidate_id") or "")
            self.assertTrue(patch_candidate_id)

            govern_resp, _ = self._call(
                "procedural_skill_govern_patch",
                {
                    "group_id": group_id,
                    "candidate_id": patch_candidate_id,
                    "lifecycle_action": "merge",
                    "reason": "merge tested procedural correction",
                    "by": "user",
                },
            )
            self.assertTrue(govern_resp.ok, getattr(govern_resp, "error", None))
            result = govern_resp.result if isinstance(govern_resp.result, dict) else {}
            governed = result.get("candidate") if isinstance(result.get("candidate"), dict) else {}
            self.assertEqual(str(governed.get("status") or ""), "merged")
            skill_write = result.get("skill_write") if isinstance(result.get("skill_write"), dict) else {}
            self.assertEqual(str(skill_write.get("status") or ""), "merged")
            asset = skill_write.get("asset") if isinstance(skill_write.get("asset"), dict) else {}
            self.assertIn(
                "Verify recall gate before acting on recalled memory.",
                list(asset.get("constraints") or []),
            )
            post_merge_evaluation = (
                asset.get("post_merge_evaluation")
                if isinstance(asset.get("post_merge_evaluation"), dict)
                else {}
            )
            self.assertEqual(str(post_merge_evaluation.get("status") or ""), "observing")
            self.assertEqual(str(post_merge_evaluation.get("candidate_id") or ""), patch_candidate_id)
            self.assertTrue(str(post_merge_evaluation.get("opened_at") or ""))
            self.assertTrue(str(post_merge_evaluation.get("observe_until") or ""))
            self.assertEqual(str(asset.get("stability") or ""), "probation")
            governance_policy = asset.get("governance_policy") if isinstance(asset.get("governance_policy"), dict) else {}
            self.assertEqual(str(governance_policy.get("patch_review_mode") or ""), "auto_merge_eligible")
        finally:
            cleanup()

    def test_report_skill_usage_validates_observing_patch_after_success(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            report_resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-003",
                    "evidence_type": "missing_constraint",
                    "evidence_payload": {"constraint": "must verify recall gate first"},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "adjust_constraint",
                    "reason": "runtime drifted without explicit guardrail",
                    "proposed_delta": {"constraint": "Verify recall gate before acting on recalled memory."},
                },
            )
            self.assertTrue(report_resp.ok, getattr(report_resp, "error", None))
            patch_candidate = (
                report_resp.result.get("patch_candidate")
                if isinstance(report_resp.result, dict) and isinstance(report_resp.result.get("patch_candidate"), dict)
                else {}
            )
            patch_candidate_id = str(patch_candidate.get("candidate_id") or "")
            self.assertTrue(patch_candidate_id)

            govern_resp, _ = self._call(
                "procedural_skill_govern_patch",
                {
                    "group_id": group_id,
                    "candidate_id": patch_candidate_id,
                    "lifecycle_action": "merge",
                    "reason": "merge tested procedural correction",
                    "by": "user",
                },
            )
            self.assertTrue(govern_resp.ok, getattr(govern_resp, "error", None))

            validate_resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-004",
                    "evidence_type": "note_only",
                    "outcome": "success",
                    "generate_patch": False,
                },
            )
            self.assertTrue(validate_resp.ok, getattr(validate_resp, "error", None))
            result = validate_resp.result if isinstance(validate_resp.result, dict) else {}
            evaluation = (
                result.get("post_merge_evaluation")
                if isinstance(result.get("post_merge_evaluation"), dict)
                else {}
            )
            self.assertEqual(str(evaluation.get("status") or ""), "validated")
            asset = evaluation.get("asset") if isinstance(evaluation.get("asset"), dict) else {}
            asset_eval = asset.get("post_merge_evaluation") if isinstance(asset.get("post_merge_evaluation"), dict) else {}
            self.assertEqual(str(asset_eval.get("status") or ""), "validated")
            self.assertEqual(str(asset_eval.get("candidate_id") or ""), patch_candidate_id)
            self.assertEqual(str(asset.get("stability") or ""), "stable")
            governance_policy = asset.get("governance_policy") if isinstance(asset.get("governance_policy"), dict) else {}
            self.assertEqual(str(governance_policy.get("patch_review_mode") or ""), "auto_merge_eligible")
        finally:
            cleanup()

    def test_report_skill_usage_marks_observing_patch_regressed_and_links_followup(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            report_resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-005",
                    "evidence_type": "missing_constraint",
                    "evidence_payload": {"constraint": "must verify recall gate first"},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "adjust_constraint",
                    "reason": "runtime drifted without explicit guardrail",
                    "proposed_delta": {"constraint": "Verify recall gate before acting on recalled memory."},
                },
            )
            self.assertTrue(report_resp.ok, getattr(report_resp, "error", None))
            patch_candidate = (
                report_resp.result.get("patch_candidate")
                if isinstance(report_resp.result, dict) and isinstance(report_resp.result.get("patch_candidate"), dict)
                else {}
            )
            patch_candidate_id = str(patch_candidate.get("candidate_id") or "")
            self.assertTrue(patch_candidate_id)

            govern_resp, _ = self._call(
                "procedural_skill_govern_patch",
                {
                    "group_id": group_id,
                    "candidate_id": patch_candidate_id,
                    "lifecycle_action": "merge",
                    "reason": "merge tested procedural correction",
                    "by": "user",
                },
            )
            self.assertTrue(govern_resp.ok, getattr(govern_resp, "error", None))

            regress_resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-006",
                    "evidence_type": "failure_signal_triggered",
                    "evidence_payload": {"failure_signal": "Recall gate was skipped again."},
                    "outcome": "regressed",
                    "generate_patch": True,
                    "patch_kind": "clarify_failure_signal",
                    "reason": "merged patch still missed runtime failure signal",
                    "proposed_delta": {"failure_signal": "Recall gate skipped again after patch merge."},
                },
            )
            self.assertTrue(regress_resp.ok, getattr(regress_resp, "error", None))
            result = regress_resp.result if isinstance(regress_resp.result, dict) else {}
            followup_candidate = result.get("patch_candidate") if isinstance(result.get("patch_candidate"), dict) else {}
            evaluation = (
                result.get("post_merge_evaluation")
                if isinstance(result.get("post_merge_evaluation"), dict)
                else {}
            )
            self.assertEqual(str(evaluation.get("status") or ""), "regressed")
            asset = evaluation.get("asset") if isinstance(evaluation.get("asset"), dict) else {}
            asset_eval = asset.get("post_merge_evaluation") if isinstance(asset.get("post_merge_evaluation"), dict) else {}
            self.assertEqual(str(asset_eval.get("status") or ""), "regressed")
            self.assertEqual(
                str(asset_eval.get("followup_candidate_id") or ""),
                str(followup_candidate.get("candidate_id") or ""),
            )
            self.assertEqual(str(asset.get("stability") or ""), "unstable")
            governance_policy = asset.get("governance_policy") if isinstance(asset.get("governance_policy"), dict) else {}
            self.assertEqual(str(governance_policy.get("patch_review_mode") or ""), "manual_review_required")
            self.assertEqual(str(followup_candidate.get("review_mode") or ""), "manual_review_required")
            lineage = followup_candidate.get("lineage") if isinstance(followup_candidate.get("lineage"), dict) else {}
            self.assertEqual(str(lineage.get("regressed_from_candidate_id") or ""), patch_candidate_id)
        finally:
            cleanup()

    def test_new_patch_candidate_inherits_manual_review_after_skill_regression(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            first_report, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-007",
                    "evidence_type": "missing_constraint",
                    "evidence_payload": {"constraint": "must verify recall gate first"},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "adjust_constraint",
                    "reason": "runtime drifted without explicit guardrail",
                    "proposed_delta": {"constraint": "Verify recall gate before acting on recalled memory."},
                },
            )
            self.assertTrue(first_report.ok, getattr(first_report, "error", None))
            first_candidate = (
                first_report.result.get("patch_candidate")
                if isinstance(first_report.result, dict) and isinstance(first_report.result.get("patch_candidate"), dict)
                else {}
            )
            first_candidate_id = str(first_candidate.get("candidate_id") or "")
            self.assertTrue(first_candidate_id)

            govern_resp, _ = self._call(
                "procedural_skill_govern_patch",
                {
                    "group_id": group_id,
                    "candidate_id": first_candidate_id,
                    "lifecycle_action": "merge",
                    "reason": "merge tested procedural correction",
                    "by": "user",
                },
            )
            self.assertTrue(govern_resp.ok, getattr(govern_resp, "error", None))

            regress_resp, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-008",
                    "evidence_type": "failure_signal_triggered",
                    "evidence_payload": {"failure_signal": "Recall gate was skipped again."},
                    "outcome": "regressed",
                    "generate_patch": True,
                    "patch_kind": "clarify_failure_signal",
                    "reason": "merged patch still missed runtime failure signal",
                    "proposed_delta": {"failure_signal": "Recall gate skipped again after patch merge."},
                },
            )
            self.assertTrue(regress_resp.ok, getattr(regress_resp, "error", None))

            next_report, _ = self._call(
                "procedural_skill_report_usage",
                {
                    "group_id": group_id,
                    "skill_id": f"procskill_{candidate_id}",
                    "by": "user",
                    "actor_id": "peer1",
                    "turn_id": "turn-009",
                    "evidence_type": "missing_step",
                    "evidence_payload": {"step": "verify prompt injection result"},
                    "outcome": "failed",
                    "generate_patch": True,
                    "patch_kind": "add_step",
                    "reason": "skill is unstable after regression",
                    "proposed_delta": {"step": "Verify prompt injection result after recall gate."},
                },
            )
            self.assertTrue(next_report.ok, getattr(next_report, "error", None))
            next_result = next_report.result if isinstance(next_report.result, dict) else {}
            next_candidate = next_result.get("patch_candidate") if isinstance(next_result.get("patch_candidate"), dict) else {}
            self.assertEqual(str(next_candidate.get("review_mode") or ""), "manual_review_required")
        finally:
            cleanup()

    def test_mcp_memory_action_report_skill_usage_routes_to_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group
            from cccc.ports.mcp import server as mcp_server
            from cccc.ports.mcp.common import _RuntimeContext
            from cccc.daemon.server import handle_request
            from cccc.contracts.v1 import DaemonRequest

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="foreman1", runtime="codex", runner="pty", enabled=True)

            def _fake_call_daemon(req, *args, **kwargs):
                resp, _ = handle_request(DaemonRequest.model_validate(req))
                if resp.ok:
                    return {"ok": True, "result": resp.result}
                return {
                    "ok": False,
                    "error": {
                        "code": str(resp.error.code or ""),
                        "message": str(resp.error.message or ""),
                        "details": resp.error.details if isinstance(resp.error.details, dict) else {},
                    },
                }

            with mock.patch.dict(os.environ, {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "foreman1"}, clear=False), mock.patch(
                "cccc.ports.mcp.common._runtime_context",
                return_value=_RuntimeContext(home=os.environ["CCCC_HOME"], group_id=group_id, actor_id="foreman1"),
            ), mock.patch(
                "cccc.ports.mcp.common.call_daemon",
                side_effect=_fake_call_daemon,
            ):
                out = mcp_server.handle_tool_call(
                    "cccc_memory",
                    {
                        "action": "report_skill_usage",
                        "group_id": group_id,
                        "skill_id": f"procskill_{candidate_id}",
                        "turn_id": "turn-mcp-001",
                        "evidence_type": "tool_mismatch",
                        "evidence_payload": {"tool": "search"},
                        "generate_patch": True,
                        "patch_kind": "clarify_failure_signal",
                        "proposed_delta": {"failure_signal": "Search ran before recall gate verification."},
                        "reason": "reported via MCP",
                        "by": "foreman1",
                    },
                )
            evidence = out.get("usage_evidence") if isinstance(out, dict) else {}
            self.assertEqual(str((evidence or {}).get("turn_id") or ""), "turn-mcp-001")
            patch_candidate = out.get("patch_candidate") if isinstance(out, dict) else {}
            self.assertEqual(str((patch_candidate or {}).get("patch_kind") or ""), "clarify_failure_signal")
        finally:
            cleanup()

    def test_govern_merge_preserves_lineage_and_marks_source_retired(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_ids = self._create_group_with_two_candidates()
            target_candidate_id, source_candidate_id = candidate_ids[0], candidate_ids[1]
            resp, _ = self._call(
                "experience_govern",
                {
                    "group_id": group_id,
                    "target_candidate_id": target_candidate_id,
                    "source_candidate_ids": [source_candidate_id],
                    "lifecycle_action": "merge",
                    "reason": "combined into stronger reusable pattern",
                    "by": "user",
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))

            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(
                result.get("changed_candidate_ids"),
                [source_candidate_id, target_candidate_id],
            )
            preview = result.get("candidates") if isinstance(result.get("candidates"), list) else []
            source_candidate = next((item for item in preview if str(item.get("id") or "") == source_candidate_id), {})
            target_candidate = next((item for item in preview if str(item.get("id") or "") == target_candidate_id), {})
            self.assertEqual(str(source_candidate.get("status") or ""), "merged")
            target_governance = target_candidate.get("governance") if isinstance(target_candidate.get("governance"), dict) else {}
            self.assertIn(source_candidate_id, target_governance.get("merged_from") or [])
            self.assertIn(f"experience:{source_candidate_id}", target_candidate.get("source_refs") or [])
        finally:
            cleanup()

    def test_govern_supersede_dry_run_has_no_side_effect(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_ids = self._create_group_with_two_candidates()
            target_candidate_id, source_candidate_id = candidate_ids[0], candidate_ids[1]
            resp, _ = self._call(
                "experience_govern",
                {
                    "group_id": group_id,
                    "target_candidate_id": target_candidate_id,
                    "source_candidate_ids": [source_candidate_id],
                    "lifecycle_action": "supersede",
                    "reason": "newer decision replaces the earlier variant",
                    "by": "user",
                    "dry_run": True,
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertTrue(bool(result.get("dry_run")))

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            raw = read_json(Path(group.path) / "state" / "experience_candidates.json")
            stored = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
            stored_source = next((item for item in stored if isinstance(item, dict) and str(item.get("id") or "") == source_candidate_id), {})
            stored_target = next((item for item in stored if isinstance(item, dict) and str(item.get("id") or "") == target_candidate_id), {})
            self.assertEqual(str(stored_source.get("status") or ""), "proposed")
            target_governance = stored_target.get("governance") if isinstance(stored_target.get("governance"), dict) else {}
            self.assertEqual(target_governance, {})
        finally:
            cleanup()

    def test_peer_cannot_govern_experience(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="foreman1", runtime="codex", runner="pty", enabled=True)
            add_actor(group, actor_id="peer2", runtime="codex", runner="pty", enabled=True)
            group.save()

            resp, _ = self._call(
                "experience_govern",
                {
                    "group_id": group_id,
                    "candidate_id": candidate_id,
                    "lifecycle_action": "reject",
                    "reason": "peer should be denied",
                    "by": "peer2",
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(resp.error.code or ""), "permission_denied")
        finally:
            cleanup()

    def test_govern_reject_tombstones_promoted_candidate_in_memory_lane(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))
            promoted_candidate = promote_resp.result.get("candidate") if isinstance(promote_resp.result, dict) else {}
            promotion = promoted_candidate.get("promotion") if isinstance(promoted_candidate.get("promotion"), dict) else {}
            memory_entry = promotion.get("memory_entry") if isinstance(promotion.get("memory_entry"), dict) else {}
            entry_id = str(memory_entry.get("entry_id") or "")

            resp, _ = self._call(
                "experience_govern",
                {
                    "group_id": group_id,
                    "candidate_id": candidate_id,
                    "lifecycle_action": "reject",
                    "reason": "retire promoted candidate",
                    "by": "user",
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(result.get("memory_effects"), [{"candidate_id": candidate_id, "action": "tombstone", "entry_id": entry_id}])
            self.assertEqual(str(result.get("commit_state") or ""), "disk_committed")

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_text = (Path(group.path) / "state" / "memory" / "MEMORY.md").read_text(encoding="utf-8")
            self.assertIn(entry_id, memory_text)
            self.assertIn(f"experience:{candidate_id}", memory_text)
            self.assertIn(f"## {entry_id} [experience_retired]", memory_text)
            self.assertIn('"lifecycle_state": "retired"', memory_text)
            skill_path = Path(group.path) / "state" / "procedural_skills" / f"procskill_{candidate_id}.json"
            self.assertFalse(skill_path.exists())
            procedural_skill_effects = result.get("procedural_skill_effects") if isinstance(result.get("procedural_skill_effects"), list) else []
            self.assertEqual(
                procedural_skill_effects,
                [
                    {
                        "candidate_id": candidate_id,
                        "status": "deleted",
                        "file_path": str(skill_path),
                        "skill_id": f"procskill_{candidate_id}",
                    }
                ],
            )
            asset_path = Path(group.path) / "state" / "experience_assets" / f"{candidate_id}.json"
            self.assertFalse(asset_path.exists())
            experience_asset_effects = result.get("experience_asset_effects") if isinstance(result.get("experience_asset_effects"), list) else []
            self.assertEqual(
                experience_asset_effects,
                [
                    {
                        "candidate_id": candidate_id,
                        "status": "deleted",
                        "file_path": str(asset_path),
                        "asset_id": f"expasset_{candidate_id}",
                    }
                ],
            )
        finally:
            cleanup()

    def test_rejected_candidate_cannot_be_promoted_again(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            reject_resp, _ = self._call(
                "experience_govern",
                {
                    "group_id": group_id,
                    "candidate_id": candidate_id,
                    "lifecycle_action": "reject",
                    "reason": "retired stays retired",
                    "by": "user",
                },
            )
            self.assertTrue(reject_resp.ok, getattr(reject_resp, "error", None))

            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertFalse(promote_resp.ok)
            self.assertEqual(str(promote_resp.error.code or ""), "validation_error")
            self.assertIn("cannot be promoted", str(promote_resp.error.message or ""))
        finally:
            cleanup()

    def test_govern_merge_rejects_promoted_source_when_target_not_promoted(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_ids = self._create_group_with_two_candidates()
            target_candidate_id, source_candidate_id = candidate_ids[0], candidate_ids[1]
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": source_candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))
            promoted_source = promote_resp.result.get("candidate") if isinstance(promote_resp.result, dict) else {}
            promoted_source_promotion = promoted_source.get("promotion") if isinstance(promoted_source.get("promotion"), dict) else {}
            source_entry_id = str(((promoted_source_promotion.get("memory_entry") or {}) if isinstance(promoted_source_promotion.get("memory_entry"), dict) else {}).get("entry_id") or "")

            resp, _ = self._call(
                "experience_govern",
                {
                    "group_id": group_id,
                    "target_candidate_id": target_candidate_id,
                    "source_candidate_ids": [source_candidate_id],
                    "lifecycle_action": "merge",
                    "reason": "promoted source must be denied",
                    "by": "user",
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(resp.error.code or ""), "validation_error")
            self.assertIn("target to already be promoted_to_memory", str(resp.error.message or ""))

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_text = (Path(group.path) / "state" / "memory" / "MEMORY.md").read_text(encoding="utf-8")
            self.assertIn(source_entry_id, memory_text)
            self.assertNotIn(f"experience:{target_candidate_id}", memory_text)
        finally:
            cleanup()

    def test_govern_supersede_retire_replace_promoted_blocks(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_ids = self._create_group_with_two_candidates()
            target_candidate_id, source_candidate_id = candidate_ids[0], candidate_ids[1]
            promote_target_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": target_candidate_id, "by": "user"},
            )
            self.assertTrue(promote_target_resp.ok, getattr(promote_target_resp, "error", None))
            target_candidate = promote_target_resp.result.get("candidate") if isinstance(promote_target_resp.result, dict) else {}
            target_promotion = target_candidate.get("promotion") if isinstance(target_candidate.get("promotion"), dict) else {}
            target_entry_id = str(((target_promotion.get("memory_entry") or {}) if isinstance(target_promotion.get("memory_entry"), dict) else {}).get("entry_id") or "")
            promote_source_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": source_candidate_id, "by": "user"},
            )
            self.assertTrue(promote_source_resp.ok, getattr(promote_source_resp, "error", None))
            source_candidate = promote_source_resp.result.get("candidate") if isinstance(promote_source_resp.result, dict) else {}
            source_promotion = source_candidate.get("promotion") if isinstance(source_candidate.get("promotion"), dict) else {}
            source_entry_id = str(((source_promotion.get("memory_entry") or {}) if isinstance(source_promotion.get("memory_entry"), dict) else {}).get("entry_id") or "")

            resp, _ = self._call(
                "experience_govern",
                {
                    "group_id": group_id,
                    "target_candidate_id": target_candidate_id,
                    "source_candidate_ids": [source_candidate_id],
                    "lifecycle_action": "supersede",
                    "reason": "newer promoted target replaces old variant",
                    "by": "user",
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_text = (Path(group.path) / "state" / "memory" / "MEMORY.md").read_text(encoding="utf-8")
            self.assertIn(target_entry_id, memory_text)
            self.assertIn(f"experience:{source_candidate_id}", memory_text)
            self.assertIn(f"## {source_entry_id} [experience_retired]", memory_text)
            self.assertEqual(memory_text.count(f"## {target_entry_id} [experience]"), 1)
        finally:
            cleanup()

    def test_promote_index_failure_returns_disk_committed_index_stale(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            with mock.patch("cccc.daemon.memory.experience_memory_lane.index_sync", side_effect=RuntimeError("index offline")):
                resp, _ = self._call(
                    "experience_promote_to_memory",
                    {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("commit_state") or ""), "disk_committed_index_stale")
            index_sync = result.get("index_sync") if isinstance(result.get("index_sync"), dict) else {}
            self.assertEqual(str(index_sync.get("status") or ""), "stale")

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_text = (Path(group.path) / "state" / "memory" / "MEMORY.md").read_text(encoding="utf-8")
            self.assertIn(candidate_id, memory_text)
        finally:
            cleanup()

    def test_promote_rolls_back_memory_when_candidate_persist_fails(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_file = Path(group.path) / "state" / "memory" / "MEMORY.md"
            original = memory_file.read_text(encoding="utf-8") if memory_file.exists() else ""

            calls = {"count": 0}
            real_persist = __import__("cccc.daemon.memory.experience_memory_lane", fromlist=["persist_candidates"]).persist_candidates

            def _boom(path, candidates):
                calls["count"] += 1
                if calls["count"] == 1:
                    raise RuntimeError("candidate write failed")
                return real_persist(path, candidates)

            with mock.patch("cccc.daemon.memory.experience_memory_lane.persist_candidates", side_effect=_boom):
                resp, _ = self._call(
                    "experience_promote_to_memory",
                    {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
                )
            self.assertFalse(resp.ok)
            self.assertEqual(str(resp.error.code or ""), "memory_sync_error")
            self.assertEqual(memory_file.read_text(encoding="utf-8") if memory_file.exists() else "", original)
        finally:
            cleanup()

    def test_repair_experience_already_structured_returns_no_change(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))
            memory_path = self._memory_path(group_id)
            before = memory_path.read_text(encoding="utf-8")

            resp, _ = self._call(
                "experience_repair_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertTrue(bool(result.get("already_structured")))
            self.assertEqual(str(result.get("commit_state") or ""), "no_change")
            self.assertEqual(memory_path.read_text(encoding="utf-8"), before)
        finally:
            cleanup()

    def test_repair_experience_already_structured_persists_missing_locator(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))
            memory_path = self._memory_path(group_id)

            def _drop_locator(item):
                promotion = item.get("promotion") if isinstance(item.get("promotion"), dict) else {}
                promotion.pop("memory_entry", None)
                item["promotion"] = promotion
                return item

            self._rewrite_candidate(group_id, candidate_id, _drop_locator)

            resp, _ = self._call(
                "experience_repair_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertTrue(bool(result.get("already_structured")))
            self.assertEqual(str(result.get("commit_state") or ""), "candidate_committed")

            from cccc.util.fs import read_json

            updated_raw = read_json(self._candidate_path(group_id))
            updated = updated_raw.get("candidates") if isinstance(updated_raw.get("candidates"), list) else []
            updated_candidate = next((item for item in updated if isinstance(item, dict) and str(item.get("id") or "") == candidate_id), {})
            promotion = updated_candidate.get("promotion") if isinstance(updated_candidate.get("promotion"), dict) else {}
            memory_entry = promotion.get("memory_entry") if isinstance(promotion.get("memory_entry"), dict) else {}
            self.assertEqual(str(memory_entry.get("entry_id") or ""), f"expmem_{candidate_id}")
            self.assertEqual(str(memory_entry.get("file_path") or ""), str(memory_path))
        finally:
            cleanup()

    def test_repair_experience_already_structured_restores_missing_procedural_skill_mirror(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            promote_resp, _ = self._call(
                "experience_promote_to_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(promote_resp.ok, getattr(promote_resp, "error", None))

            from cccc.kernel.group import load_group
            from cccc.util.fs import read_json

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            skill_path = Path(group.path) / "state" / "procedural_skills" / f"procskill_{candidate_id}.json"
            self.assertTrue(skill_path.exists())
            skill_path.unlink()
            self.assertFalse(skill_path.exists())

            resp, _ = self._call(
                "experience_repair_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertTrue(bool(result.get("already_structured")))
            self.assertEqual(str(result.get("commit_state") or ""), "no_change")
            skill_write = result.get("skill_write") if isinstance(result.get("skill_write"), dict) else {}
            self.assertEqual(str(skill_write.get("status") or ""), "written")
            self.assertEqual(str(skill_write.get("file_path") or ""), str(skill_path))
            self.assertTrue(skill_path.exists())
            skill_doc = read_json(skill_path)
            self.assertEqual(str(skill_doc.get("source_experience_candidate_id") or ""), candidate_id)
            self.assertEqual(str(skill_doc.get("memory_entry_id") or ""), f"expmem_{candidate_id}")
        finally:
            cleanup()

    def test_repair_experience_rewraps_unique_legacy_block_and_updates_candidate(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            candidate_path = self._candidate_path(group_id)
            memory_path = self._memory_path(group_id)
            from cccc.util.fs import read_json

            raw = read_json(candidate_path)
            stored = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
            candidate = next((item for item in stored if isinstance(item, dict) and str(item.get("id") or "") == candidate_id), {})
            legacy_body = __import__("cccc.daemon.memory.experience_memory_lane", fromlist=["_render_memory_summary"])._render_memory_summary(
                {**candidate, "status": "promoted_to_memory"}
            )
            memory_path.parent.mkdir(parents=True, exist_ok=True)
            memory_path.write_text(legacy_body, encoding="utf-8")

            def _mutate(item):
                item["status"] = "promoted_to_memory"
                item["promotion"] = {"by": "user", "at": "2026-04-11T00:00:00Z", "target": "memory"}
                return item

            self._rewrite_candidate(group_id, candidate_id, _mutate)

            resp, _ = self._call(
                "experience_repair_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertFalse(bool(result.get("already_structured")))
            self.assertEqual(str(result.get("commit_state") or ""), "disk_committed")

            repaired_text = memory_path.read_text(encoding="utf-8")
            self.assertIn(f"## expmem_{candidate_id} [experience]", repaired_text)
            self.assertIn('"lifecycle_state": "active"', repaired_text)
            self.assertIn(legacy_body.strip(), repaired_text)

            updated_raw = read_json(candidate_path)
            updated = updated_raw.get("candidates") if isinstance(updated_raw.get("candidates"), list) else []
            updated_candidate = next((item for item in updated if isinstance(item, dict) and str(item.get("id") or "") == candidate_id), {})
            promotion = updated_candidate.get("promotion") if isinstance(updated_candidate.get("promotion"), dict) else {}
            memory_entry = promotion.get("memory_entry") if isinstance(promotion.get("memory_entry"), dict) else {}
            self.assertEqual(str(memory_entry.get("entry_id") or ""), f"expmem_{candidate_id}")
            self.assertEqual(str(memory_entry.get("file_path") or ""), str(memory_path))
        finally:
            cleanup()

    def test_repair_experience_rejects_ambiguous_legacy_matches(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            candidate_path = self._candidate_path(group_id)
            memory_path = self._memory_path(group_id)
            from cccc.util.fs import read_json

            raw = read_json(candidate_path)
            stored = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
            candidate = next((item for item in stored if isinstance(item, dict) and str(item.get("id") or "") == candidate_id), {})
            legacy_body = __import__("cccc.daemon.memory.experience_memory_lane", fromlist=["_render_memory_summary"])._render_memory_summary(
                {**candidate, "status": "promoted_to_memory"}
            )
            memory_path.parent.mkdir(parents=True, exist_ok=True)
            memory_path.write_text(f"{legacy_body}\n{legacy_body}", encoding="utf-8")

            def _mutate(item):
                item["status"] = "promoted_to_memory"
                item["promotion"] = {"by": "user", "at": "2026-04-11T00:00:00Z", "target": "memory"}
                return item

            self._rewrite_candidate(group_id, candidate_id, _mutate)
            resp, _ = self._call(
                "experience_repair_memory",
                {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(resp.error.code or ""), "validation_error")
            self.assertIn("exactly once", str(resp.error.message or ""))
        finally:
            cleanup()

    def test_repair_experience_index_failure_keeps_disk_committed(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            candidate_path = self._candidate_path(group_id)
            memory_path = self._memory_path(group_id)
            from cccc.util.fs import read_json

            raw = read_json(candidate_path)
            stored = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
            candidate = next((item for item in stored if isinstance(item, dict) and str(item.get("id") or "") == candidate_id), {})
            legacy_body = __import__("cccc.daemon.memory.experience_memory_lane", fromlist=["_render_memory_summary"])._render_memory_summary(
                {**candidate, "status": "promoted_to_memory"}
            )
            memory_path.parent.mkdir(parents=True, exist_ok=True)
            memory_path.write_text(legacy_body, encoding="utf-8")

            def _mutate(item):
                item["status"] = "promoted_to_memory"
                item["promotion"] = {"by": "user", "at": "2026-04-11T00:00:00Z", "target": "memory"}
                return item

            self._rewrite_candidate(group_id, candidate_id, _mutate)

            with mock.patch("cccc.daemon.memory.experience_memory_lane.index_sync", side_effect=RuntimeError("index offline")):
                resp, _ = self._call(
                    "experience_repair_memory",
                    {"group_id": group_id, "candidate_id": candidate_id, "by": "user"},
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result if isinstance(resp.result, dict) else {}
            self.assertEqual(str(result.get("commit_state") or ""), "disk_committed_index_stale")
            self.assertIn(f"## expmem_{candidate_id} [experience]", memory_path.read_text(encoding="utf-8"))

            from cccc.util.fs import read_json as _read_json
            updated_raw = _read_json(candidate_path)
            updated = updated_raw.get("candidates") if isinstance(updated_raw.get("candidates"), list) else []
            updated_candidate = next((item for item in updated if isinstance(item, dict) and str(item.get("id") or "") == candidate_id), {})
            promotion = updated_candidate.get("promotion") if isinstance(updated_candidate.get("promotion"), dict) else {}
            self.assertEqual(str(((promotion.get("memory_entry") or {}) if isinstance(promotion.get("memory_entry"), dict) else {}).get("entry_id") or ""), f"expmem_{candidate_id}")
        finally:
            cleanup()

    def test_mcp_memory_action_govern_experience_routes_to_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_ids = self._create_group_with_two_candidates()
            target_candidate_id, source_candidate_id = candidate_ids[0], candidate_ids[1]
            from cccc.kernel.actors import add_actor
            from cccc.kernel.group import load_group
            from cccc.ports.mcp import server as mcp_server
            from cccc.ports.mcp.common import _RuntimeContext
            from cccc.daemon.server import handle_request
            from cccc.contracts.v1 import DaemonRequest

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="foreman1", runtime="codex", runner="pty", enabled=True)

            def _fake_call_daemon(req, *args, **kwargs):
                resp, _ = handle_request(DaemonRequest.model_validate(req))
                if resp.ok:
                    return {"ok": True, "result": resp.result}
                return {
                    "ok": False,
                    "error": {
                        "code": str(resp.error.code or ""),
                        "message": str(resp.error.message or ""),
                        "details": resp.error.details if isinstance(resp.error.details, dict) else {},
                    },
                }

            with mock.patch.dict(os.environ, {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "foreman1"}, clear=False), mock.patch(
                "cccc.ports.mcp.common._runtime_context",
                return_value=_RuntimeContext(home=os.environ["CCCC_HOME"], group_id=group_id, actor_id="foreman1"),
            ), mock.patch(
                "cccc.ports.mcp.common.call_daemon",
                side_effect=_fake_call_daemon,
            ):
                out = mcp_server.handle_tool_call(
                    "cccc_memory",
                    {
                        "action": "govern_experience",
                        "group_id": group_id,
                        "lifecycle_action": "merge",
                        "target_candidate_id": target_candidate_id,
                        "source_candidate_ids": [source_candidate_id],
                        "reason": "merge via MCP",
                        "by": "foreman1",
                    },
                )
            changed = out.get("changed_candidate_ids") if isinstance(out, dict) else []
            self.assertEqual(changed, [source_candidate_id, target_candidate_id])
        finally:
            cleanup()

    def test_mcp_memory_promote_requires_actor_identity(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_id = self._create_group_with_candidate()
            from cccc.ports.mcp import server as mcp_server
            from cccc.ports.mcp.common import MCPError, _RuntimeContext

            with mock.patch.dict(os.environ, {"CCCC_GROUP_ID": "", "CCCC_ACTOR_ID": ""}, clear=False), mock.patch(
                "cccc.ports.mcp.common._runtime_context",
                return_value=_RuntimeContext(home=os.environ["CCCC_HOME"], group_id="", actor_id=""),
            ):
                with self.assertRaises(MCPError) as raised:
                    mcp_server.handle_tool_call(
                        "cccc_memory",
                        {"action": "promote_experience", "group_id": group_id, "candidate_id": candidate_id},
                    )
            self.assertEqual(getattr(raised.exception, "code", ""), "missing_actor_id")
        finally:
            cleanup()

    def test_mcp_memory_govern_requires_actor_identity(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id, candidate_ids = self._create_group_with_two_candidates()
            target_candidate_id, source_candidate_id = candidate_ids[0], candidate_ids[1]
            from cccc.ports.mcp import server as mcp_server
            from cccc.ports.mcp.common import MCPError, _RuntimeContext

            with mock.patch.dict(os.environ, {"CCCC_GROUP_ID": "", "CCCC_ACTOR_ID": ""}, clear=False), mock.patch(
                "cccc.ports.mcp.common._runtime_context",
                return_value=_RuntimeContext(home=os.environ["CCCC_HOME"], group_id="", actor_id=""),
            ):
                with self.assertRaises(MCPError) as raised:
                    mcp_server.handle_tool_call(
                        "cccc_memory",
                        {
                            "action": "govern_experience",
                            "group_id": group_id,
                            "lifecycle_action": "merge",
                            "target_candidate_id": target_candidate_id,
                            "source_candidate_ids": [source_candidate_id],
                        },
                    )
            self.assertEqual(getattr(raised.exception, "code", ""), "missing_actor_id")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
