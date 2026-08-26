import os
import tempfile
import threading
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

    def test_custom_access_token_must_be_http_bearer_safe(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            with self.assertRaisesRegex(ValueError, "bearer-token"):
                create_access_token(
                    "admin",
                    is_admin=True,
                    custom_token="token; 含",
                )
        finally:
            cleanup()

    def test_load_access_tokens_rejects_invalid_yaml(self) -> None:
        from cccc.kernel.access_tokens import load_access_tokens

        home, cleanup = self._with_home()
        try:
            (home / "access_tokens.yaml").write_text("tokens: [", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "access token store is invalid"):
                load_access_tokens()
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

    def test_concurrent_creates_preserve_both_tokens(self) -> None:
        import cccc.kernel.access_tokens as access_tokens

        _, cleanup = self._with_home()
        first_save_started = threading.Event()
        release_first_save = threading.Event()
        second_lock_attempted = threading.Event()
        second_finished = threading.Event()
        results: list[str] = []
        errors: list[BaseException] = []
        original_acquire = access_tokens.acquire_lockfile
        original_save = access_tokens._save_access_tokens_unlocked

        def observed_acquire(path, *, blocking=True):
            if threading.current_thread().name == "token-writer-b":
                second_lock_attempted.set()
            return original_acquire(path, blocking=blocking)

        def controlled_save(tokens, home=None):
            if threading.current_thread().name == "token-writer-a":
                first_save_started.set()
                if not release_first_save.wait(timeout=3.0):
                    raise AssertionError("timed out waiting to release the first token write")
            return original_save(tokens, home)

        def create(user_id: str, custom_token: str) -> None:
            try:
                entry = access_tokens.create_access_token(
                    user_id,
                    is_admin=True,
                    custom_token=custom_token,
                )
                results.append(str(entry.get("user_id") or ""))
            except BaseException as exc:  # pragma: no cover - asserted below
                errors.append(exc)
            finally:
                if threading.current_thread().name == "token-writer-b":
                    second_finished.set()

        first = threading.Thread(
            target=create,
            args=("admin-a", "acc_concurrent_a"),
            name="token-writer-a",
        )
        second = threading.Thread(
            target=create,
            args=("admin-b", "acc_concurrent_b"),
            name="token-writer-b",
        )
        try:
            with (
                mock.patch.object(access_tokens, "acquire_lockfile", side_effect=observed_acquire),
                mock.patch.object(access_tokens, "_save_access_tokens_unlocked", side_effect=controlled_save),
            ):
                first.start()
                self.assertTrue(first_save_started.wait(timeout=2.0))
                second.start()
                self.assertTrue(second_lock_attempted.wait(timeout=2.0))
                self.assertFalse(second_finished.wait(timeout=0.1))
                release_first_save.set()
                first.join(timeout=3.0)
                second.join(timeout=3.0)

            self.assertFalse(first.is_alive())
            self.assertFalse(second.is_alive())
            self.assertEqual(errors, [])
            self.assertCountEqual(results, ["admin-a", "admin-b"])
            self.assertCountEqual(
                [item.get("user_id") for item in access_tokens.list_access_tokens()],
                ["admin-a", "admin-b"],
            )
        finally:
            release_first_save.set()
            first.join(timeout=1.0)
            second.join(timeout=1.0)
            cleanup()

    def test_last_admin_cannot_be_demoted_or_deleted_while_scoped_tokens_remain(self) -> None:
        from cccc.kernel.access_tokens import (
            LastAdminRequiredError,
            create_access_token,
            delete_access_token,
            list_access_tokens,
            update_access_token,
        )

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin", is_admin=True)
            member = create_access_token("member", allowed_groups=["g1"], is_admin=False)
            admin_token = str(admin.get("token") or "")
            member_token = str(member.get("token") or "")

            with self.assertRaises(LastAdminRequiredError):
                update_access_token(admin_token, allowed_groups=["g1"], is_admin=False)
            with self.assertRaises(LastAdminRequiredError):
                delete_access_token(admin_token)

            self.assertCountEqual(
                [item.get("user_id") for item in list_access_tokens()],
                ["admin", "member"],
            )
            self.assertTrue(delete_access_token(member_token))
            with self.assertRaises(LastAdminRequiredError):
                delete_access_token(admin_token)
            self.assertEqual(
                [item.get("user_id") for item in list_access_tokens()],
                ["admin"],
            )
        finally:
            cleanup()
