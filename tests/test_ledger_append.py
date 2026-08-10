import json
import tempfile
import unittest
from pathlib import Path


class TestLedgerAppend(unittest.TestCase):
    def test_append_separates_an_invalid_unterminated_tail(self) -> None:
        from cccc.kernel.ledger import append_event
        from cccc.kernel.ledger_index import lookup_event_by_id

        with tempfile.TemporaryDirectory() as raw_root:
            ledger = Path(raw_root) / "groups" / "g_test" / "ledger.jsonl"
            ledger.parent.mkdir(parents=True)
            partial = b'{"v":1,"id":"crash-tail"'
            ledger.write_bytes(partial)

            event = append_event(
                ledger,
                kind="chat.message",
                group_id="g_test",
                scope_key="",
                by="user",
                data={"text": "after crash", "to": ["user"]},
            )

            lines = ledger.read_bytes().splitlines()
            self.assertEqual(lines[0], partial)
            self.assertEqual(json.loads(lines[1])["id"], event["id"])
            self.assertEqual(lookup_event_by_id(ledger, event["id"])["id"], event["id"])

    def test_append_preserves_a_complete_unterminated_event(self) -> None:
        from cccc.kernel.ledger import append_event

        with tempfile.TemporaryDirectory() as raw_root:
            ledger = Path(raw_root) / "groups" / "g_test" / "ledger.jsonl"
            first = append_event(
                ledger,
                kind="chat.message",
                group_id="g_test",
                scope_key="",
                by="user",
                data={"text": "first", "to": ["user"]},
            )
            ledger.write_bytes(ledger.read_bytes().removesuffix(b"\n"))

            second = append_event(
                ledger,
                kind="chat.message",
                group_id="g_test",
                scope_key="",
                by="user",
                data={"text": "second", "to": ["user"]},
            )

            events = [json.loads(line) for line in ledger.read_bytes().splitlines()]
            self.assertEqual([event["id"] for event in events], [first["id"], second["id"]])


if __name__ == "__main__":
    unittest.main()
