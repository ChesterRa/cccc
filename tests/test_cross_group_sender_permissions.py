import os
import shutil
import tempfile
import unittest


class TestCrossGroupSenderPermissions(unittest.TestCase):
    def setUp(self) -> None:
        self.old_home = os.environ.get("CCCC_HOME")
        self.temp = tempfile.TemporaryDirectory()
        os.environ["CCCC_HOME"] = self.temp.name

    def tearDown(self) -> None:
        try:
            self.temp.cleanup()
        except OSError:
            shutil.rmtree(self.temp.name, ignore_errors=True)
        if self.old_home is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = self.old_home

    def call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))[0]

    def create_group(self, title: str) -> str:
        response = self.call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(response.ok, response.error)
        return str(response.result["group_id"])

    def add_actor(self, group_id: str, actor_id: str) -> None:
        response = self.call(
            "actor_add",
            {"group_id": group_id, "actor_id": actor_id, "runtime": "codex", "by": "user"},
        )
        self.assertTrue(response.ok, response.error)

    def ledger_size(self, group_id: str) -> int:
        from cccc.kernel.group import load_group
        from cccc.kernel.inbox import iter_events

        group = load_group(group_id)
        self.assertIsNotNone(group)
        return len(list(iter_events(group.ledger_path)))

    def test_peer_can_send_but_unknown_actor_cannot_write_either_ledger(self) -> None:
        source = self.create_group("source")
        destination = self.create_group("destination")
        self.add_actor(source, "lead")
        self.add_actor(source, "peer")
        self.add_actor(destination, "destination-lead")

        peer_response = self.call(
            "send_cross_group",
            {
                "group_id": source,
                "dst_group_id": destination,
                "by": " peer ",
                "to": ["destination-lead"],
                "text": "valid peer message",
                "message_mode": "mail",
            },
        )
        self.assertTrue(peer_response.ok, peer_response.error)
        self.assertEqual(peer_response.result["src_event"]["by"], "peer")

        source_size = self.ledger_size(source)
        destination_size = self.ledger_size(destination)
        forged_response = self.call(
            "send_cross_group",
            {
                "group_id": source,
                "dst_group_id": destination,
                "by": "forged-actor",
                "to": ["destination-lead"],
                "text": "forged message",
                "message_mode": "send",
            },
        )

        self.assertFalse(forged_response.ok)
        self.assertEqual(forged_response.error.code, "permission_denied")
        self.assertEqual(forged_response.error.message, "unknown actor: forged-actor")
        self.assertEqual(self.ledger_size(source), source_size)
        self.assertEqual(self.ledger_size(destination), destination_size)


if __name__ == "__main__":
    unittest.main()
