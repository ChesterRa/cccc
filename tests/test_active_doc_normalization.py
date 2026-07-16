import os
import tempfile
import unittest


class TestActiveDocNormalization(unittest.TestCase):
    def test_load_active_normalizes_non_dict_payload(self) -> None:
        from cccc.kernel.active import active_path, load_active
        from cccc.util.fs import atomic_write_json, read_json

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                p = active_path()
                p.parent.mkdir(parents=True, exist_ok=True)
                atomic_write_json(p, ["bad", "shape"])

                doc = load_active()
                self.assertIsInstance(doc, dict)
                self.assertEqual(doc.get("v"), 1)
                self.assertEqual(doc.get("active_group_id"), "")
                self.assertTrue(str(doc.get("updated_at") or "").strip())

                persisted = read_json(p)
                self.assertIsInstance(persisted, dict)
                self.assertEqual((persisted or {}).get("active_group_id"), "")
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
