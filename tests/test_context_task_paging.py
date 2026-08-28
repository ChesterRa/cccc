import os
import tempfile
import unittest
from unittest.mock import patch


class TestContextTaskPaging(unittest.TestCase):
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
            "group_create", {"title": "paged tasks", "topic": "", "by": "user"}
        )
        self.assertTrue(response.ok, response.error)
        return response.result["group_id"]

    def seed(self, group_id: str) -> None:
        from cccc.kernel.context import ContextStorage, Task
        from cccc.kernel.group import load_group

        storage = ContextStorage(load_group(group_id))
        for index in range(1, 36):
            storage.save_task(
                Task(
                    id=f"T{index:03}",
                    title=f"planned {index:02}",
                    assignee="peer" if index % 2 == 0 else None,
                )
            )
        storage.bump_version_state(tasks_changed=True)

    def test_overview_skips_tasks_and_exposes_task_revision(self) -> None:
        group_id = self.create_group()
        self.seed(group_id)
        from cccc.kernel.context import ContextStorage

        with patch.object(
            ContextStorage,
            "list_tasks",
            side_effect=AssertionError("overview must not list task files"),
        ):
            response = self.call(
                "context_get", {"group_id": group_id, "detail": "overview"}
            )

        self.assertTrue(response.ok, response.error)
        self.assertNotIn("tasks", response.result["coordination"])
        self.assertNotIn("tasks_summary", response.result)
        self.assertTrue(response.result["tasks_version"].startswith("tasksv:"))

    def test_batch_pages_scan_once_and_keep_an_unfiltered_index(self) -> None:
        group_id = self.create_group()
        self.seed(group_id)
        from cccc.kernel.context import ContextStorage

        original = ContextStorage.list_tasks
        with patch.object(
            ContextStorage, "list_tasks", autospec=True, side_effect=original
        ) as listed:
            response = self.call(
                "task_list",
                {
                    "group_id": group_id,
                    "statuses": "planned,active,done",
                    "query": "planned 01",
                    "limit": "30",
                    "include_index": "true",
                },
            )

        self.assertTrue(response.ok, response.error)
        self.assertEqual(listed.call_count, 1)
        self.assertEqual(response.result["pages"]["planned"]["count"], 1)
        self.assertEqual(response.result["pages"]["active"]["count"], 0)
        self.assertEqual(len(response.result["task_index"]), 35)
        self.assertTrue(response.result["tasks_version"].startswith("tasksv:"))

        exact = self.call("task_list", {"group_id": group_id, "task_id": "T001"})
        batch = self.call(
            "task_list", {"group_id": group_id, "task_ids": "T003,T001,T404"}
        )
        self.assertTrue(exact.result["delete_info"]["allowed"])
        self.assertEqual(
            [task["id"] for task in batch.result["tasks"]], ["T003", "T001"]
        )

if __name__ == "__main__":
    unittest.main()
