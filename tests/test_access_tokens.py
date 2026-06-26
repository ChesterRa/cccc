import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock


class TestAccessTokens(unittest.TestCase):
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

    def test_create_lookup_list_delete_access_token(self) -> None:
        from cccc.kernel.access_tokens import (
            create_access_token,
            delete_access_token,
            list_access_tokens,
            lookup_access_token,
        )

        _, cleanup = self._with_home()
        try:
            created = create_access_token("user-a", allowed_groups=["g1", "g1", "g2"], is_admin=False)
            token = str(created.get("token") or "")

            self.assertTrue(token.startswith("acc_"))
            self.assertEqual(str(created.get("user_id") or ""), "user-a")
            self.assertEqual(created.get("allowed_groups"), ["g1", "g2"])

            looked_up = lookup_access_token(token)
            self.assertIsNotNone(looked_up)
            assert looked_up is not None
            self.assertEqual(str(looked_up.get("user_id") or ""), "user-a")
            self.assertEqual(looked_up.get("allowed_groups"), ["g1", "g2"])

            listed = list_access_tokens()
            self.assertEqual(len(listed), 1)
            self.assertEqual(str(listed[0].get("token") or ""), token)

            self.assertTrue(delete_access_token(token))
            self.assertIsNone(lookup_access_token(token))
            self.assertEqual(list_access_tokens(), [])
        finally:
            cleanup()

    def test_load_access_tokens_tolerates_invalid_yaml(self) -> None:
        from cccc.kernel.access_tokens import load_access_tokens

        home, cleanup = self._with_home()
        try:
            (home / "access_tokens.yaml").write_text("tokens: [", encoding="utf-8")
            self.assertEqual(load_access_tokens(), {})
        finally:
            cleanup()

    def test_reuses_access_tokens_when_file_is_unchanged(self) -> None:
        from cccc.kernel.access_tokens import list_access_tokens, load_access_tokens, lookup_access_token

        home, cleanup = self._with_home()
        try:
            token_file = home / "access_tokens.yaml"
            token_file.write_text(
                "tokens:\n"
                "  acc_test:\n"
                "    user_id: user-a\n"
                "    allowed_groups: []\n"
                "    is_admin: true\n",
                encoding="utf-8",
            )
            with mock.patch(
                "cccc.kernel.access_tokens.Path.read_text",
                autospec=True,
                return_value=token_file.read_text(encoding="utf-8"),
            ) as read_text:
                self.assertIn("acc_test", load_access_tokens())
                self.assertIsNotNone(lookup_access_token("acc_test"))
                self.assertEqual(len(list_access_tokens()), 1)

            self.assertEqual(read_text.call_count, 1)
        finally:
            cleanup()

    def test_cached_access_token_entries_do_not_share_allowed_groups(self) -> None:
        from cccc.kernel.access_tokens import load_access_tokens, lookup_access_token

        home, cleanup = self._with_home()
        try:
            token_file = home / "access_tokens.yaml"
            token_file.write_text(
                "tokens:\n"
                "  acc_test:\n"
                "    user_id: user-a\n"
                "    allowed_groups: [g1]\n"
                "    is_admin: false\n",
                encoding="utf-8",
            )

            first = load_access_tokens()
            first["acc_test"]["allowed_groups"].append("g2")

            second = load_access_tokens()
            self.assertEqual(second["acc_test"]["allowed_groups"], ["g1"])

            looked_up = lookup_access_token("acc_test")
            self.assertIsNotNone(looked_up)
            assert looked_up is not None
            looked_up["allowed_groups"].append("g3")

            third = load_access_tokens()
            self.assertEqual(third["acc_test"]["allowed_groups"], ["g1"])
        finally:
            cleanup()

    def test_create_access_token_requires_user_id(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            with self.assertRaises(ValueError):
                create_access_token("")
        finally:
            cleanup()
