import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from unittest.mock import patch


class TestGroupBridgeReplyRelay(unittest.TestCase):
    def _isolated_home(self, td: str) -> ExitStack:
        stack = ExitStack()
        home = Path(td)
        stack.enter_context(patch("cccc.kernel.group_bridge.pairing.ensure_home", return_value=home))
        stack.enter_context(patch("cccc.kernel.group_bridge.registration.ensure_home", return_value=home))
        stack.enter_context(patch("cccc.kernel.group_bridge.peer_addresses.ensure_home", return_value=home))
        return stack

    def test_missing_group_bridge_reply_route_returns_diagnostic_error(self) -> None:
        from cccc.daemon.group_bridge.reply_relay import relay_group_bridge_reply

        with tempfile.TemporaryDirectory() as td, self._isolated_home(td):
            resp = relay_group_bridge_reply(
                group_id="g_cross",
                original_data={
                    "source_platform": "group_bridge_session",
                    "source_user_id": "peer-main",
                    "src_group_id": "g_main",
                    "src_event_id": "remote-event-1",
                },
                reply_event_id="reply-event-1",
                text="answer",
                by="user",
                to=["user"],
                priority="normal",
                reply_required=False,
                refs=[],
            )

        self.assertIsNotNone(resp)
        assert resp is not None
        self.assertFalse(resp.ok)
        self.assertIsNotNone(resp.error)
        assert resp.error is not None
        self.assertEqual(resp.error.code, "group_bridge_reply_route_not_found")
        self.assertEqual(resp.error.details["remote_group_id"], "g_main")
        self.assertEqual(resp.error.details["remote_peer_id"], "peer-main")

    def test_missing_reply_route_returns_diagnostic_error(self) -> None:
        from cccc.daemon.group_bridge.reply_relay import relay_group_bridge_reply

        resp = relay_group_bridge_reply(
            group_id="g_cross",
            original_data={
                "source_platform": "group_bridge_session",
                "source_user_id": "peer-main",
                "src_group_id": "g_main",
                "src_event_id": "remote-event-1",
            },
            reply_event_id="reply-event-1",
            text="answer",
            by="user",
            to=["user"],
            priority="normal",
            reply_required=False,
            refs=[],
        )

        self.assertIsNotNone(resp)
        assert resp is not None
        self.assertFalse(resp.ok)
        self.assertIsNotNone(resp.error)
        assert resp.error is not None
        self.assertEqual(resp.error.code, "group_bridge_reply_route_not_found")
        self.assertEqual(resp.error.details["remote_group_id"], "g_main")
        self.assertEqual(resp.error.details["source_platform"], "group_bridge_session")

    def test_session_reply_route_without_endpoint_is_sendable_when_active_session_exists(self) -> None:
        import asyncio

        from cccc.daemon.group_bridge.reply_relay import group_bridge_reply_registration_id
        from cccc.daemon.group_bridge.ws_session import GroupBridgeWsSession, clear_sessions, register_session
        from cccc.kernel.group_bridge import pairing as pairing_kernel

        async def send_request(_request, _timeout):
            return {"ok": True, "event_id": "remote-event"}

        original_data = {
            "source_platform": "group_bridge_session",
            "source_user_id": "peer-main",
            "src_group_id": "g_main",
            "src_event_id": "remote-event-1",
        }

        with tempfile.TemporaryDirectory() as td, self._isolated_home(td):
            invite = pairing_kernel.create_pairing_invite(group_id="g_cross")
            request = pairing_kernel.create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_main",
                requester_peer_id="peer-main",
            )
            approved = pairing_kernel.approve_pairing_request(request["request_id"], approver_user_id="user-a")

            clear_sessions()
            asyncio.run(
                register_session(
                    GroupBridgeWsSession(
                        target_group_id="g_cross",
                        src_group_id="g_main",
                        remote_peer_id="peer-main",
                        send_request=send_request,
                    )
                )
            )
            try:
                registration_id = group_bridge_reply_registration_id(group_id="g_cross", original_data=original_data)
            finally:
                clear_sessions()

        self.assertEqual(registration_id, approved["registration"]["registration_id"])

    def test_session_reply_route_without_endpoint_is_sendable_for_retry_without_local_session(self) -> None:
        from cccc.daemon.group_bridge.reply_relay import group_bridge_reply_registration_id
        from cccc.daemon.group_bridge.ws_session import clear_sessions
        from cccc.kernel.group_bridge import pairing as pairing_kernel

        original_data = {
            "source_platform": "group_bridge_session",
            "source_user_id": "peer-main",
            "src_group_id": "g_main",
            "src_event_id": "remote-event-1",
        }

        with tempfile.TemporaryDirectory() as td, self._isolated_home(td):
            invite = pairing_kernel.create_pairing_invite(group_id="g_cross")
            request = pairing_kernel.create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_main",
                requester_peer_id="peer-main",
            )
            approved = pairing_kernel.approve_pairing_request(request["request_id"], approver_user_id="user-a")

            clear_sessions()
            registration_id = group_bridge_reply_registration_id(group_id="g_cross", original_data=original_data)

        self.assertEqual(registration_id, approved["registration"]["registration_id"])

    def test_http_inbound_source_multiaddrs_updates_address_book_only(self) -> None:
        import os

        from cccc.daemon.messaging.chat_ops import handle_send
        from cccc.kernel.group_bridge import pairing as pairing_kernel
        from cccc.kernel.group_bridge.peer_addresses import resolve_peer_multiaddrs
        from cccc.kernel.group_bridge.registration import list_registrations
        import yaml

        old_home = os.environ.get("CCCC_HOME")
        with tempfile.TemporaryDirectory() as td, self._isolated_home(td):
            os.environ["CCCC_HOME"] = td
            try:
                group_dir = Path(td) / "groups" / "g_cross"
                group_dir.mkdir(parents=True)
                (group_dir / "context").mkdir()
                (group_dir / "state").mkdir()
                (group_dir / "group.yaml").write_text(
                    yaml.safe_dump(
                        {
                            "v": 1,
                            "group_id": "g_cross",
                            "title": "cross",
                            "state": "active",
                            "active_scope_key": "",
                            "actors": [
                                {"id": "foreman", "title": "foreman", "role": "foreman", "enabled": True}
                            ],
                        },
                        sort_keys=False,
                    ),
                    encoding="utf-8",
                )
                invite = pairing_kernel.create_pairing_invite(group_id="g_cross")
                request = pairing_kernel.create_pairing_request(
                    invite["pairing_code"],
                    requester_group_id="g_main",
                    requester_peer_id="peer-main",
                    requester_endpoint="http://remote.example:8848",
                    requester_multiaddrs=["/ip4/127.0.0.1/tcp/4001/p2p/peer-main"],
                )
                approved = pairing_kernel.approve_pairing_request(request["request_id"], approver_user_id="user-a")

                resp = handle_send(
                    {
                        "group_id": "g_cross",
                        "text": "hello",
                        "to": ["@foreman"],
                        "source_platform": "group_bridge_session",
                        "source_user_id": "peer-main",
                        "src_group_id": "g_main",
                        "src_event_id": "remote-event-1",
                        "source_multiaddrs": ["/ip4/127.0.0.1/tcp/5001/p2p/peer-main"],
                        "client_id": "remote-event-1",
                    },
                    coerce_bool=bool,
                    normalize_attachments=lambda raw, group: [],
                    effective_runner_kind=lambda group_id: "pty",
                    auto_wake_recipients=lambda group, to, by: [],
                    automation_on_resume=lambda group: None,
                    automation_on_new_message=lambda group: None,
                    clear_pending_system_notifies=lambda group_id, actors: None,
                )

                self.assertTrue(resp.ok)
                registrations = list_registrations(home=Path(td))
                self.assertEqual(registrations[0]["registration_id"], approved["registration"]["registration_id"])
                self.assertEqual(registrations[0]["transport"], "group_bridge_session")
                self.assertEqual(registrations[0]["multiaddrs"], [])
                trust = pairing_kernel.list_trusts(group_id="g_cross", home=Path(td))[0]
                self.assertEqual(trust["transport"], "group_bridge_session")
                self.assertEqual(trust["multiaddrs"], [])
                self.assertEqual(
                    resolve_peer_multiaddrs("peer-main", remote_group_id="g_main", home=Path(td)),
                    ("/ip4/127.0.0.1/tcp/5001/p2p/peer-main",),
                )
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
