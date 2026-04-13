import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest.mock import patch

from cccc.util.fs import atomic_write_json, atomic_write_text


class TestMcpExperienceRecall(unittest.TestCase):
    def test_query_reads_group_state_primary_and_sanitizes(self) -> None:
        from cccc.daemon.memory.experience_assets import query_experience_recall

        with TemporaryDirectory() as td:
            root = Path(td)
            state_dir = root / "state"
            state_dir.mkdir(parents=True, exist_ok=True)
            atomic_write_json(
                state_dir / "experience_candidates.json",
                {
                    "schema": 1,
                    "candidates": [
                        {
                            "id": "exp_promoted_1",
                            "status": "promoted",
                            "summary": "Use deterministic lifecycle state transitions.",
                            "score": 0.93,
                            "source_refs": ["decision:D001"],
                            "next_action": "do-not-leak",
                            "blockers": ["do-not-leak"],
                        },
                        {
                            "id": "exp_candidate_1",
                            "status": "proposed",
                            "summary": "Keep candidate compact.",
                            "score": 0.61,
                            "next_action": "do-not-leak",
                        },
                        {
                            "id": "exp_candidate_2",
                            "status": "rejected",
                            "summary": "This rejected candidate should not be recalled.",
                            "score": 0.99,
                        },
                    ],
                },
            )

            context = {
                "coordination": {
                    "tasks": [{"id": "T001", "title": "Ship recall", "status": "active"}],
                    "recent_decisions": [{"summary": "Freeze recall contract", "task_id": "T001", "by": "foreman"}],
                    "experience": {
                        "promoted": [{"id": "fallback_promoted", "summary": "fallback promoted", "score": 0.2}],
                    },
                }
            }
            memory_hits = [
                {"path": "/tmp/memory/MEMORY.md", "start_line": 12, "score": 0.88, "snippet": "Stable recall rule."}
            ]

            with patch("cccc.daemon.memory.experience_assets.load_group", return_value=SimpleNamespace(path=root)):
                out = query_experience_recall(
                    group_id="g_test",
                    query="recall lifecycle",
                    context=context,
                    memory_hits=memory_hits,
                )

        self.assertTrue(bool(out.get("has_any")))
        self.assertTrue(bool(out.get("has_high_relevance_promoted")))
        promoted = out["experience"]["promoted"]
        self.assertTrue(promoted)
        self.assertEqual(promoted[0]["id"], "exp_promoted_1")
        self.assertNotIn("next_action", promoted[0])
        self.assertNotIn("blockers", promoted[0])
        candidate_ids = [item["id"] for item in out["experience"]["candidates"]]
        self.assertEqual(candidate_ids, ["exp_candidate_1"])
        self.assertEqual(out["task_refs"][0]["id"], "T001")
        self.assertEqual(out["decision_refs"][0]["task_id"], "T001")
        self.assertEqual(out["memory_hits"][0]["path"], "/tmp/memory/MEMORY.md")

    def test_query_filters_retired_memory_block_and_retired_candidate_cross_check(self) -> None:
        from cccc.daemon.memory.experience_assets import query_experience_recall

        with TemporaryDirectory() as td:
            root = Path(td)
            state_dir = root / "state"
            memory_dir = state_dir / "memory"
            memory_dir.mkdir(parents=True, exist_ok=True)
            atomic_write_json(
                state_dir / "experience_candidates.json",
                {
                    "schema": 1,
                    "candidates": [
                        {
                            "id": "exp_active",
                            "status": "promoted_to_memory",
                            "summary": "Keep active experience.",
                            "score": 0.91,
                            "source_refs": ["decision:D100"],
                        },
                        {
                            "id": "exp_retired",
                            "status": "rejected",
                            "summary": "Do not recall retired experience.",
                            "score": 0.87,
                        },
                    ],
                },
            )
            memory_text = (
                '## expmem_exp_active [experience] 2026-04-11T00:00:00Z\n'
                '<!-- cccc.memory.meta {"entry_id":"expmem_exp_active","candidate_id":"exp_active","kind":"experience","lifecycle_state":"active","source_refs":["experience:exp_active"],"content_hash":"a","date":"2026-04-11","group_label":"g","actor_id":"user","created_at":"2026-04-11T00:00:00Z","tags":["experience","promoted"],"supersedes":[]} -->\n\n'
                'Experience: active\n\n'
                '## expmem_exp_retired [experience_retired] 2026-04-11T00:00:00Z\n'
                '<!-- cccc.memory.meta {"entry_id":"expmem_exp_retired","candidate_id":"exp_retired","kind":"experience_retired","lifecycle_state":"retired","source_refs":["experience:exp_retired"],"content_hash":"b","date":"2026-04-11","group_label":"g","actor_id":"user","created_at":"2026-04-11T00:00:00Z","tags":["experience","retired"],"supersedes":[]} -->\n\n'
                'Experience retired: retired\n\n'
            )
            memory_file = memory_dir / "MEMORY.md"
            atomic_write_text(memory_file, memory_text)

            with patch("cccc.daemon.memory.experience_assets.load_group", return_value=SimpleNamespace(path=root)):
                out = query_experience_recall(
                    group_id="g_test",
                    query="experience",
                    context={"coordination": {}},
                    memory_hits=[
                        {"path": str(memory_file), "start_line": 1, "score": 0.88, "snippet": "active"},
                        {"path": str(memory_file), "start_line": 6, "score": 0.89, "snippet": "retired"},
                    ],
                )

        self.assertEqual(len(out["memory_hits"]), 1)
        self.assertEqual(out["memory_hits"][0]["start_line"], 1)

    def test_query_rewrites_stale_memory_snippet_from_current_structured_block(self) -> None:
        from cccc.daemon.memory.experience_assets import query_experience_recall

        with TemporaryDirectory() as td:
            root = Path(td)
            state_dir = root / "state"
            memory_dir = state_dir / "memory"
            memory_dir.mkdir(parents=True, exist_ok=True)
            memory_file = memory_dir / "MEMORY.md"
            atomic_write_text(
                memory_file,
                '## expmem_exp_active [experience] 2026-04-11T00:00:00Z\n'
                '<!-- cccc.memory.meta {"entry_id":"expmem_exp_active","candidate_id":"exp_active","kind":"experience","lifecycle_state":"active","source_refs":["experience:exp_active"],"content_hash":"a","date":"2026-04-11","group_label":"g","actor_id":"user","created_at":"2026-04-11T00:00:00Z","tags":["experience","promoted"],"supersedes":[]} -->\n\n'
                'Experience: current structured content\n'
                '- candidate_id: exp_active\n\n',
            )
            atomic_write_json(
                state_dir / "experience_candidates.json",
                {"schema": 1, "candidates": [{"id": "exp_active", "status": "promoted_to_memory", "summary": "current structured content"}]},
            )

            with patch("cccc.daemon.memory.experience_assets.load_group", return_value=SimpleNamespace(path=root)):
                out = query_experience_recall(
                    group_id="g_test",
                    query="structured content",
                    context={"coordination": {}},
                    memory_hits=[
                        {"path": str(memory_file), "start_line": 1, "score": 0.88, "snippet": "legacy leaked snippet"},
                    ],
                )

        self.assertEqual(len(out["memory_hits"]), 1)
        self.assertNotEqual(out["memory_hits"][0]["snippet"], "legacy leaked snippet")
        self.assertIn("current structured content", out["memory_hits"][0]["snippet"])

    def test_query_filters_retired_candidates_in_fallback_branch(self) -> None:
        from cccc.daemon.memory.experience_assets import query_experience_recall

        with patch("cccc.daemon.memory.experience_assets.load_group", return_value=None):
            out = query_experience_recall(
                group_id="g_test",
                query="fallback experience",
                context={
                    "coordination": {
                        "experience": {
                            "promoted": [{"id": "p1", "status": "promoted", "summary": "keep me", "score": 0.8}],
                            "candidates": [
                                {"id": "c1", "status": "proposed", "summary": "keep me", "score": 0.7},
                                {"id": "c2", "status": "merged", "summary": "drop me", "score": 0.99},
                            ],
                        }
                    }
                },
                memory_hits=[],
            )

        self.assertEqual([item["id"] for item in out["experience"]["candidates"]], ["c1"])

    def test_query_returns_clean_empty_payload_when_no_hits(self) -> None:
        from cccc.daemon.memory.experience_assets import query_experience_recall

        with patch("cccc.daemon.memory.experience_assets.load_group", return_value=None):
            out = query_experience_recall(
                group_id="g_test",
                query="",
                context={},
                memory_hits=[],
            )

        self.assertFalse(bool(out.get("has_any")))
        self.assertFalse(bool(out.get("has_high_relevance_promoted")))
        self.assertEqual(out.get("memory_hits"), [])
        self.assertEqual(out.get("task_refs"), [])
        self.assertEqual(out.get("decision_refs"), [])
        self.assertEqual(out.get("experience", {}).get("promoted"), [])
        self.assertEqual(out.get("experience", {}).get("candidates"), [])
