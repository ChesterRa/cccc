import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestWebTokens(unittest.TestCase):
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

    def test_create_lookup_list_delete_token(self) -> None:
        from cccc.kernel.web_tokens import create_token, delete_token, list_tokens, lookup_token

        _, cleanup = self._with_home()
        try:
            created = create_token("user-a", allowed_groups=["g1", "g1", "g2"], is_admin=False)
            token = str(created.get("token") or "")

            self.assertTrue(token.startswith("usr_"))
            self.assertEqual(str(created.get("user_id") or ""), "user-a")
            self.assertEqual(created.get("allowed_groups"), ["g1", "g2"])

            looked_up = lookup_token(token)
            self.assertIsNotNone(looked_up)
            assert looked_up is not None
            self.assertEqual(str(looked_up.get("user_id") or ""), "user-a")
            self.assertEqual(looked_up.get("allowed_groups"), ["g1", "g2"])

            listed = list_tokens()
            self.assertEqual(len(listed), 1)
            self.assertEqual(str(listed[0].get("token") or ""), token)

            self.assertTrue(delete_token(token))
            self.assertIsNone(lookup_token(token))
            self.assertEqual(list_tokens(), [])
        finally:
            cleanup()

    def test_load_tokens_tolerates_invalid_yaml(self) -> None:
        from cccc.kernel.web_tokens import load_tokens

        home, cleanup = self._with_home()
        try:
            (home / "web_tokens.yaml").write_text("tokens: [", encoding="utf-8")
            self.assertEqual(load_tokens(), {})
        finally:
            cleanup()

    def test_create_token_requires_user_id(self) -> None:
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            with self.assertRaises(ValueError):
                create_token("")
        finally:
            cleanup()
