import os
import tempfile
import unittest
from pathlib import Path


class TestBlobPathResolution(unittest.TestCase):
    def test_resolve_blob_attachment_path_guards_scope(self) -> None:
        from cccc.kernel.blobs import resolve_blob_attachment_path
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                reg = load_registry()
                group = create_group(reg, title="blob-path", topic="")

                rel = "state/blobs/x.txt"
                resolved = resolve_blob_attachment_path(group, rel_path=rel)
                self.assertEqual(resolved, (group.path / Path(rel)).resolve())

                with self.assertRaises(ValueError):
                    resolve_blob_attachment_path(group, rel_path="../outside.txt")
                with self.assertRaises(ValueError):
                    resolve_blob_attachment_path(group, rel_path="/abs/path.txt")
                with self.assertRaises(ValueError):
                    resolve_blob_attachment_path(group, rel_path="state/ledger/x.txt")
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
