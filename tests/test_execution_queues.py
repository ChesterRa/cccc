from __future__ import annotations

import threading
import time
import unittest


class _Conn:
    def __init__(self) -> None:
        self.closed = False
        self.sent: list[dict] = []

    def close(self) -> None:
        self.closed = True


class TestExecutionQueues(unittest.TestCase):
    def test_request_execution_queue_processes_request_and_closes_connection(self) -> None:
        from cccc.contracts.v1 import DaemonResponse
        from cccc.daemon.ops.execution_queues import DaemonRequestExecutionQueue

        stop_event = threading.Event()
        handled: list[dict] = []
        exits: list[bool] = []
        conn = _Conn()

        queue = DaemonRequestExecutionQueue(
            stop_event=stop_event,
            handle_request=lambda req: (handled.append(req) or DaemonResponse(ok=True, result={"ok": True}), False),
            send_json=lambda queued_conn, payload: queued_conn.sent.append(payload),
            dump_response=lambda resp: resp.model_dump(),
            logger=__import__("logging").getLogger("test"),
            on_should_exit=lambda: exits.append(True),
        )
        thread = threading.Thread(target=queue.run_forever, daemon=True)
        thread.start()
        self.assertTrue(queue.submit(conn=conn, req={"op": "ping"}))
        for _ in range(50):
            if conn.closed:
                break
            time.sleep(0.01)
        stop_event.set()
        thread.join(timeout=1.0)

        self.assertEqual(handled, [{"op": "ping"}])
        self.assertTrue(conn.closed)
        self.assertTrue(conn.sent)
        self.assertEqual(exits, [])

    def test_group_space_sync_run_queue_dedupes_and_upgrades_force(self) -> None:
        from cccc.daemon.ops.execution_queues import GroupSpaceSyncRunQueue

        queue = GroupSpaceSyncRunQueue()
        first = queue.submit(group_id="g1", provider="notebooklm", force=False, by="user")
        second = queue.submit(group_id="g1", provider="notebooklm", force=True, by="peer1")
        ran: list[tuple[str, bool, str]] = []
        processed = queue.drain(
            limit=4,
            runner=lambda task: ran.append((task.group_id, bool(task.force), task.by)),
        )

        self.assertEqual(bool(first.get("queued")), True)
        self.assertEqual(bool(first.get("completed")), False)
        self.assertNotIn("completion_signal", first)
        self.assertNotIn("recommended_next_action", first)
        self.assertEqual(str(second.get("reason") or ""), "already_pending")
        self.assertEqual(bool(second.get("completed")), False)
        self.assertNotIn("completion_signal", second)
        self.assertEqual(processed, 1)
        self.assertEqual(ran, [("g1", True, "peer1")])

    def test_group_space_sync_run_queue_keeps_followup_when_already_running(self) -> None:
        from cccc.daemon.ops.execution_queues import GroupSpaceSyncRunQueue

        queue = GroupSpaceSyncRunQueue()
        first = queue.submit(group_id="g1", provider="notebooklm", force=False, by="user")
        first_runs: list[tuple[str, bool, str]] = []

        processed = queue.drain(
            limit=1,
            runner=lambda task: (
                first_runs.append((task.group_id, bool(task.force), task.by)),
                queue.submit(group_id="g1", provider="notebooklm", force=True, by="peer1"),
            ),
        )
        self.assertEqual(bool(first.get("queued")), True)
        self.assertEqual(processed, 1)
        self.assertEqual(first_runs, [("g1", False, "user")])

        second_runs: list[tuple[str, bool, str]] = []
        processed = queue.drain(
            limit=1,
            runner=lambda task: second_runs.append((task.group_id, bool(task.force), task.by)),
        )
        self.assertEqual(processed, 1)
        self.assertEqual(second_runs, [("g1", True, "peer1")])

    def test_group_space_sync_run_queue_upgrades_followup_force_while_running(self) -> None:
        from cccc.daemon.ops.execution_queues import GroupSpaceSyncRunQueue

        queue = GroupSpaceSyncRunQueue()
        queue.submit(group_id="g1", provider="notebooklm", force=False, by="user")
        observed: list[dict[str, object]] = []

        queue.drain(
            limit=1,
            runner=lambda _task: (
                observed.append(queue.submit(group_id="g1", provider="notebooklm", force=False, by="peer1")),
                observed.append(queue.submit(group_id="g1", provider="notebooklm", force=True, by="peer2")),
            ),
        )

        self.assertEqual(str(observed[0].get("reason") or ""), "queued_after_running")
        self.assertEqual(bool(observed[0].get("queued")), True)
        self.assertEqual(bool(observed[0].get("completed")), False)
        self.assertNotIn("completion_signal", observed[0])
        self.assertEqual(str(observed[1].get("reason") or ""), "already_pending")
        self.assertEqual(bool(observed[1].get("force")), True)

        reruns: list[tuple[str, bool, str]] = []
        queue.drain(
            limit=1,
            runner=lambda task: reruns.append((task.group_id, bool(task.force), task.by)),
        )
        self.assertEqual(reruns, [("g1", True, "peer2")])


if __name__ == "__main__":
    unittest.main()
