import os
import tempfile
import unittest


class TestRegistryDocNormalization(unittest.TestCase):
    def test_load_registry_normalizes_non_dict_root(self) -> None:
        from cccc.kernel.registry import load_registry
        from cccc.paths import ensure_home
        from cccc.util.fs import atomic_write_json

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                home = ensure_home()
                atomic_write_json(home / "registry.json", ["bad"])

                reg = load_registry()
                self.assertIsInstance(reg.doc, dict)
                self.assertIsInstance(reg.groups, dict)
                self.assertIsInstance(reg.defaults, dict)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_load_registry_normalizes_bad_groups_defaults(self) -> None:
        from cccc.kernel.registry import load_registry, set_default_group_for_scope
        from cccc.paths import ensure_home
        from cccc.util.fs import atomic_write_json, read_json

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                home = ensure_home()
                atomic_write_json(
                    home / "registry.json",
                    {
                        "v": 1,
                        "created_at": "2026-01-01T00:00:00Z",
                        "updated_at": "2026-01-01T00:00:00Z",
                        "groups": [],
                        "defaults": "oops",
                    },
                )

                reg = load_registry()
                self.assertIsInstance(reg.groups, dict)
                self.assertIsInstance(reg.defaults, dict)
                set_default_group_for_scope(reg, "s_x", "g_x")

                persisted = read_json(home / "registry.json")
                self.assertIsInstance(persisted, dict)
                assert isinstance(persisted, dict)
                self.assertIsInstance(persisted.get("groups"), dict)
                self.assertIsInstance(persisted.get("defaults"), dict)
                self.assertEqual((persisted.get("defaults") or {}).get("s_x"), "g_x")
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
