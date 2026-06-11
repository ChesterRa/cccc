import os
import tempfile
import unittest
from pathlib import Path


class TestFederationAuth(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return Path(td), cleanup

    def test_admin_all_vs_empty_none(self) -> None:
        from cccc.kernel.federation.auth import can_access_group

        # admin => allowed_groups intentionally empty means ALL groups.
        admin = {"is_admin": True, "allowed_groups": []}
        self.assertTrue(can_access_group(admin, "any-group"))

        # non-admin with empty allowed_groups means NONE (not all).
        nobody = {"is_admin": False, "allowed_groups": []}
        self.assertFalse(can_access_group(nobody, "g1"))

    def test_explicit_allow_list(self) -> None:
        from cccc.kernel.federation.auth import can_access_group

        entry = {"is_admin": False, "allowed_groups": ["g1", "g2"]}
        self.assertTrue(can_access_group(entry, "g1"))
        self.assertTrue(can_access_group(entry, "g2"))
        self.assertFalse(can_access_group(entry, "g3"))

    def test_authorize_token_group_reuses_access_tokens(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.federation.auth import authorize_token_group

        _, cleanup = self._with_home()
        try:
            created = create_access_token("user-a", allowed_groups=["g1"], is_admin=False)
            tok = str(created.get("token") or "")

            allow = authorize_token_group(tok, "g1")
            self.assertTrue(allow["allowed"])
            self.assertFalse(allow["is_admin"])

            deny = authorize_token_group(tok, "g2")
            self.assertFalse(deny["allowed"])

            unknown = authorize_token_group("acc_does_not_exist", "g1")
            self.assertFalse(unknown["allowed"])
        finally:
            cleanup()

    def test_admin_token_authorizes_any_group(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.federation.auth import authorize_token_group

        _, cleanup = self._with_home()
        try:
            created = create_access_token("admin-a", is_admin=True)
            tok = str(created.get("token") or "")
            decision = authorize_token_group(tok, "whatever-group")
            self.assertTrue(decision["allowed"])
            self.assertTrue(decision["is_admin"])
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
