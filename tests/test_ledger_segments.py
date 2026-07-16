import tempfile
import unittest
import os
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

    def test_ensure_ledger_layout_does_not_touch_existing_active_ledger(self) -> None:
        from cccc.kernel.ledger_segments import active_ledger_path, ensure_ledger_layout

        with tempfile.TemporaryDirectory() as td:
            group_path = Path(td)
            ensure_ledger_layout(group_path)
            active = active_ledger_path(group_path)
            fixed_ns = 1_700_000_000_123_456_789
            os.utime(active, ns=(fixed_ns, fixed_ns))

            ensure_ledger_layout(group_path)

            self.assertEqual(active.stat().st_mtime_ns, fixed_ns)


if __name__ == "__main__":
    unittest.main()
