import os
import tempfile
import threading
import unittest
from unittest.mock import patch


class TestContextFullSnapshotConsistency(unittest.TestCase):
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

    def test_full_read_waits_for_task_data_and_revision_commit(self) -> None:
        created = self.call("group_create", {"title": "full snapshot", "topic": "", "by": "user"})
        self.assertTrue(created.ok, created.error)
        group_id = str(created.result["group_id"])
        from cccc.kernel.context import ContextStorage

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
            if threading.current_thread().name == "full-reader":
                reader_scanned.set()
            return original_list(storage)

        with patch.object(ContextStorage, "save_task", autospec=True, side_effect=paused_save):
            writer = threading.Thread(
                target=lambda: writer_results.append(
                    self.call(
                        "context_sync",
                        {
                            "group_id": group_id,
                            "by": "user",
                            "ops": [{"op": "task.create", "title": "atomic full task"}],
                        },
                    )
                ),
                name="full-writer",
            )
            writer.start()
            self.assertTrue(task_written.wait(timeout=2.0))
            with patch.object(ContextStorage, "list_tasks", autospec=True, side_effect=observed_list):
                reader = threading.Thread(
                    target=lambda: reader_results.append(
                        self.call("context_get", {"group_id": group_id, "detail": "full"})
                    ),
                    name="full-reader",
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
        tasks = reader_results[0].result["coordination"]["tasks"]
        self.assertEqual([task["title"] for task in tasks], ["atomic full task"])
        from cccc.kernel.group import load_group

        storage = ContextStorage(load_group(group_id))
        self.assertEqual(
            reader_results[0].result["tasks_version"],
            f"tasksv:{storage.load_version_state()['tasks_rev']}",
        )


if __name__ == "__main__":
    unittest.main()
