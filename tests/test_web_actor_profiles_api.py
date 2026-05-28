import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebActorProfilesApi(unittest.TestCase):
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

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        return create_group(reg, title="web-actor-profile-test", topic="").group_id

    def test_profiles_routes_support_my_and_all_views(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = str(create_access_token("admin-user", is_admin=True).get("token") or "")
            member = str(create_access_token("member-user", is_admin=False).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()

                create_my = client.put(
                    "/api/v1/profiles/member-profile",
                    headers={"Authorization": f"Bearer {member}"},
                    json={
                        "scope": "user",
                        "owner_id": "member-user",
                        "name": "Member Profile",
                        "runtime": "codex",
                        "runner": "pty",
                        "command": [],
                    },
                )
                self.assertEqual(create_my.status_code, 200)
                create_my_body = create_my.json()
                self.assertTrue(bool(create_my_body.get("ok")))

                create_global = client.post(
                    "/api/v1/actor_profiles",
                    headers={"Authorization": f"Bearer {admin}"},
                    json={
                        "by": "user",
                        "profile": {
                            "id": "global-profile",
                            "name": "Global Profile",
                            "runtime": "codex",
                            "runner": "pty",
                            "command": [],
                        },
                    },
                )
                self.assertEqual(create_global.status_code, 200)
                self.assertTrue(bool(create_global.json().get("ok")))

                my_resp = client.get("/api/v1/profiles?view=my", headers={"Authorization": f"Bearer {member}"})
                self.assertEqual(my_resp.status_code, 200)
                my_profiles = (((my_resp.json().get("result") or {}).get("profiles")) or [])
                self.assertEqual([(item.get("id"), item.get("scope"), item.get("owner_id")) for item in my_profiles], [("member-profile", "user", "member-user")])

                denied_all = client.get("/api/v1/profiles?view=all", headers={"Authorization": f"Bearer {member}"})
                self.assertEqual(denied_all.status_code, 200)
                denied_all_body = denied_all.json()
                self.assertFalse(bool(denied_all_body.get("ok")))
                self.assertEqual(str(((denied_all_body.get("error") or {}).get("code")) or ""), "permission_denied")

                all_resp = client.get("/api/v1/profiles?view=all", headers={"Authorization": f"Bearer {admin}"})
                self.assertEqual(all_resp.status_code, 200)
                all_profiles = (((all_resp.json().get("result") or {}).get("profiles")) or [])
                self.assertEqual(
                    sorted((item.get("id"), item.get("scope"), item.get("owner_id")) for item in all_profiles),
                    [("global-profile", "global", ""), ("member-profile", "user", "member-user")],
                )

                member_accessible_resp = client.get("/api/v1/profiles?view=accessible", headers={"Authorization": f"Bearer {member}"})
                self.assertEqual(member_accessible_resp.status_code, 200)
                member_accessible_profiles = (((member_accessible_resp.json().get("result") or {}).get("profiles")) or [])
                self.assertEqual(
                    sorted((item.get("id"), item.get("scope"), item.get("owner_id")) for item in member_accessible_profiles),
                    [("global-profile", "global", ""), ("member-profile", "user", "member-user")],
                )

                admin_accessible_resp = client.get("/api/v1/profiles?view=accessible", headers={"Authorization": f"Bearer {admin}"})
                self.assertEqual(admin_accessible_resp.status_code, 200)
                admin_accessible_profiles = (((admin_accessible_resp.json().get("result") or {}).get("profiles")) or [])
                self.assertEqual(
                    sorted((item.get("id"), item.get("scope"), item.get("owner_id")) for item in admin_accessible_profiles),
                    [("global-profile", "global", ""), ("member-profile", "user", "member-user")],
                )
        finally:
            cleanup()

    def test_legacy_actor_profiles_routes_remain_global_compatible(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = str(create_access_token("admin-user", is_admin=True).get("token") or "")
            member = str(create_access_token("member-user", is_admin=False).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()

                create_global = client.post(
                    "/api/v1/actor_profiles",
                    headers={"Authorization": f"Bearer {admin}"},
                    json={
                        "by": "user",
                        "profile": {
                            "id": "legacy-global",
                            "name": "Legacy Global",
                            "runtime": "codex",
                            "runner": "pty",
                            "command": [],
                        },
                    },
                )
                self.assertEqual(create_global.status_code, 200)
                self.assertTrue(bool(create_global.json().get("ok")))

                list_global = client.get("/api/v1/actor_profiles", headers={"Authorization": f"Bearer {member}"})
                self.assertEqual(list_global.status_code, 200)
                profiles = (((list_global.json().get("result") or {}).get("profiles")) or [])
                self.assertEqual([(item.get("id"), item.get("scope"), item.get("owner_id")) for item in profiles], [("legacy-global", "global", "")])

                delete_global = client.delete(
                    "/api/v1/actor_profiles/legacy-global?by=user",
                    headers={"Authorization": f"Bearer {admin}"},
                )
                self.assertEqual(delete_global.status_code, 200)
                self.assertTrue(bool(delete_global.json().get("ok")))
        finally:
            cleanup()

    def test_member_can_manage_own_profile_secrets_via_profiles_routes(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            member = str(create_access_token("member-user", is_admin=False).get("token") or "")
            other = str(create_access_token("other-user", is_admin=False).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()

                create_my = client.put(
                    "/api/v1/profiles/member-secret-profile",
                    headers={"Authorization": f"Bearer {member}"},
                    json={
                        "scope": "user",
                        "owner_id": "member-user",
                        "name": "Member Secret Profile",
                        "runtime": "codex",
                        "runner": "pty",
                        "command": [],
                    },
                )
                self.assertEqual(create_my.status_code, 200)
                self.assertTrue(bool(create_my.json().get("ok")))

                update_secret = client.post(
                    "/api/v1/profiles/member-secret-profile/env_private",
                    headers={"Authorization": f"Bearer {member}"},
                    json={
                        "by": "user",
                        "scope": "user",
                        "owner_id": "member-user",
                        "set": {"API_KEY": "member-secret"},
                        "unset": [],
                        "clear": False,
                    },
                )
                self.assertEqual(update_secret.status_code, 200)
                self.assertTrue(bool(update_secret.json().get("ok")))

                list_secret = client.get(
                    "/api/v1/profiles/member-secret-profile/env_private?by=user&scope=user&owner_id=member-user",
                    headers={"Authorization": f"Bearer {member}"},
                )
                self.assertEqual(list_secret.status_code, 200)
                self.assertEqual((((list_secret.json().get("result") or {}).get("keys")) or []), ["API_KEY"])

                denied = client.get(
                    "/api/v1/profiles/member-secret-profile/env_private?by=user&scope=user&owner_id=member-user",
                    headers={"Authorization": f"Bearer {other}"},
                )
                self.assertEqual(denied.status_code, 200)
                self.assertFalse(bool(denied.json().get("ok")))
                self.assertEqual(str(((denied.json().get("error") or {}).get("code")) or ""), "permission_denied")
        finally:
            cleanup()

    def test_standard_web_allows_claude_headless_profile_and_actor(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            member = str(create_access_token("member-user", is_admin=False, allowed_groups=[group_id]).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()

                create_profile = client.put(
                    "/api/v1/profiles/claude-headless",
                    headers={"Authorization": f"Bearer {member}"},
                    json={
                        "scope": "user",
                        "owner_id": "member-user",
                        "name": "Claude Headless",
                        "runtime": "claude",
                        "runner": "headless",
                        "command": [],
                    },
                )
                self.assertEqual(create_profile.status_code, 200)
                create_profile_body = create_profile.json()
                self.assertTrue(bool(create_profile_body.get("ok")))
                created_profile = ((create_profile_body.get("result") or {}).get("profile")) or {}
                self.assertEqual(str(created_profile.get("runner") or ""), "headless")

                list_profiles = client.get(
                    "/api/v1/profiles?view=my",
                    headers={"Authorization": f"Bearer {member}"},
                )
                self.assertEqual(list_profiles.status_code, 200)
                profiles = (((list_profiles.json().get("result") or {}).get("profiles")) or [])
                listed_profile = next(
                    item for item in profiles
                    if str(item.get("id") or "") == "claude-headless"
                )
                self.assertEqual(str(listed_profile.get("runner") or ""), "headless")
                self.assertEqual(str(listed_profile.get("runtime") or ""), "claude")

                create_actor = client.post(
                    f"/api/v1/groups/{group_id}/actors",
                    headers={"Authorization": f"Bearer {member}"},
                    json={
                        "actor_id": "peer-1",
                        "runtime": "claude",
                        "runner": "headless",
                        "profile_id": "claude-headless",
                        "profile_scope": "user",
                        "profile_owner": "member-user",
                    },
                )
                self.assertEqual(create_actor.status_code, 200)
                create_actor_body = create_actor.json()
                self.assertTrue(bool(create_actor_body.get("ok")))
                actor = ((create_actor_body.get("result") or {}).get("actor")) or {}
                self.assertEqual(str(actor.get("runtime") or ""), "claude")
                self.assertEqual(str(actor.get("runner") or ""), "headless")
        finally:
            cleanup()

    def test_actor_create_rejects_user_scoped_headless_profile(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()

                global_profile = self._local_call_daemon(
                    {
                        "op": "actor_profile_upsert",
                        "args": {
                            "by": "user",
                            "profile": {
                                "id": "shared-profile",
                                "name": "Shared PTY",
                                "runtime": "codex",
                                "runner": "pty",
                                "command": [],
                            },
                        },
                    }
                )
                self.assertTrue(bool(global_profile.get("ok")))

                user_profile = self._local_call_daemon(
                    {
                        "op": "actor_profile_upsert",
                        "args": {
                            "by": "user",
                            "caller_id": "member-user",
                            "is_admin": False,
                            "profile": {
                                "id": "shared-profile",
                                "scope": "user",
                                "owner_id": "member-user",
                                "name": "Scoped Headless",
                                "runtime": "custom",
                                "runner": "headless",
                                "command": ["bash", "-lc", "echo hi"],
                            },
                        },
                    }
                )
                self.assertTrue(bool(user_profile.get("ok")))

                create_resp = client.post(
                    f"/api/v1/groups/{group_id}/actors",
                    json={
                        "actor_id": "peer-1",
                        "runtime": "codex",
                        "runner": "pty",
                        "profile_id": "shared-profile",
                        "profile_scope": "user",
                        "profile_owner": "member-user",
                    },
                )
                self.assertEqual(create_resp.status_code, 400)
                body = create_resp.json()
                self.assertEqual(str(((body.get("error") or {}).get("code")) or ""), "headless_internal_only")
        finally:
            cleanup()

    def test_actor_update_rejects_user_scoped_headless_profile(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()

                created_actor = self._local_call_daemon(
                    {
                        "op": "actor_add",
                        "args": {
                            "group_id": group_id,
                            "actor_id": "peer-1",
                            "runtime": "codex",
                            "runner": "pty",
                            "command": [],
                            "env": {},
                            "by": "user",
                        },
                    }
                )
                self.assertTrue(bool(created_actor.get("ok")))

                self._local_call_daemon(
                    {
                        "op": "actor_profile_upsert",
                        "args": {
                            "by": "user",
                            "profile": {
                                "id": "shared-profile",
                                "name": "Shared PTY",
                                "runtime": "codex",
                                "runner": "pty",
                                "command": [],
                            },
                        },
                    }
                )
                self._local_call_daemon(
                    {
                        "op": "actor_profile_upsert",
                        "args": {
                            "by": "user",
                            "caller_id": "member-user",
                            "is_admin": False,
                            "profile": {
                                "id": "shared-profile",
                                "scope": "user",
                                "owner_id": "member-user",
                                "name": "Scoped Headless",
                                "runtime": "custom",
                                "runner": "headless",
                                "command": ["bash", "-lc", "echo hi"],
                            },
                        },
                    }
                )

                update_resp = client.post(
                    f"/api/v1/groups/{group_id}/actors/peer-1",
                    json={
                        "profile_id": "shared-profile",
                        "profile_scope": "user",
                        "profile_owner": "member-user",
                    },
                )
                self.assertEqual(update_resp.status_code, 400)
                body = update_resp.json()
                self.assertEqual(str(((body.get("error") or {}).get("code")) or ""), "headless_internal_only")
        finally:
            cleanup()

    def test_actor_create_rejects_user_scoped_headless_profile_even_with_non_owner_token(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            viewer_token = str(create_access_token("viewer-user", is_admin=False, allowed_groups=[group_id]).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()

                self._local_call_daemon(
                    {
                        "op": "actor_profile_upsert",
                        "args": {
                            "by": "user",
                            "caller_id": "member-user",
                            "is_admin": False,
                            "profile": {
                                "id": "shared-profile",
                                "scope": "user",
                                "owner_id": "member-user",
                                "name": "Scoped Headless",
                                "runtime": "custom",
                                "runner": "headless",
                                "command": ["bash", "-lc", "echo hi"],
                            },
                        },
                    }
                )

                create_resp = client.post(
                    f"/api/v1/groups/{group_id}/actors",
                    headers={"Authorization": f"Bearer {viewer_token}"},
                    json={
                        "actor_id": "peer-1",
                        "runtime": "codex",
                        "runner": "pty",
                        "profile_id": "shared-profile",
                        "profile_scope": "user",
                        "profile_owner": "member-user",
                    },
                )
                self.assertEqual(create_resp.status_code, 400)
                body = create_resp.json()
                self.assertEqual(str(((body.get("error") or {}).get("code")) or ""), "headless_internal_only")
        finally:
            cleanup()
