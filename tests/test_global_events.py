import json
import os
import tempfile
import unittest
from pathlib import Path


class TestGlobalEvents(unittest.TestCase):
    def test_publish_event_appends_jsonl(self) -> None:
        from cccc.kernel.events import publish_event

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                publish_event("group.created", {"group_id": "g_test", "title": "demo"})

                p = Path(td) / "daemon" / "ccccd.events.jsonl"
                self.assertTrue(p.exists())
                lines = p.read_text(encoding="utf-8", errors="replace").splitlines()
                self.assertGreaterEqual(len(lines), 1)

                ev = json.loads(lines[-1])
                self.assertEqual(ev.get("kind"), "group.created")
                self.assertEqual(ev.get("data", {}).get("group_id"), "g_test")
                self.assertIn("ts", ev)
                self.assertIn("id", ev)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()

