import os
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch


class TestActorPrivateEnv(unittest.TestCase):
    def test_actor_recreation_does_not_inherit_residual_private_env(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.actors.private_env_ops import load_actor_private_env
        from cccc.daemon.server import handle_request

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                create, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_create", "args": {"title": "generation", "topic": "", "by": "user"}}
                    )
                )
                group_id = str((create.result or {}).get("group_id") or "").strip()
                add_args = {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "custom",
                    "runner": "headless",
                    "by": "user",
                }
                added, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "actor_add", "args": {**add_args, "env_private": {"TOKEN": "old-generation"}}}
                    )
                )
                self.assertTrue(added.ok, getattr(added, "error", None))
                removed, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "actor_remove", "args": {"group_id": group_id, "actor_id": "peer1", "by": "user"}}
                    )
                )
                self.assertTrue(removed.ok, getattr(removed, "error", None))

                # Model a legacy/post-commit cleanup failure that left generation-scoped
                # credentials behind after the actor itself was removed.
                from cccc.daemon.actors.private_env_ops import update_actor_private_env

                update_actor_private_env(
                    group_id,
                    "peer1",
                    set_vars={"TOKEN": "old-generation"},
                    unset_keys=[],
                    clear=True,
                )
                recreated, _ = handle_request(
                    DaemonRequest.model_validate({"op": "actor_add", "args": add_args})
                )
                self.assertTrue(recreated.ok, getattr(recreated, "error", None))
                self.assertEqual(load_actor_private_env(group_id, "peer1"), {})
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_private_env_updates_serialize_the_complete_read_modify_write(self) -> None:
        from cccc.daemon.actors import private_env_ops

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                barrier = threading.Barrier(2)
                original_read = private_env_ops.read_json

                def interleaved_read(path: Path):
                    value = original_read(path)
                    try:
                        barrier.wait(timeout=0.5)
                    except threading.BrokenBarrierError:
                        pass
                    return value

                errors: list[BaseException] = []

                def update(key: str) -> None:
                    try:
                        private_env_ops.update_actor_private_env(
                            "g_concurrent",
                            "peer1",
                            set_vars={key: key.lower()},
                            unset_keys=[],
                            clear=False,
                        )
                    except BaseException as error:  # pragma: no cover - asserted below
                        errors.append(error)

                with patch.object(private_env_ops, "read_json", side_effect=interleaved_read):
                    first = threading.Thread(target=update, args=("FIRST",))
                    second = threading.Thread(target=update, args=("SECOND",))
                    first.start()
                    second.start()
                    first.join(timeout=3)
                    second.join(timeout=3)

                self.assertFalse(first.is_alive())
                self.assertFalse(second.is_alive())
                self.assertEqual(errors, [])
                self.assertEqual(
                    private_env_ops.load_actor_private_env("g_concurrent", "peer1"),
                    {"FIRST": "first", "SECOND": "second"},
                )
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_private_env_user_only_permissions(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import load_group

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                create, _ = handle_request(
                    DaemonRequest.model_validate({"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}})
                )
                self.assertTrue(create.ok, getattr(create, "error", None))
                group_id = str((create.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                add_actor(
                    group,
                    actor_id="peer1",
                    title="peer1",
                    command=[],
                    env={},
                    enabled=False,
                    runner="headless",
                    runtime="codex",
                )

                denied_update, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_env_private_update",
                            "args": {
                                "group_id": group_id,
                                "actor_id": "peer1",
                                "by": "peer1",
                                "set": {"OPENAI_API_KEY": "secret"},
                            },
                        }
                    )
                )
                self.assertFalse(denied_update.ok)
                self.assertEqual(getattr(denied_update.error, "code", ""), "permission_denied")

                denied_keys, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_env_private_keys",
                            "args": {"group_id": group_id, "actor_id": "peer1", "by": "peer1"},
                        }
                    )
                )
                self.assertFalse(denied_keys.ok)
                self.assertEqual(getattr(denied_keys.error, "code", ""), "permission_denied")
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_private_env_roundtrip_and_merge(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request, _merge_actor_env_with_private
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import load_group

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                # Create a group (no scope attached; we won't start actors in this test).
                resp, _ = handle_request(
                    DaemonRequest.model_validate({"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}})
                )
                self.assertTrue(resp.ok, getattr(resp, "error", None))
                group_id = str((resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

                group = load_group(group_id)
                self.assertIsNotNone(group)

                add_actor(
                    group,
                    actor_id="peer1",
                    title="peer1",
                    command=[],
                    env={"OPENAI_API_KEY": "public"},
                    enabled=False,
                    runner="headless",
                    runtime="codex",
                )

                # Set secrets (values should never be returned).
                upd, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_env_private_update",
                            "args": {
                                "group_id": group_id,
                                "actor_id": "peer1",
                                "by": "user",
                                "set": {"OPENAI_API_KEY": "supersecret", "ANTHROPIC_API_KEY": "a"},
                            },
                        }
                    )
                )
                self.assertTrue(upd.ok, getattr(upd, "error", None))
                keys = (upd.result or {}).get("keys") or []
                self.assertIn("OPENAI_API_KEY", keys)
                self.assertIn("ANTHROPIC_API_KEY", keys)

                # List keys.
                listed, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "actor_env_private_keys", "args": {"group_id": group_id, "actor_id": "peer1", "by": "user"}}
                    )
                )
                self.assertTrue(listed.ok, getattr(listed, "error", None))
                self.assertEqual(set(listed.result.get("keys") or []), set(keys))
                masked = listed.result.get("masked_values") if isinstance(listed.result, dict) else {}
                self.assertIsInstance(masked, dict)
                assert isinstance(masked, dict)
                self.assertEqual(str(masked.get("OPENAI_API_KEY") or ""), "su******et")
                self.assertEqual(str(masked.get("ANTHROPIC_API_KEY") or ""), "******")
                self.assertNotIn("supersecret", str(masked))

                # File exists under CCCC_HOME/state/... and is user-only on POSIX.
                secret_dir = Path(td) / "state" / "secrets" / "actors" / group_id
                files = list(secret_dir.glob("*.json"))
                self.assertEqual(len(files), 1)
                if os.name != "nt":
                    mode = files[0].stat().st_mode & 0o777
                    self.assertEqual(mode, 0o600)

                # Private env overlays actor.env (private wins).
                merged = _merge_actor_env_with_private(group_id, "peer1", {"OPENAI_API_KEY": "public", "X": "1"})
                self.assertEqual(merged.get("OPENAI_API_KEY"), "supersecret")
                self.assertEqual(merged.get("X"), "1")

                # Unset one key.
                upd2, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_env_private_update",
                            "args": {"group_id": group_id, "actor_id": "peer1", "by": "user", "unset": ["ANTHROPIC_API_KEY"]},
                        }
                    )
                )
                self.assertTrue(upd2.ok, getattr(upd2, "error", None))
                self.assertNotIn("ANTHROPIC_API_KEY", upd2.result.get("keys") or [])

                # Clear all.
                clr, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "actor_env_private_update", "args": {"group_id": group_id, "actor_id": "peer1", "by": "user", "clear": True}}
                    )
                )
                self.assertTrue(clr.ok, getattr(clr, "error", None))
                self.assertEqual(clr.result.get("keys") or [], [])
                self.assertFalse(files[0].exists())
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_actor_add_can_set_env_private_before_first_start(self) -> None:
        """actor_add accepts write-only env_private (by=user) and persists it before the first start."""
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                # Create a group (no scope attached; actor_add will not start the process, but should still store secrets).
                resp, _ = handle_request(
                    DaemonRequest.model_validate({"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}})
                )
                self.assertTrue(resp.ok, getattr(resp, "error", None))
                group_id = str((resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

                add, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_add",
                            "args": {
                                "group_id": group_id,
                                "actor_id": "peer1",
                                "runner": "headless",
                                "runtime": "codex",
                                "env_private": {"OPENAI_API_KEY": "secret"},
                                "by": "user",
                            },
                        }
                    )
                )
                self.assertTrue(add.ok, getattr(add, "error", None))

                listed, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "actor_env_private_keys", "args": {"group_id": group_id, "actor_id": "peer1", "by": "user"}}
                    )
                )
                self.assertTrue(listed.ok, getattr(listed, "error", None))
                self.assertEqual(set(listed.result.get("keys") or []), {"OPENAI_API_KEY"})
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_foreman_strict_clone_copies_private_env(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                create, _ = handle_request(
                    DaemonRequest.model_validate({"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}})
                )
                self.assertTrue(create.ok, getattr(create, "error", None))
                group_id = str((create.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

                add_foreman, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_add",
                            "args": {
                                "group_id": group_id,
                                "actor_id": "lead",
                                "runner": "headless",
                                "runtime": "codex",
                                "env": {"PUBLIC_KEY": "public"},
                                "by": "user",
                            },
                        }
                    )
                )
                self.assertTrue(add_foreman.ok, getattr(add_foreman, "error", None))

                private_update, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_env_private_update",
                            "args": {
                                "group_id": group_id,
                                "actor_id": "lead",
                                "by": "user",
                                "set": {"OPENAI_API_KEY": "secret-token"},
                            },
                        }
                    )
                )
                self.assertTrue(private_update.ok, getattr(private_update, "error", None))

                add_peer, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_add",
                            "args": {
                                "group_id": group_id,
                                "actor_id": "peer1",
                                "runner": "headless",
                                "runtime": "codex",
                                "by": "lead",
                            },
                        }
                    )
                )
                self.assertTrue(add_peer.ok, getattr(add_peer, "error", None))

                listed, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "actor_env_private_keys", "args": {"group_id": group_id, "actor_id": "peer1", "by": "user"}}
                    )
                )
                self.assertTrue(listed.ok, getattr(listed, "error", None))
                self.assertEqual(set(listed.result.get("keys") or []), {"OPENAI_API_KEY"})

                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
                peer = next(
                    actor
                    for actor in actors
                    if isinstance(actor, dict) and str(actor.get("id") or "").strip() == "peer1"
                )
                self.assertEqual(dict(peer.get("env") or {}), {"PUBLIC_KEY": "public"})
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_actor_add_rejects_env_private_for_non_user(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                create, _ = handle_request(
                    DaemonRequest.model_validate({"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}})
                )
                self.assertTrue(create.ok, getattr(create, "error", None))
                group_id = str((create.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

                denied, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "actor_add",
                            "args": {
                                "group_id": group_id,
                                "actor_id": "peer1",
                                "runtime": "codex",
                                "runner": "headless",
                                "env_private": {"OPENAI_API_KEY": "secret"},
                                "by": "peer1",
                            },
                        }
                    )
                )
                self.assertFalse(denied.ok)
                self.assertEqual(getattr(denied.error, "code", ""), "actor_add_failed")
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
