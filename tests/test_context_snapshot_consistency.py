import os
import tempfile
import threading
import unittest
from unittest.mock import patch


class TestContextSnapshotConsistency(unittest.TestCase):
    def setUp(self) -> None:
        self.old_home = os.environ.get("CCCC_HOME")
        self.temp = tempfile.TemporaryDirectory()
        os.environ["CCCC_HOME"] = self.temp.name

    def tearDown(self) -> None:
        self.temp.cleanup()
        if self.old_home is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = self.old_home

    def call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))[0]

    def create_group(self) -> str:
        response = self.call(
            "group_create", {"title": "snapshot tests", "topic": "", "by": "user"}
        )
        self.assertTrue(response.ok, response.error)
        return response.result["group_id"]

    def test_task_list_waits_until_task_data_and_revision_are_committed(self) -> None:
        group_id = self.create_group()
        from cccc.kernel.context import ContextStorage
        from cccc.kernel.group import load_group

        task_written = threading.Event()
        release_writer = threading.Event()
        reader_scanned = threading.Event()
        writer_results = []
        reader_results = []
        original_save = ContextStorage.save_task
        original_list = ContextStorage.list_tasks

        def paused_save(storage, task):
            original_save(storage, task)
            task_written.set()
            self.assertTrue(release_writer.wait(timeout=2.0))

        def observed_list(storage):
            if threading.current_thread().name == "task-reader":
                reader_scanned.set()
            return original_list(storage)

        with patch.object(
            ContextStorage, "save_task", autospec=True, side_effect=paused_save
        ):
            writer = threading.Thread(
                target=lambda: writer_results.append(
                    self.call(
                        "context_sync",
                        {
                            "group_id": group_id,
                            "by": "user",
                            "ops": [{"op": "task.create", "title": "atomic task"}],
                        },
                    )
                ),
                name="task-writer",
            )
            writer.start()
            self.assertTrue(task_written.wait(timeout=2.0))
            with patch.object(
                ContextStorage, "list_tasks", autospec=True, side_effect=observed_list
            ):
                reader = threading.Thread(
                    target=lambda: reader_results.append(
                        self.call(
                            "task_list",
                            {"group_id": group_id, "status": "planned", "limit": "30"},
                        )
                    ),
                    name="task-reader",
                )
                reader.start()
                try:
                    self.assertFalse(reader_scanned.wait(timeout=0.1))
                finally:
                    release_writer.set()
                writer.join(timeout=2.0)
                reader.join(timeout=2.0)

        self.assertFalse(writer.is_alive())
        self.assertFalse(reader.is_alive())
        self.assertTrue(writer_results[0].ok, writer_results[0].error)
        self.assertTrue(reader_results[0].ok, reader_results[0].error)
        self.assertEqual(reader_results[0].result["tasks"][0]["title"], "atomic task")
        storage = ContextStorage(load_group(group_id))
        expected = f"tasksv:{storage.load_version_state()['tasks_rev']}"
        self.assertEqual(reader_results[0].result["tasks_version"], expected)

    def test_overview_waits_until_context_data_and_revision_are_committed(self) -> None:
        group_id = self.create_group()
        from cccc.kernel.context import ContextStorage

        context_written = threading.Event()
        release_writer = threading.Event()
        reader_loaded = threading.Event()
        writer_results = []
        reader_results = []
        original_save = ContextStorage.save_context
        original_load = ContextStorage.load_context

        def paused_save(storage, context):
            original_save(storage, context)
            context_written.set()
            self.assertTrue(release_writer.wait(timeout=2.0))

        def observed_load(storage):
            if threading.current_thread().name == "overview-reader":
                reader_loaded.set()
            return original_load(storage)

        with patch.object(
            ContextStorage, "save_context", autospec=True, side_effect=paused_save
        ):
            writer = threading.Thread(
                target=lambda: writer_results.append(
                    self.call(
                        "context_sync",
                        {
                            "group_id": group_id,
                            "by": "user",
                            "ops": [
                                {
                                    "op": "coordination.brief.update",
                                    "objective": "atomic overview",
                                }
                            ],
                        },
                    )
                ),
                name="context-writer",
            )
            writer.start()
            self.assertTrue(context_written.wait(timeout=2.0))
            with patch.object(
                ContextStorage, "load_context", autospec=True, side_effect=observed_load
            ):
                reader = threading.Thread(
                    target=lambda: reader_results.append(
                        self.call(
                            "context_get",
                            {"group_id": group_id, "detail": "overview"},
                        )
                    ),
                    name="overview-reader",
                )
                reader.start()
                try:
                    self.assertFalse(reader_loaded.wait(timeout=0.1))
                finally:
                    release_writer.set()
                writer.join(timeout=2.0)
                reader.join(timeout=2.0)

        self.assertFalse(writer.is_alive())
        self.assertFalse(reader.is_alive())
        self.assertTrue(writer_results[0].ok, writer_results[0].error)
        self.assertTrue(reader_results[0].ok, reader_results[0].error)
        self.assertEqual(
            reader_results[0].result["coordination"]["brief"]["objective"],
            "atomic overview",
        )
        self.assertEqual(
            reader_results[0].result["version"], writer_results[0].result["version"]
        )

    def test_legacy_summary_snapshot_gets_tasks_version_before_rebuild(self) -> None:
        group_id = self.create_group()
        from cccc.kernel.context import ContextStorage
        from cccc.kernel.group import load_group

        storage = ContextStorage(load_group(group_id))
        storage.save_summary_snapshot(
            basis=storage.summary_basis(),
            version=storage.compute_version(),
            result={
                "version": storage.compute_version(),
                "coordination": {"tasks": []},
            },
        )

        with patch(
            "cccc.daemon.context.context_ops._schedule_summary_snapshot_rebuild",
            return_value=True,
        ) as scheduled:
            response = self.call(
                "context_get", {"group_id": group_id, "detail": "summary"}
            )

        self.assertTrue(response.ok, response.error)
        self.assertEqual(response.result["tasks_version"], "tasksv:0")
        self.assertEqual(response.result["meta"]["summary_snapshot"]["state"], "stale")
        scheduled.assert_called_once_with(group_id)

if __name__ == "__main__":
    unittest.main()
