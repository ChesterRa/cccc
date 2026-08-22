import unittest
from unittest.mock import patch


class TestCliMessageDeliveryControl(unittest.TestCase):
    def test_parser_exposes_deliver_and_cancel_reply(self) -> None:
        from cccc.cli.main import build_parser

        deliver = build_parser().parse_args(
            ["deliver", "event-1", "--to", "peer-1,peer-2", "--force-ambiguous"]
        )
        self.assertEqual(deliver.event_id, "event-1")
        self.assertEqual(deliver.to, ["peer-1,peer-2"])
        self.assertTrue(deliver.force_ambiguous)

        cancel = build_parser().parse_args(["cancel-reply", "event-2"])
        self.assertEqual(cancel.event_id, "event-2")

        reply = build_parser().parse_args(["reply", "event-3", "later", "--mode", "mail"])
        self.assertEqual(reply.mode, "mail")

    def test_commands_forward_existing_event_without_new_message_text(self) -> None:
        from cccc.cli.main import build_parser

        requests: list[dict] = []

        def fake_call(request: dict) -> dict:
            requests.append(request)
            return {"ok": True, "result": {}}

        with (
            patch("cccc.cli.messaging_cmds._resolve_group_id", return_value="g1"),
            patch("cccc.cli.messaging_cmds._ensure_daemon_running", return_value=True),
            patch("cccc.cli.messaging_cmds.call_daemon", side_effect=fake_call),
            patch("cccc.cli.messaging_cmds._print_json"),
        ):
            deliver = build_parser().parse_args(
                ["deliver", "event-1", "--to", "peer-1,peer-2", "--force-ambiguous"]
            )
            self.assertEqual(deliver.func(deliver), 0)
            cancel = build_parser().parse_args(["cancel-reply", "event-2"])
            self.assertEqual(cancel.func(cancel), 0)
            reply = build_parser().parse_args(["reply", "event-3", "later", "--mode", "mail"])
            self.assertEqual(reply.func(reply), 0)

        self.assertEqual(requests[0]["op"], "message_deliver")
        self.assertEqual(requests[0]["args"]["source_event_id"], "event-1")
        self.assertEqual(requests[0]["args"]["actor_ids"], ["peer-1", "peer-2"])
        self.assertTrue(requests[0]["args"]["force_ambiguous"])
        self.assertNotIn("text", requests[0]["args"])
        self.assertEqual(requests[1]["op"], "reply_request_cancel")
        self.assertEqual(requests[1]["args"]["source_event_id"], "event-2")
        self.assertEqual(requests[2]["op"], "reply")
        self.assertEqual(requests[2]["args"]["reply_to"], "event-3")
        self.assertEqual(requests[2]["args"]["message_mode"], "mail")


if __name__ == "__main__":
    unittest.main()
