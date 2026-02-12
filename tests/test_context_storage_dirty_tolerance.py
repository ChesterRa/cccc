import os
import tempfile
import unittest

import yaml


class TestContextStorageDirtyTolerance(unittest.TestCase):
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

    def _new_storage(self):
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.context import ContextStorage

        reg = load_registry()
        group = create_group(reg, title="context-dirty", topic="")
        return group, ContextStorage(group)

    def test_load_context_skips_bad_items_and_coerces_status(self) -> None:
        _, cleanup = self._with_home()
        try:
            group, storage = self._new_storage()
            storage._ensure_dirs()  # noqa: SLF001
            context_path = group.path / "context" / "context.yaml"
            context_path.write_text(
                yaml.safe_dump(
                    {
                        "vision": "v",
                        "meta": [],
                        "milestones": [
                            {"id": "M1", "name": "good", "status": "active"},
                            "not-a-map",
                            {"id": "M2", "name": "bad-status", "status": "unknown_status"},
                        ],
                        "notes": [{"id": "N1", "content": "ok"}, 123],
                        "references": [{"id": "R1", "url": "u", "note": "n"}, "bad"],
                    },
                    sort_keys=False,
                ),
                encoding="utf-8",
            )

            ctx = storage.load_context()
            self.assertEqual(ctx.vision, "v")
            self.assertEqual(len(ctx.milestones), 2)
            self.assertEqual(ctx.milestones[0].status.value, "active")
            self.assertEqual(ctx.milestones[1].status.value, "planned")
            self.assertEqual(len(ctx.notes), 1)
            self.assertEqual(len(ctx.references), 1)
            self.assertIsInstance(ctx.meta, dict)
            self.assertTrue(bool(ctx.meta))
        finally:
            cleanup()

    def test_load_task_tolerates_bad_step_status(self) -> None:
        _, cleanup = self._with_home()
        try:
            group, storage = self._new_storage()
            storage._ensure_dirs()  # noqa: SLF001
            task_path = group.path / "context" / "tasks" / "T001.yaml"
            task_path.write_text(
                yaml.safe_dump(
                    {
                        "id": "T001",
                        "name": "task",
                        "status": "unknown_status",
                        "steps": [
                            {"id": "S1", "name": "step1", "status": "broken_status"},
                            "bad",
                        ],
                    },
                    sort_keys=False,
                ),
                encoding="utf-8",
            )

            task = storage.load_task("T001")
            self.assertIsNotNone(task)
            assert task is not None
            self.assertEqual(task.status.value, "planned")
            self.assertEqual(len(task.steps), 1)
            self.assertEqual(task.steps[0].status.value, "pending")
        finally:
            cleanup()

    def test_load_presence_tolerates_bad_shape(self) -> None:
        _, cleanup = self._with_home()
        try:
            group, storage = self._new_storage()
            storage._ensure_dirs()  # noqa: SLF001
            presence_path = group.path / "context" / "presence.yaml"
            presence_path.write_text(
                yaml.safe_dump(
                    {
                        "agents": [{"id": "peer1", "status": "busy"}, "bad"],
                        "heartbeat_timeout_seconds": "oops",
                    },
                    sort_keys=False,
                ),
                encoding="utf-8",
            )

            presence = storage.load_presence()
            self.assertEqual(len(presence.agents), 1)
            self.assertEqual(presence.agents[0].id, "peer1")
            self.assertEqual(presence.heartbeat_timeout_seconds, 300)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
