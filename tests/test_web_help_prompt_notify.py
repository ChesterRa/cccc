import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebHelpPromptNotify(unittest.TestCase):
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

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

    def _seed_group(self, *, runner: str = "headless") -> str:
        create, _ = self._call("group_create", {"title": "help-notify", "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        group_id = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)

        for actor_id, title in (("fm1", "Foreman"), ("peer1", "Peer 1"), ("peer2", "Peer 2")):
            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "title": title,
                    "runtime": "codex",
                    "runner": runner,
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))
        return group_id

    def test_structured_common_update_notifies_all_running_actors(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            group_id = self._seed_group()
            notify_calls: list[dict] = []

            def fake_daemon(req: dict):
                op = str(req.get("op") or "")
                if op == "actor_list":
                    return {
                        "ok": True,
                        "result": {
                            "actors": [
                                {"id": "fm1", "role": "foreman", "running": True},
                                {"id": "peer1", "role": "peer", "running": True},
                                {"id": "peer2", "role": "peer", "running": False},
                            ]
                        },
                    }
                if op == "system_notify":
                    notify_calls.append(req)
                return self._local_call_daemon(req)

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_daemon):
                client = TestClient(create_app())
                resp = client.put(
                    f"/api/v1/groups/{group_id}/prompts/help",
                    json={
                        "by": "user",
                        "content": "Shared guidance changed.\n",
                        "editor_mode": "structured",
                        "changed_blocks": ["common"],
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("ok"))
            result = body.get("result") or {}
            self.assertEqual(result.get("notified_actor_ids"), ["fm1", "peer1"])
            targets = sorted(str((call.get("args") or {}).get("target_actor_id") or "") for call in notify_calls)
            self.assertEqual(targets, ["fm1", "peer1"])
            titles = sorted(str((call.get("args") or {}).get("title") or "") for call in notify_calls)
            self.assertEqual(titles, ["Help updated: common guidance", "Help updated: common guidance"])
            for call in notify_calls:
                args = call.get("args") or {}
                self.assertIn("Updated: common guidance.", str(args.get("message") or ""))
        finally:
            cleanup()

    def test_structured_actor_update_notifies_only_running_target_actor(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            group_id = self._seed_group()
            notify_calls: list[dict] = []

            def fake_daemon(req: dict):
                op = str(req.get("op") or "")
                if op == "actor_list":
                    return {
                        "ok": True,
                        "result": {
                            "actors": [
                                {"id": "fm1", "role": "foreman", "running": True},
                                {"id": "peer1", "role": "peer", "running": True},
                                {"id": "peer2", "role": "peer", "running": False},
                            ]
                        },
                    }
                if op == "system_notify":
                    notify_calls.append(req)
                return self._local_call_daemon(req)

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_daemon):
                client = TestClient(create_app())
                resp = client.put(
                    f"/api/v1/groups/{group_id}/prompts/help",
                    json={
                        "by": "user",
                        "content": "## @actor: fm1\n\nStay skeptical.\n",
                        "editor_mode": "structured",
                        "changed_blocks": ["actor:fm1"],
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("ok"))
            result = body.get("result") or {}
            self.assertEqual(result.get("notified_actor_ids"), ["fm1"])
            self.assertEqual(len(notify_calls), 1)
            notify_args = notify_calls[0].get("args") or {}
            self.assertEqual(str(notify_args.get("target_actor_id") or ""), "fm1")
            self.assertEqual(str(notify_args.get("title") or ""), "Help updated: your actor note")
            self.assertIn("Updated: your actor note.", str(notify_args.get("message") or ""))
        finally:
            cleanup()

    def test_structured_help_update_notifies_only_running_target_scope(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            group_id = self._seed_group()
            notify_calls: list[dict] = []

            def fake_daemon(req: dict):
                op = str(req.get("op") or "")
                if op == "actor_list":
                    return {
                        "ok": True,
                        "result": {
                            "actors": [
                                {"id": "fm1", "role": "foreman", "running": True},
                                {"id": "peer1", "role": "peer", "running": True},
                                {"id": "peer2", "role": "peer", "running": False},
                            ]
                        },
                    }
                if op == "system_notify":
                    notify_calls.append(req)
                return self._local_call_daemon(req)

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_daemon):
                client = TestClient(create_app())
                resp = client.put(
                    f"/api/v1/groups/{group_id}/prompts/help",
                    json={
                        "by": "user",
                        "content": "## @role: peer\n\nReport sharply.\n",
                        "editor_mode": "structured",
                        "changed_blocks": ["role:peer"],
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("ok"))
            result = body.get("result") or {}
            self.assertEqual(result.get("notified_actor_ids"), ["peer1"])
            self.assertEqual(len(notify_calls), 1)
            notify_args = notify_calls[0].get("args") or {}
            self.assertEqual(str(notify_args.get("target_actor_id") or ""), "peer1")
            self.assertEqual(str(notify_args.get("by") or ""), "system")
            self.assertEqual(str(notify_args.get("title") or ""), "Help updated: peer notes")
            self.assertIn("Updated: peer notes.", str(notify_args.get("message") or ""))
        finally:
            cleanup()

    def test_structured_voice_secretary_update_notifies_only_voice_secretary(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            group_id = self._seed_group()
            notify_calls: list[dict] = []

            def fake_daemon(req: dict):
                op = str(req.get("op") or "")
                if op == "actor_list":
                    return {
                        "ok": True,
                        "result": {
                            "actors": [
                                {"id": "fm1", "role": "foreman", "running": True},
                                {"id": "peer1", "role": "peer", "running": True},
                                {
                                    "id": "voice-secretary",
                                    "role": "voice_secretary",
                                    "internal_kind": "voice_secretary",
                                    "running": True,
                                },
                            ]
                        },
                    }
                if op == "system_notify":
                    notify_calls.append(req)
                return self._local_call_daemon(req)

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_daemon):
                client = TestClient(create_app())
                resp = client.put(
                    f"/api/v1/groups/{group_id}/prompts/help",
                    json={
                        "by": "user",
                        "content": "## @voice_secretary\n\nKeep Secretary guidance concise.\n",
                        "editor_mode": "structured",
                        "changed_blocks": ["voice_secretary"],
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("ok"))
            result = body.get("result") or {}
            self.assertEqual(result.get("notified_actor_ids"), ["voice-secretary"])
            self.assertEqual(len(notify_calls), 1)
            notify_args = notify_calls[0].get("args") or {}
            self.assertEqual(str(notify_args.get("target_actor_id") or ""), "voice-secretary")
            self.assertEqual(str(notify_args.get("title") or ""), "Help updated: Voice Secretary guidance")
            self.assertIn("Updated: Voice Secretary guidance.", str(notify_args.get("message") or ""))
        finally:
            cleanup()

    def test_raw_help_update_notifies_all_running_actors(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            group_id = self._seed_group()
            notify_calls: list[dict] = []

            def fake_daemon(req: dict):
                op = str(req.get("op") or "")
                if op == "actor_list":
                    return {
                        "ok": True,
                        "result": {
                            "actors": [
                                {"id": "fm1", "role": "foreman", "running": True},
                                {"id": "peer1", "role": "peer", "running": True},
                                {"id": "peer2", "role": "peer", "running": False},
                            ]
                        },
                    }
                if op == "system_notify":
                    notify_calls.append(req)
                return self._local_call_daemon(req)

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_daemon):
                client = TestClient(create_app())
                resp = client.put(
                    f"/api/v1/groups/{group_id}/prompts/help",
                    json={
                        "by": "user",
                        "content": "# Team Help\n\nEverything changed.\n",
                        "editor_mode": "raw",
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("ok"))
            result = body.get("result") or {}
            self.assertEqual(result.get("notified_actor_ids"), ["fm1", "peer1"])
            targets = sorted(str((call.get("args") or {}).get("target_actor_id") or "") for call in notify_calls)
            self.assertEqual(targets, ["fm1", "peer1"])
            titles = sorted(str((call.get("args") or {}).get("title") or "") for call in notify_calls)
            self.assertEqual(titles, ["Help updated", "Help updated"])
        finally:
            cleanup()

    def test_help_update_dispatches_notify_to_running_pty_target(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            group_id = self._seed_group(runner="pty")

            def fake_daemon(req: dict):
                op = str(req.get("op") or "")
                if op == "actor_list":
                    return {
                        "ok": True,
                        "result": {
                            "actors": [
                                {"id": "fm1", "role": "foreman", "running": True},
                                {"id": "peer1", "role": "peer", "running": False},
                                {"id": "peer2", "role": "peer", "running": False},
                            ]
                        },
                    }
                return self._local_call_daemon(req)

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_daemon), patch(
                "cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running", return_value=True
            ), patch("cccc.daemon.messaging.delivery.queue_system_notify") as queue_mock, patch(
                "cccc.daemon.messaging.delivery.flush_pending_messages", return_value=True
            ) as flush_mock:
                client = TestClient(create_app())
                resp = client.put(
                    f"/api/v1/groups/{group_id}/prompts/help",
                    json={
                        "by": "user",
                        "content": "## @actor: fm1\n\nStay skeptical.\n",
                        "editor_mode": "structured",
                        "changed_blocks": ["actor:fm1"],
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("ok"))
            result = body.get("result") or {}
            self.assertEqual(result.get("notified_actor_ids"), ["fm1"])
            queue_mock.assert_called_once()
            queue_kwargs = queue_mock.call_args.kwargs
            self.assertEqual(queue_kwargs.get("actor_id"), "fm1")
            self.assertEqual(queue_kwargs.get("title"), "Help updated: your actor note")
            self.assertIn("Updated: your actor note.", str(queue_kwargs.get("message") or ""))
            flush_mock.assert_called_once()
            self.assertEqual(flush_mock.call_args.kwargs.get("actor_id"), "fm1")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
