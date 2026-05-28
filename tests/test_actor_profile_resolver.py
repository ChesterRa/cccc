import os
import tempfile
import unittest


class TestActorProfileResolver(unittest.TestCase):
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

        return td, cleanup

    def test_legacy_upsert_defaults_to_global_scope(self) -> None:
        from cccc.daemon.actors.actor_profile_store import get_actor_profile, list_actor_profiles, upsert_actor_profile

        _, cleanup = self._with_home()
        try:
            created = upsert_actor_profile(
                {
                    "id": "shared",
                    "name": "Global Shared",
                    "runtime": "codex",
                    "runner": "headless",
                    "command": [],
                }
            )
            self.assertEqual(str(created.get("scope") or ""), "global")
            self.assertEqual(str(created.get("owner_id") or ""), "")

            fetched = get_actor_profile("shared")
            self.assertIsInstance(fetched, dict)
            assert isinstance(fetched, dict)
            self.assertEqual(str(fetched.get("scope") or ""), "global")
            self.assertEqual(str(fetched.get("owner_id") or ""), "")

            listed = list_actor_profiles()
            self.assertEqual(len(listed), 1)
            self.assertEqual(str(listed[0].get("id") or ""), "shared")
            self.assertEqual(str(listed[0].get("scope") or ""), "global")
        finally:
            cleanup()

    def test_resolver_supports_duplicate_ids_across_scope_and_owner(self) -> None:
        from cccc.contracts.v1 import ActorProfile, ActorProfileRef
        from cccc.daemon.actors.actor_profile_store import ProfileResolver, get_actor_profile

        _, cleanup = self._with_home()
        try:
            resolver = ProfileResolver()
            self.assertTrue(
                resolver.save_profile(
                    ActorProfile(id="shared", name="Global Shared", runtime="codex", runner="headless", command=[]),
                    caller_id="admin-user",
                    is_admin=True,
                )
            )
            self.assertTrue(
                resolver.save_profile(
                    ActorProfile(
                        id="shared",
                        name="User Shared",
                        scope="user",
                        owner_id="user-a",
                        runtime="codex",
                        runner="headless",
                        command=[],
                    ),
                    caller_id="user-a",
                    is_admin=False,
                )
            )

            global_profile = get_actor_profile("shared")
            self.assertIsInstance(global_profile, dict)
            assert isinstance(global_profile, dict)
            self.assertEqual(str(global_profile.get("name") or ""), "Global Shared")

            global_list = resolver.list_profiles("global", caller_id="user-a", is_admin=False)
            self.assertEqual([(item.id, item.scope, item.owner_id) for item in global_list], [("shared", "global", "")])

            my_list = resolver.list_profiles("my", caller_id="user-a", is_admin=False)
            self.assertEqual([(item.id, item.scope, item.owner_id) for item in my_list], [("shared", "user", "user-a")])

            accessible_member = resolver.list_profiles("accessible", caller_id="user-a", is_admin=False)
            self.assertEqual(
                [(item.id, item.scope, item.owner_id) for item in accessible_member],
                [("shared", "global", ""), ("shared", "user", "user-a")],
            )

            accessible_admin = resolver.list_profiles("accessible", caller_id="admin-user", is_admin=True)
            self.assertEqual(
                sorted((item.id, item.scope, item.owner_id) for item in accessible_admin),
                [("shared", "global", ""), ("shared", "user", "user-a")],
            )

            denied = resolver.resolve(
                ActorProfileRef(profile_id="shared", profile_scope="user", profile_owner="user-a"),
                caller_id="user-b",
                is_admin=False,
            )
            self.assertIsNone(denied)

            owned = resolver.resolve(
                ActorProfileRef(profile_id="shared", profile_scope="user", profile_owner="user-a"),
                caller_id="user-a",
                is_admin=False,
            )
            self.assertIsNotNone(owned)
            assert owned is not None
            self.assertEqual(owned.name, "User Shared")
        finally:
            cleanup()

    def test_resolver_enforces_save_delete_permissions(self) -> None:
        from cccc.contracts.v1 import ActorProfile, ActorProfileRef
        from cccc.daemon.actors.actor_profile_store import ProfileResolver

        _, cleanup = self._with_home()
        try:
            resolver = ProfileResolver()

            self.assertFalse(
                resolver.save_profile(
                    ActorProfile(id="global-one", runtime="codex", runner="headless", command=[]),
                    caller_id="member-user",
                    is_admin=False,
                )
            )

            user_profile = ActorProfile(
                id="user-one",
                name="Member Profile",
                scope="user",
                owner_id="member-user",
                runtime="codex",
                runner="headless",
                command=[],
            )
            self.assertTrue(resolver.save_profile(user_profile, caller_id="member-user", is_admin=False))
            self.assertFalse(
                resolver.delete_profile(
                    ActorProfileRef(profile_id="user-one", profile_scope="user", profile_owner="member-user"),
                    caller_id="other-user",
                    is_admin=False,
                )
            )
            self.assertTrue(
                resolver.delete_profile(
                    ActorProfileRef(profile_id="user-one", profile_scope="user", profile_owner="member-user"),
                    caller_id="admin-user",
                    is_admin=True,
                )
            )
            self.assertIsNone(
                resolver.resolve(
                    ActorProfileRef(profile_id="user-one", profile_scope="user", profile_owner="member-user"),
                    caller_id="admin-user",
                    is_admin=True,
                )
            )
        finally:
            cleanup()
