from __future__ import annotations

import copy
import os
import tempfile
import unittest
import threading
from unittest.mock import patch


class TestSharedDocumentConcurrency(unittest.TestCase):
    def setUp(self) -> None:
        self._old_home = os.environ.get("CCCC_HOME")
        self._home = tempfile.TemporaryDirectory()
        os.environ["CCCC_HOME"] = self._home.name

    def tearDown(self) -> None:
        self._home.cleanup()
        if self._old_home is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = self._old_home

    def test_stale_group_save_preserves_disjoint_browser_target(self) -> None:
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry
        from cccc.ports.web_model_browser_sidecar import record_chatgpt_browser_state

        created = create_group(load_registry(), title="shared-write")
        stale = load_group(created.group_id)
        assert stale is not None
        conversation_url = "https://chatgpt.com/c/6a75a645-2d80-83e8-9c3c-76d4eae96845"

        self.assertTrue(
            record_chatgpt_browser_state(
                created.group_id,
                "web1",
                {
                    "conversation_url": conversation_url,
                    "target_saved_at": "2026-08-12T00:00:00Z",
                },
            )
        )
        stale.doc["topic"] = "unrelated update"
        stale.save()

        current = load_group(created.group_id)
        assert current is not None
        target = (current.doc.get("web_model_browser_targets") or {}).get("web1") or {}
        self.assertEqual(target.get("url"), conversation_url)
        self.assertEqual(current.doc.get("topic"), "unrelated update")

    def test_same_group_field_conflict_fails_without_overwrite(self) -> None:
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry
        from cccc.util.fs import ConcurrentDocumentWriteError

        created = create_group(load_registry(), title="shared-write")
        first = load_group(created.group_id)
        stale = load_group(created.group_id)
        assert first is not None and stale is not None

        first.doc["topic"] = "first"
        first.save()
        stale.doc["topic"] = "second"
        with self.assertRaises(ConcurrentDocumentWriteError):
            stale.save()

        current = load_group(created.group_id)
        assert current is not None
        self.assertEqual(current.doc.get("topic"), "first")

    def test_stale_registry_save_preserves_new_group(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        stale = load_registry()
        created = create_group(load_registry(), title="must survive")
        stale.defaults["scope-x"] = created.group_id
        stale.save()

        current = load_registry()
        self.assertIn(created.group_id, current.groups)
        self.assertEqual(current.defaults.get("scope-x"), created.group_id)

    def test_same_registry_field_conflict_fails_without_resurrection(self) -> None:
        from cccc.kernel.registry import load_registry
        from cccc.util.fs import ConcurrentDocumentWriteError

        first = load_registry()
        stale = load_registry()
        first.defaults["scope-x"] = "g_first"
        first.save()
        stale.defaults["scope-x"] = "g_second"
        with self.assertRaises(ConcurrentDocumentWriteError):
            stale.save()

        self.assertEqual(load_registry().defaults.get("scope-x"), "g_first")

    def test_concurrent_global_setting_sections_do_not_overwrite_each_other(self) -> None:
        from cccc.kernel import settings as settings_module

        original_write = settings_module.atomic_write_text
        writes_met = threading.Barrier(2)

        def interleaved_write(path: object, text: str) -> None:
            try:
                writes_met.wait(timeout=0.25)
            except threading.BrokenBarrierError:
                pass
            original_write(path, text)

        errors: list[BaseException] = []

        def update_observability() -> None:
            try:
                settings_module.update_observability_settings({"developer_mode": True})
            except BaseException as error:  # pragma: no cover - asserted below
                errors.append(error)

        def update_branding() -> None:
            try:
                settings_module.update_web_branding_settings({"product_name": "Concurrent"})
            except BaseException as error:  # pragma: no cover - asserted below
                errors.append(error)

        with patch.object(settings_module, "atomic_write_text", side_effect=interleaved_write):
            first = threading.Thread(target=update_observability)
            second = threading.Thread(target=update_branding)
            first.start()
            second.start()
            first.join(timeout=3)
            second.join(timeout=3)

        self.assertFalse(first.is_alive())
        self.assertFalse(second.is_alive())
        self.assertEqual(errors, [])
        current = settings_module.load_settings()
        self.assertTrue(bool((current.get("observability") or {}).get("developer_mode")))
        self.assertEqual((current.get("web_branding") or {}).get("product_name"), "Concurrent")

    def test_concurrent_voice_sessions_do_not_overwrite_each_other(self) -> None:
        from cccc.daemon.assistants import assistant_ops
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        group = create_group(load_registry(), title="voice-state-write", publish=False)
        original_load = assistant_ops._load_runtime_state
        snapshots_met = threading.Barrier(2)

        def load_same_snapshot(current_group: object) -> dict[str, object]:
            state = original_load(current_group)
            snapshots_met.wait(timeout=2)
            return state

        responses: list[object] = []

        def update_session(session_id: str) -> None:
            responses.append(
                assistant_ops.handle_assistant_voice_session_update(
                    {
                        "group_id": group.group_id,
                        "session_id": session_id,
                        "by": "user",
                        "patch": {"status": "ready"},
                    }
                )
            )

        with patch.object(assistant_ops, "_load_runtime_state", side_effect=load_same_snapshot):
            first = threading.Thread(target=update_session, args=("session-a",))
            second = threading.Thread(target=update_session, args=("session-b",))
            first.start()
            second.start()
            first.join(timeout=3)
            second.join(timeout=3)

        self.assertFalse(first.is_alive())
        self.assertFalse(second.is_alive())
        self.assertTrue(all(bool(getattr(response, "ok", False)) for response in responses))
        reloaded_group = load_group(group.group_id)
        assert reloaded_group is not None
        sessions = original_load(reloaded_group).get("voice_sessions") or {}
        self.assertEqual(set(sessions), {"session-a", "session-b"})

    def test_concurrent_space_provider_updates_preserve_disjoint_fields(self) -> None:
        from cccc.daemon.space import group_space_store

        original_load = group_space_store._load_providers_doc
        snapshots_met = threading.Barrier(2)

        def load_isolated_snapshot() -> tuple[object, dict[str, object]]:
            path, doc = original_load()
            snapshot = copy.deepcopy(doc)
            snapshots_met.wait(timeout=2)
            return path, snapshot

        errors: list[BaseException] = []

        def update_enabled() -> None:
            try:
                group_space_store.set_space_provider_state("notebooklm", enabled=True)
            except BaseException as error:  # pragma: no cover - asserted below
                errors.append(error)

        def update_health() -> None:
            try:
                group_space_store.set_space_provider_state(
                    "notebooklm",
                    last_error="temporary failure",
                    touch_health=True,
                )
            except BaseException as error:  # pragma: no cover - asserted below
                errors.append(error)

        with patch.object(group_space_store, "_load_providers_doc", side_effect=load_isolated_snapshot):
            first = threading.Thread(target=update_enabled)
            second = threading.Thread(target=update_health)
            first.start()
            second.start()
            first.join(timeout=3)
            second.join(timeout=3)

        self.assertFalse(first.is_alive())
        self.assertFalse(second.is_alive())
        self.assertEqual(errors, [])
        current = group_space_store.get_space_provider_state("notebooklm")
        self.assertTrue(bool(current.get("enabled")))
        self.assertEqual(current.get("last_error"), "temporary failure")
        self.assertTrue(str(current.get("last_health_at") or ""))

    def test_concurrent_space_bindings_preserve_both_groups(self) -> None:
        from cccc.daemon.space import group_space_store
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        first_group = create_group(load_registry(), title="space-binding-a", publish=False)
        second_group = create_group(load_registry(), title="space-binding-b", publish=False)
        original_load = group_space_store._load_bindings_doc
        snapshots_met = threading.Barrier(2)

        def load_isolated_snapshot() -> tuple[object, dict[str, object]]:
            path, doc = original_load()
            snapshot = copy.deepcopy(doc)
            snapshots_met.wait(timeout=2)
            return path, snapshot

        errors: list[BaseException] = []

        def bind(group_id: str, remote_space_id: str) -> None:
            try:
                group_space_store.upsert_space_binding(
                    group_id,
                    remote_space_id=remote_space_id,
                    by="user",
                )
            except BaseException as error:  # pragma: no cover - asserted below
                errors.append(error)

        with patch.object(group_space_store, "_load_bindings_doc", side_effect=load_isolated_snapshot):
            first = threading.Thread(target=bind, args=(first_group.group_id, "remote-a"))
            second = threading.Thread(target=bind, args=(second_group.group_id, "remote-b"))
            first.start()
            second.start()
            first.join(timeout=3)
            second.join(timeout=3)

        self.assertFalse(first.is_alive())
        self.assertFalse(second.is_alive())
        self.assertEqual(errors, [])
        first_binding = group_space_store.get_space_binding(first_group.group_id)
        second_binding = group_space_store.get_space_binding(second_group.group_id)
        self.assertEqual((first_binding or {}).get("remote_space_id"), "remote-a")
        self.assertEqual((second_binding or {}).get("remote_space_id"), "remote-b")

    def test_concurrent_space_job_enqueue_preserves_idempotency(self) -> None:
        from cccc.daemon.space import group_space_store
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        group = create_group(load_registry(), title="space-job-write", publish=False)
        original_load = group_space_store._load_jobs_doc
        snapshots_met = threading.Barrier(2)
        responses: list[tuple[dict[str, object], bool]] = []
        errors: list[BaseException] = []

        def load_isolated_snapshot() -> tuple[object, dict[str, object]]:
            path, doc = original_load()
            snapshot = copy.deepcopy(doc)
            snapshots_met.wait(timeout=2)
            return path, snapshot

        def enqueue() -> None:
            try:
                responses.append(
                    group_space_store.enqueue_space_job(
                        group_id=group.group_id,
                        provider="notebooklm",
                        remote_space_id="remote-space",
                        kind="context_sync",
                        payload={"text": "same payload"},
                        idempotency_key="same-request",
                    )
                )
            except BaseException as error:  # pragma: no cover - asserted below
                errors.append(error)

        with patch.object(group_space_store, "_load_jobs_doc", side_effect=load_isolated_snapshot):
            first = threading.Thread(target=enqueue)
            second = threading.Thread(target=enqueue)
            first.start()
            second.start()
            first.join(timeout=3)
            second.join(timeout=3)

        self.assertFalse(first.is_alive())
        self.assertFalse(second.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(len(responses), 2)
        self.assertEqual(len({str(item[0].get("job_id") or "") for item in responses}), 1)
        self.assertEqual(sorted(bool(item[1]) for item in responses), [False, True])

    def test_malformed_actor_private_env_is_not_overwritten(self) -> None:
        from cccc.daemon.actors import private_env_ops

        path = private_env_ops._private_env_path("g_secret", "peer1")
        path.parent.mkdir(parents=True, exist_ok=True)
        malformed = "{not-json\n"
        path.write_text(malformed, encoding="utf-8")

        with self.assertRaises(ValueError):
            private_env_ops.update_actor_private_env(
                "g_secret",
                "peer1",
                set_vars={"NEW_TOKEN": "new"},
                unset_keys=[],
                clear=False,
            )

        self.assertEqual(path.read_text(encoding="utf-8"), malformed)

    def test_malformed_profile_secret_is_not_overwritten(self) -> None:
        from cccc.daemon.actors import actor_profile_store

        profile = actor_profile_store.upsert_actor_profile(
            {
                "id": "profile-secret",
                "name": "Profile Secret",
                "runtime": "codex",
                "runner": "pty",
            }
        )
        path = actor_profile_store._profile_secret_path(profile)
        path.parent.mkdir(parents=True, exist_ok=True)
        malformed = "{not-json\n"
        path.write_text(malformed, encoding="utf-8")

        with self.assertRaises(ValueError):
            actor_profile_store.update_actor_profile_secrets(
                profile,
                set_vars={"NEW_TOKEN": "new"},
                unset_keys=[],
                clear=False,
            )

        self.assertEqual(path.read_text(encoding="utf-8"), malformed)

    def test_malformed_space_provider_secret_is_not_overwritten(self) -> None:
        from cccc.daemon.space import group_space_store

        path = group_space_store._provider_secret_path("notebooklm")
        path.parent.mkdir(parents=True, exist_ok=True)
        malformed = "{not-json\n"
        path.write_text(malformed, encoding="utf-8")

        with self.assertRaises(ValueError):
            group_space_store.update_space_provider_secrets(
                "notebooklm",
                set_vars={"NEW_TOKEN": "new"},
                unset_keys=[],
                clear=False,
            )

        self.assertEqual(path.read_text(encoding="utf-8"), malformed)

    def test_malformed_group_bridge_authorities_are_not_overwritten(self) -> None:
        from pathlib import Path

        from cccc.kernel.group_bridge.credentials import save_pairing_bearer_token
        from cccc.kernel.group_bridge.pairing import create_pairing_invite
        from cccc.kernel.group_bridge.receipts import record_receipt
        from cccc.kernel.group_bridge.registration import upsert_registration

        home = Path(self._home.name)
        malformed = "records: [unterminated\n"
        cases = [
            (
                home / "group_bridge_credentials.yaml",
                lambda: save_pairing_bearer_token(
                    local_group_id="g_local",
                    remote_group_id="g_remote",
                    remote_endpoint="https://example.com",
                    token="secret",
                    home=home,
                ),
            ),
            (
                home / "group_bridge_pairing.yaml",
                lambda: create_pairing_invite(group_id="g_local", home=home),
            ),
            (
                home / "group_bridge_registrations.yaml",
                lambda: upsert_registration(
                    "g_local",
                    "https://example.com",
                    home=home,
                ),
            ),
            (
                home / "group_bridge_receipts.yaml",
                lambda: record_receipt(
                    "reg_local",
                    "same-request",
                    {"status": "sent"},
                    home=home,
                ),
            ),
        ]

        for path, mutate in cases:
            with self.subTest(path=path.name):
                path.write_text(malformed, encoding="utf-8")
                with self.assertRaises(ValueError):
                    mutate()
                self.assertEqual(path.read_text(encoding="utf-8"), malformed)


if __name__ == "__main__":
    unittest.main()
