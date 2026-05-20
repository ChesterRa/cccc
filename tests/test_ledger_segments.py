import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestLedgerSegments(unittest.TestCase):
    def test_list_ledger_sources_does_not_read_known_segments_for_line_counts(self) -> None:
        from cccc.kernel.ledger_segments import list_ledger_sources, save_ledger_manifest

        with tempfile.TemporaryDirectory() as td:
            group_path = Path(td)
            segment_rel = "state/ledger/segments/ledger.20260520T000000Z.000001.jsonl.gz"
            segment_path = group_path / segment_rel
            segment_path.parent.mkdir(parents=True, exist_ok=True)
            segment_path.write_bytes(b"not actually gzip")
            save_ledger_manifest(
                group_path,
                {
                    "next_segment_seq": 2,
                    "segments": [
                        {
                            "id": "000001",
                            "seq": 1,
                            "path": segment_rel,
                            "compressed": True,
                            "created_at": "20260520T000000Z",
                            "sealed_at": "20260520T000000Z",
                            "reason": "test",
                            "size_bytes": segment_path.stat().st_size,
                            "line_count": 123,
                        }
                    ],
                },
            )

            with patch("cccc.kernel.ledger_segments.open_ledger_source_text") as open_source:
                sources = list_ledger_sources(group_path)

            open_source.assert_not_called()
            self.assertEqual([str(source.get("path") or "") for source in sources], [segment_rel, "ledger.jsonl"])


if __name__ == "__main__":
    unittest.main()
