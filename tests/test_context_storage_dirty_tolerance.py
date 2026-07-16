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

    def test_load_context_coerces_dirty_data(self) -> None:
        _, cleanup = self._with_home()
        try:
            group, storage = self._new_storage()
            storage._ensure_dirs()  # noqa: SLF001
            context_path = group.path / "context" / "context.yaml"
            context_path.write_text(
                yaml.safe_dump(
                    {
                        "coordination": {
                            "brief": {
                                "objective": "v",
                                "current_focus": "testing",
                                "constraints": ["lead", "peer"],
                            }
                        },
                        "meta": [],
                    },
                    sort_keys=False,
                ),
                encoding="utf-8",
            )

            ctx = storage.load_context()
            self.assertEqual(ctx.coordination.brief.objective, "v")
            self.assertEqual(ctx.coordination.brief.constraints, ["lead", "peer"])
            self.assertEqual(ctx.coordination.brief.current_focus, "testing")
            # meta was invalid (list instead of dict), should be replaced with default
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
                        "title": "task",
                        "status": "unknown_status",
                        "checklist": [
                            {"id": "S1", "text": "step1", "status": "broken_status"},
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
            self.assertEqual(len(task.checklist), 1)
            self.assertEqual(task.checklist[0].status.value, "pending")
        finally:
            cleanup()

    def test_load_agents_tolerates_bad_shape(self) -> None:
        _, cleanup = self._with_home()
        try:
            group, storage = self._new_storage()
            storage._ensure_dirs()  # noqa: SLF001
            agents_path = group.path / "context" / "agents.yaml"
            agents_path.write_text(
                yaml.safe_dump(
                    {
                        "agent_states": [{"actor_id": "peer1", "hot": {"focus": "busy"}}, "bad"],
                    },
                    sort_keys=False,
                ),
                encoding="utf-8",
            )

            agents_state = storage.load_agents()
            self.assertEqual(len(agents_state.agents), 1)
            self.assertEqual(agents_state.agents[0].id, "peer1")
            self.assertEqual(agents_state.agents[0].hot.focus, "busy")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
