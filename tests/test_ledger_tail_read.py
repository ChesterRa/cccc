import tempfile
import unittest
from pathlib import Path


class TestLedgerTailRead(unittest.TestCase):
    def test_read_last_lines_reads_tail_without_final_newline(self) -> None:
        from cccc.kernel.ledger import _read_last_lines_from_regular_file

        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "events.jsonl"
            lines = [f"line-{idx}-context-同步" for idx in range(200)]
            path.write_text("\n".join(lines), encoding="utf-8")

            tail = _read_last_lines_from_regular_file(path, 5, block_size=17)

        self.assertEqual(tail, lines[-5:])

    def test_read_last_lines_preserves_existing_blank_line_semantics(self) -> None:
        from cccc.kernel.ledger import read_last_lines

        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "daemon.log"
            path.write_text("one\n\n二\nthree\n", encoding="utf-8")

            tail = read_last_lines(path, 3)

        self.assertEqual(tail, ["one", "二", "three"])

    def test_read_last_lines_returns_empty_for_missing_file(self) -> None:
        from cccc.kernel.ledger import read_last_lines

        with tempfile.TemporaryDirectory() as td:
            tail = read_last_lines(Path(td) / "missing.log", 10)

        self.assertEqual(tail, [])


if __name__ == "__main__":
    unittest.main()
