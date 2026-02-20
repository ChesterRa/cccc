import os
import tempfile
import unittest


class TestActorProfilesOps(unittest.TestCase):
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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group(self, title: str = "ap-test") -> str:
        create, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        gid = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _create_profile(self, name: str = "Codex Headless") -> dict:
        upsert, _ = self._call(
            "actor_profile_upsert",
            {
                "by": "user",
                "profile": {
                    "name": name,
                    "runtime": "codex",
                    "runner": "headless",
                    "command": [],
                    "submit": "enter",
                    "env": {"X_PROFILE": "1"},
                },
            },
        )
        self.assertTrue(upsert.ok, getattr(upsert, "error", None))
        profile = (upsert.result or {}).get("profile") if isinstance(upsert.result, dict) else None
        self.assertIsInstance(profile, dict)
        assert isinstance(profile, dict)
        self.assertTrue(str(profile.get("id") or "").strip())
        return profile

    def test_profile_upsert_revision_and_secret_keys(self) -> None:
        _, cleanup = self._with_home()
        try:
            profile = self._create_profile()
            pid = str(profile.get("id") or "")
            rev1 = int(profile.get("revision") or 0)
            self.assertEqual(rev1, 1)

            update, _ = self._call(
                "actor_profile_upsert",
                {
                    "by": "user",
                    "expected_revision": rev1,
                    "profile": {
                        "id": pid,
                        "name": "Codex Headless V2",
                    },
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))
            profile2 = (update.result or {}).get("profile") if isinstance(update.result, dict) else None
            self.assertIsInstance(profile2, dict)
            assert isinstance(profile2, dict)
            self.assertEqual(int(profile2.get("revision") or 0), 2)

            mismatch, _ = self._call(
                "actor_profile_upsert",
                {
                    "by": "user",
                    "expected_revision": rev1,
                    "profile": {"id": pid, "name": "stale write"},
                },
            )
            self.assertFalse(mismatch.ok)
            self.assertEqual(getattr(mismatch.error, "code", ""), "profile_revision_mismatch")

            set_secret, _ = self._call(
                "actor_profile_secret_update",
                {
                    "by": "user",
                    "profile_id": pid,
                    "set": {"OPENAI_API_KEY": "supersecret"},
                },
            )
            self.assertTrue(set_secret.ok, getattr(set_secret, "error", None))

            keys, _ = self._call("actor_profile_secret_keys", {"by": "user", "profile_id": pid})
            self.assertTrue(keys.ok, getattr(keys, "error", None))
            result = keys.result if isinstance(keys.result, dict) else {}
            self.assertEqual(set(result.get("keys") or []), {"OPENAI_API_KEY", "X_PROFILE"})
            masked = result.get("masked_values")
            self.assertIsInstance(masked, dict)
            assert isinstance(masked, dict)
            self.assertEqual(str(masked.get("OPENAI_API_KEY") or ""), "su******et")

            get_profile, _ = self._call("actor_profile_get", {"by": "user", "profile_id": pid})
            self.assertTrue(get_profile.ok, getattr(get_profile, "error", None))
            pdoc = (get_profile.result or {}).get("profile") if isinstance(get_profile.result, dict) else {}
            self.assertIsInstance(pdoc, dict)
            assert isinstance(pdoc, dict)
            self.assertEqual(dict(pdoc.get("env") or {}), {})

        finally:
            cleanup()

    def test_linked_actor_is_runtime_readonly_and_convert_to_custom(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            profile = self._create_profile()
            pid = str(profile.get("id") or "")

            set_secret, _ = self._call(
                "actor_profile_secret_update",
                {
                    "by": "user",
                    "profile_id": pid,
                    "set": {"OPENAI_API_KEY": "supersecret"},
                },
            )
            self.assertTrue(set_secret.ok, getattr(set_secret, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "peer1",
                    "runtime": "gemini",  # should be overridden by profile
                    "runner": "pty",
                    "profile_id": pid,
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))
            actor = (add.result or {}).get("actor") if isinstance(add.result, dict) else None
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(str(actor.get("profile_id") or ""), pid)
            self.assertEqual(str(actor.get("runtime") or ""), "codex")
            self.assertEqual(str(actor.get("runner") or ""), "headless")

            private_keys, _ = self._call(
                "actor_env_private_keys",
                {"group_id": group_id, "actor_id": "peer1", "by": "user"},
            )
            self.assertFalse(private_keys.ok)
            self.assertEqual(getattr(private_keys.error, "code", ""), "actor_profile_linked_readonly")

            patch_runtime, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "patch": {"runtime": "claude"},
                },
            )
            self.assertFalse(patch_runtime.ok)
            self.assertEqual(getattr(patch_runtime.error, "code", ""), "actor_profile_linked_readonly")

            convert, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "profile_action": "convert_to_custom",
                    "patch": {},
                },
            )
            self.assertTrue(convert.ok, getattr(convert, "error", None))
            converted_actor = (convert.result or {}).get("actor") if isinstance(convert.result, dict) else None
            self.assertIsInstance(converted_actor, dict)
            assert isinstance(converted_actor, dict)
            self.assertEqual(str(converted_actor.get("profile_id") or ""), "")

            keys_after_convert, _ = self._call(
                "actor_env_private_keys",
                {"group_id": group_id, "actor_id": "peer1", "by": "user"},
            )
            self.assertTrue(keys_after_convert.ok, getattr(keys_after_convert, "error", None))
            keys_result = keys_after_convert.result if isinstance(keys_after_convert.result, dict) else {}
            self.assertEqual(set(keys_result.get("keys") or []), {"OPENAI_API_KEY", "X_PROFILE"})

            update_private, _ = self._call(
                "actor_env_private_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "set": {"OPENAI_API_KEY": "changed"},
                },
            )
            self.assertTrue(update_private.ok, getattr(update_private, "error", None))

        finally:
            cleanup()

    def test_profile_delete_rejects_in_use_and_supports_force_detach(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group("ap-delete")
            profile = self._create_profile("Delete Candidate")
            pid = str(profile.get("id") or "")

            set_secret, _ = self._call(
                "actor_profile_secret_update",
                {
                    "by": "user",
                    "profile_id": pid,
                    "set": {"OPENAI_API_KEY": "supersecret"},
                },
            )
            self.assertTrue(set_secret.ok, getattr(set_secret, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer One",
                    "runtime": "codex",
                    "runner": "headless",
                    "profile_id": pid,
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            delete_in_use, _ = self._call("actor_profile_delete", {"profile_id": pid, "by": "user"})
            self.assertFalse(delete_in_use.ok)
            self.assertEqual(getattr(delete_in_use.error, "code", ""), "profile_in_use")
            details = getattr(delete_in_use.error, "details", {}) or {}
            usage = details.get("usage") if isinstance(details, dict) else []
            self.assertIsInstance(usage, list)
            usage_entry = usage[0] if usage else {}
            self.assertIsInstance(usage_entry, dict)
            assert isinstance(usage_entry, dict)
            self.assertEqual(str(usage_entry.get("group_id") or ""), group_id)
            self.assertEqual(str(usage_entry.get("group_title") or ""), "ap-delete")
            self.assertEqual(str(usage_entry.get("actor_id") or ""), "peer1")
            self.assertEqual(str(usage_entry.get("actor_title") or ""), "Peer One")

            delete_ok, _ = self._call("actor_profile_delete", {"profile_id": pid, "by": "user", "force_detach": True})
            self.assertTrue(delete_ok.ok, getattr(delete_ok, "error", None))
            result = delete_ok.result if isinstance(delete_ok.result, dict) else {}
            self.assertEqual(int(result.get("detached_count") or 0), 1)

            list_resp, _ = self._call("actor_list", {"group_id": group_id})
            self.assertTrue(list_resp.ok, getattr(list_resp, "error", None))
            actors = (list_resp.result or {}).get("actors") if isinstance(list_resp.result, dict) else []
            self.assertIsInstance(actors, list)
            actor = next((a for a in actors if isinstance(a, dict) and str(a.get("id") or "") == "peer1"), None)
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(str(actor.get("profile_id") or ""), "")

            keys_after_force, _ = self._call(
                "actor_env_private_keys",
                {"group_id": group_id, "actor_id": "peer1", "by": "user"},
            )
            self.assertTrue(keys_after_force.ok, getattr(keys_after_force, "error", None))
            keys_result = keys_after_force.result if isinstance(keys_after_force.result, dict) else {}
            self.assertEqual(set(keys_result.get("keys") or []), {"OPENAI_API_KEY", "X_PROFILE"})
        finally:
            cleanup()

    def test_actor_add_rejects_invalid_profile_id(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group("ap-invalid-id")
            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "profile_id": "../bad",
                    "by": "user",
                },
            )
            self.assertFalse(add.ok)
            self.assertEqual(getattr(add.error, "code", ""), "actor_add_failed")
            self.assertIn("invalid profile_id", str(getattr(add.error, "message", "")))
        finally:
            cleanup()

    def test_group_start_returns_profile_not_found_when_link_is_missing(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group("ap-missing-profile")
            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            profile = self._create_profile("Start Missing Profile")
            pid = str(profile.get("id") or "")

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "profile_id": pid,
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            from cccc.daemon.actor_profile_store import delete_actor_profile, delete_actor_profile_secrets

            delete_actor_profile(pid)
            delete_actor_profile_secrets(pid)

            start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(getattr(start.error, "code", ""), "profile_not_found")
            self.assertIn(pid, str(getattr(start.error, "message", "")))
        finally:
            cleanup()

    def test_copy_actor_private_env_to_profile_secrets(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group("ap-copy-secrets")
            profile = self._create_profile("Copy Source")
            pid = str(profile.get("id") or "")

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "env": {"MODEL": "gpt-5"},
                    "env_private": {"OPENAI_API_KEY": "supersecret"},
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            copy_resp, _ = self._call(
                "actor_profile_secret_copy_from_actor",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "profile_id": pid,
                    "by": "user",
                },
            )
            self.assertTrue(copy_resp.ok, getattr(copy_resp, "error", None))
            keys = (copy_resp.result or {}).get("keys") if isinstance(copy_resp.result, dict) else []
            self.assertEqual(set(keys or []), {"OPENAI_API_KEY", "MODEL"})

            profile_keys, _ = self._call("actor_profile_secret_keys", {"profile_id": pid, "by": "user"})
            self.assertTrue(profile_keys.ok, getattr(profile_keys, "error", None))
            result = profile_keys.result if isinstance(profile_keys.result, dict) else {}
            self.assertEqual(set(result.get("keys") or []), {"OPENAI_API_KEY", "MODEL"})
        finally:
            cleanup()

    def test_copy_profile_private_env_from_profile(self) -> None:
        _, cleanup = self._with_home()
        try:
            source = self._create_profile("Source Profile")
            source_pid = str(source.get("id") or "")
            target = self._create_profile("Target Profile")
            target_pid = str(target.get("id") or "")

            set_source, _ = self._call(
                "actor_profile_secret_update",
                {
                    "by": "user",
                    "profile_id": source_pid,
                    "set": {"OPENAI_API_KEY": "source-secret", "MODEL": "gpt-5"},
                },
            )
            self.assertTrue(set_source.ok, getattr(set_source, "error", None))

            set_target, _ = self._call(
                "actor_profile_secret_update",
                {
                    "by": "user",
                    "profile_id": target_pid,
                    "set": {"ANTHROPIC_API_KEY": "old-target-secret"},
                },
            )
            self.assertTrue(set_target.ok, getattr(set_target, "error", None))

            copy_resp, _ = self._call(
                "actor_profile_secret_copy_from_profile",
                {
                    "by": "user",
                    "profile_id": target_pid,
                    "source_profile_id": source_pid,
                },
            )
            self.assertTrue(copy_resp.ok, getattr(copy_resp, "error", None))
            copied_keys = (copy_resp.result or {}).get("keys") if isinstance(copy_resp.result, dict) else []
            self.assertEqual(set(copied_keys or []), {"OPENAI_API_KEY", "MODEL", "X_PROFILE"})

            target_keys_resp, _ = self._call("actor_profile_secret_keys", {"profile_id": target_pid, "by": "user"})
            self.assertTrue(target_keys_resp.ok, getattr(target_keys_resp, "error", None))
            target_keys_result = target_keys_resp.result if isinstance(target_keys_resp.result, dict) else {}
            self.assertEqual(set(target_keys_result.get("keys") or []), {"OPENAI_API_KEY", "MODEL", "X_PROFILE"})
        finally:
            cleanup()

    def test_attach_profile_with_title_patch_succeeds(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group("ap-attach-title")
            profile = self._create_profile("Attach + Title")
            pid = str(profile.get("id") or "")

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer One",
                    "runtime": "gemini",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            attach, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "profile_id": pid,
                    "patch": {"title": "Linked Peer"},
                },
            )
            self.assertTrue(attach.ok, getattr(attach, "error", None))
            actor = (attach.result or {}).get("actor") if isinstance(attach.result, dict) else {}
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(str(actor.get("profile_id") or ""), pid)
            self.assertEqual(str(actor.get("title") or ""), "Linked Peer")
            self.assertEqual(str(actor.get("runtime") or ""), "codex")
            self.assertEqual(str(actor.get("runner") or ""), "headless")

            private_keys, _ = self._call(
                "actor_env_private_keys",
                {"group_id": group_id, "actor_id": "peer1", "by": "user"},
            )
            self.assertFalse(private_keys.ok)
            self.assertEqual(getattr(private_keys.error, "code", ""), "actor_profile_linked_readonly")
        finally:
            cleanup()

    def test_attach_profile_rejects_runtime_patch_in_same_request(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group("ap-attach-reject")
            profile = self._create_profile("Attach Reject")
            pid = str(profile.get("id") or "")

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "gemini",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            bad_attach, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "profile_id": pid,
                    "patch": {"runtime": "claude"},
                },
            )
            self.assertFalse(bad_attach.ok)
            self.assertEqual(getattr(bad_attach.error, "code", ""), "invalid_request")
            self.assertIn("cannot patch runtime fields", str(getattr(bad_attach.error, "message", "")))
        finally:
            cleanup()

    def test_convert_to_custom_with_title_patch_succeeds(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group("ap-convert-title")
            profile = self._create_profile("Convert + Title")
            pid = str(profile.get("id") or "")

            set_secret, _ = self._call(
                "actor_profile_secret_update",
                {
                    "by": "user",
                    "profile_id": pid,
                    "set": {"OPENAI_API_KEY": "supersecret"},
                },
            )
            self.assertTrue(set_secret.ok, getattr(set_secret, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer One",
                    "runtime": "codex",
                    "runner": "headless",
                    "profile_id": pid,
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            convert, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "profile_action": "convert_to_custom",
                    "patch": {"title": "Converted Peer"},
                },
            )
            self.assertTrue(convert.ok, getattr(convert, "error", None))
            actor = (convert.result or {}).get("actor") if isinstance(convert.result, dict) else {}
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(str(actor.get("profile_id") or ""), "")
            self.assertEqual(str(actor.get("title") or ""), "Converted Peer")

            keys_after_convert, _ = self._call(
                "actor_env_private_keys",
                {"group_id": group_id, "actor_id": "peer1", "by": "user"},
            )
            self.assertTrue(keys_after_convert.ok, getattr(keys_after_convert, "error", None))
            keys_result = keys_after_convert.result if isinstance(keys_after_convert.result, dict) else {}
            self.assertEqual(set(keys_result.get("keys") or []), {"OPENAI_API_KEY", "X_PROFILE"})
        finally:
            cleanup()

    def test_convert_to_custom_rejects_runtime_patch_in_same_request(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group("ap-convert-reject")
            profile = self._create_profile("Convert Reject")
            pid = str(profile.get("id") or "")

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "profile_id": pid,
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            bad_convert, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "profile_action": "convert_to_custom",
                    "patch": {"runtime": "claude"},
                },
            )
            self.assertFalse(bad_convert.ok)
            # Linked actor runtime fields are read-only; this guard triggers before convert+patch guard.
            self.assertEqual(getattr(bad_convert.error, "code", ""), "actor_profile_linked_readonly")
            self.assertIn("read-only", str(getattr(bad_convert.error, "message", "")))
        finally:
            cleanup()

    def test_two_step_convert_then_runtime_patch_succeeds(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group("ap-two-step")
            profile = self._create_profile("Two Step")
            pid = str(profile.get("id") or "")

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "profile_id": pid,
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            convert, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "profile_action": "convert_to_custom",
                    "patch": {},
                },
            )
            self.assertTrue(convert.ok, getattr(convert, "error", None))

            patch_runtime, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "patch": {"runtime": "custom"},
                },
            )
            self.assertTrue(patch_runtime.ok, getattr(patch_runtime, "error", None))
            actor = (patch_runtime.result or {}).get("actor") if isinstance(patch_runtime.result, dict) else {}
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(str(actor.get("profile_id") or ""), "")
            self.assertEqual(str(actor.get("runtime") or ""), "custom")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
