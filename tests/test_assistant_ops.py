import base64
import hashlib
import io
import json
import os
import shutil
import tarfile
import tempfile
import time
import unittest
from unittest.mock import patch
from pathlib import Path
from email.message import Message


class TestAssistantOps(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            for attempt in range(5):
                try:
                    shutil.rmtree(td)
                    break
                except FileNotFoundError:
                    break
                except OSError:
                    if attempt >= 4:
                        raise
                    time.sleep(0.05)

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group(self) -> str:
        create, _ = self._call("group_create", {"title": "assistants", "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        group_id = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)
        return group_id

    def _add_foreman(self, group_id: str) -> None:
        add, _ = self._call(
            "actor_add",
            {
                "group_id": group_id,
                "by": "user",
                "actor_id": "lead",
                "title": "Foreman",
                "runtime": "codex",
                "runner": "headless",
                "enabled": True,
            },
        )
        self.assertTrue(add.ok, getattr(add, "error", None))

    def _ensure_foreman(self, group_id: str) -> None:
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group

        group = load_group(group_id)
        if group is not None and find_actor(group, "lead") is not None:
            return
        self._add_foreman(group_id)

    def _attach_scope(self, group_id: str, path: str) -> None:
        attach, _ = self._call("attach", {"group_id": group_id, "path": path, "by": "user"})
        self.assertTrue(attach.ok, getattr(attach, "error", None))

    def _enable_voice_secretary(self, group_id: str) -> None:
        self._ensure_foreman(group_id)
        from cccc.kernel.group import load_group
        from cccc.kernel.prompt_files import resolve_active_scope_root

        group = load_group(group_id)
        if group is not None and resolve_active_scope_root(group) is None:
            repo = Path(os.environ["CCCC_HOME"]) / f"repo-{group_id}"
            repo.mkdir(parents=True, exist_ok=True)
            self._attach_scope(group_id, str(repo))
        enable, _ = self._call(
            "assistant_settings_update",
            {
                "group_id": group_id,
                "by": "user",
                "assistant_id": "voice_secretary",
                "patch": {"enabled": True},
            },
        )
        self.assertTrue(enable.ok, getattr(enable, "error", None))

    def _enable_voice_secretary_service_asr(self, group_id: str) -> None:
        self._ensure_foreman(group_id)
        enable, _ = self._call(
            "assistant_settings_update",
            {
                "group_id": group_id,
                "by": "user",
                "assistant_id": "voice_secretary",
                "patch": {
                    "enabled": True,
                    "config": {
                        "capture_mode": "service",
                        "recognition_backend": "assistant_service_local_asr",
                        "retention_ttl_seconds": 120,
                        "tts_enabled": False,
                    },
                },
            },
        )
        self.assertTrue(enable.ok, getattr(enable, "error", None))

    def _write_voice_model_manifest(self, home: str, *, model_id: str = "mock_asr") -> Path:
        source_dir = Path(home) / "voice-model-source"
        source_dir.mkdir(parents=True, exist_ok=True)
        adapter_path = source_dir / "adapter.py"
        adapter_bytes = (
            "import json\n"
            "import sys\n"
            "from pathlib import Path\n"
            "\n"
            "audio = Path(sys.argv[-1])\n"
            "print(json.dumps({'text': f'managed transcript:{audio.suffix}:{audio.stat().st_size}'}))\n"
        ).encode("utf-8")
        adapter_path.write_bytes(adapter_bytes)
        manifest = {
            "voice_secretary_asr_models": [
                {
                    "model_id": model_id,
                    "kind": "asr",
                    "title": "Mock Managed ASR",
                    "command_template": ["{python}", "{model_dir}/adapter.py", "{audio_path}"],
                    "artifacts": [
                        {
                            "path": "adapter.py",
                            "url": adapter_path.as_uri(),
                            "sha256": hashlib.sha256(adapter_bytes).hexdigest(),
                            "size_bytes": len(adapter_bytes),
                        }
                    ],
                }
            ]
        }
        manifest_path = Path(home) / "config" / "voice-models.json"
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        return manifest_path

    def test_voice_model_http_metadata_hash_mismatch_fails_before_download(self) -> None:
        from cccc.daemon.assistants.voice_models import VoiceModelError, _download_artifact

        class _HeadResponse:
            headers: Message

            def __init__(self) -> None:
                self.headers = Message()
                self.headers["ETag"] = '"833ca2dcfdf8ec91bd4f31cfac36d6124e0c459074d5e909aec9cabe6204a3ea"'
                self.headers["Content-Length"] = "936291369"

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb) -> bool:
                return False

        def _fake_urlopen(req, *args, **kwargs):
            if getattr(req, "get_method", lambda: "")() == "HEAD":
                return _HeadResponse()
            raise AssertionError("download should not start after metadata mismatch")

        artifact = {
            "path": "model.pt",
            "url": "https://example.invalid/models/mock-asr/model.pt",
            "sha256": "f7cbae52854eb138167e493bf43e549bd6268d999fd9c5c0b1eef9a6ee294505",
            "size_bytes": 936291369,
        }
        with tempfile.TemporaryDirectory() as td, patch("urllib.request.urlopen", side_effect=_fake_urlopen):
            with self.assertRaises(VoiceModelError) as cm:
                _download_artifact(artifact, output_path=Path(td) / "model.pt")
        self.assertEqual(cm.exception.code, "voice_model_manifest_hash_invalid")

    def test_voice_model_manifest_rejects_unsafe_install_paths(self) -> None:
        from cccc.daemon.assistants.voice_models import VoiceModelError, load_voice_model_catalog

        digest = "a" * 64

        def manifest(**entry_overrides: object) -> str:
            entry = {
                "model_id": "safe_asr",
                "kind": "asr",
                "title": "Safe ASR",
                "command_template": ["{python}", "{model_dir}/adapter.py", "{audio_path}"],
                "artifacts": [{"path": "adapter.py", "url": "file:///tmp/adapter.py", "sha256": digest}],
            }
            entry.update(entry_overrides)
            return json.dumps({"voice_secretary_asr_models": [entry]})

        cases = [
            {"model_id": "../escape"},
            {"artifacts": [{"path": "../escape.py", "url": "file:///tmp/adapter.py", "sha256": digest}]},
            {
                "streaming": {
                    "engine": "zipformer2_ctc",
                    "tokens": "../tokens.txt",
                    "model": "model.onnx",
                }
            },
            {"required_files": ["../../outside.onnx"]},
        ]
        for overrides in cases:
            with self.subTest(overrides=overrides):
                with self.assertRaises(VoiceModelError) as cm:
                    load_voice_model_catalog(manifest(**overrides))
                self.assertEqual(cm.exception.code, "voice_model_manifest_invalid")

    def test_voice_model_archive_rejects_links_and_special_entries(self) -> None:
        from cccc.daemon.assistants.voice_models import VoiceModelError, _safe_extract_tar

        with tempfile.TemporaryDirectory() as td:
            archive_path = Path(td) / "bad.tar"
            with tarfile.open(archive_path, "w") as archive:
                info = tarfile.TarInfo("safe.txt")
                payload = b"ok"
                info.size = len(payload)
                archive.addfile(info, io.BytesIO(payload))
                link = tarfile.TarInfo("escape-link")
                link.type = tarfile.SYMTYPE
                link.linkname = "/etc/passwd"
                archive.addfile(link)
            with self.assertRaises(VoiceModelError) as cm:
                _safe_extract_tar(archive_path, Path(td) / "out")
        self.assertEqual(cm.exception.code, "voice_model_archive_invalid")

    def test_default_voice_catalog_prefers_zipformer_transducer_model(self) -> None:
        from cccc.daemon.assistants.voice_models import load_voice_model_catalog

        catalog = load_voice_model_catalog()
        model_ids = list(catalog.keys())
        self.assertGreater(len(model_ids), 0)
        self.assertEqual(model_ids[0], "sherpa_onnx_streaming_zipformer_zh_int8")
        streaming = catalog[model_ids[0]].get("streaming") or {}
        self.assertEqual(streaming.get("engine"), "transducer")
        self.assertTrue(str(streaming.get("encoder") or "").endswith("encoder.int8.onnx"))
        self.assertTrue(str(streaming.get("decoder") or "").endswith("decoder.onnx"))
        self.assertTrue(str(streaming.get("joiner") or "").endswith("joiner.int8.onnx"))
        self.assertTrue(str(streaming.get("tokens") or "").endswith("tokens.txt"))

    def test_sherpa_streaming_model_resolution_falls_back_from_invalid_selection(self) -> None:
        from cccc.daemon.assistants import sherpa_streaming_asr

        def _status(model_id: str) -> dict:
            if model_id == "ready_streaming":
                return {
                    "model_id": model_id,
                    "runtime_id": "sherpa_onnx_streaming",
                    "streaming_ready": True,
                }
            return {"model_id": model_id, "status": "unknown", "available": False}

        with patch.object(sherpa_streaming_asr, "get_voice_model_status", side_effect=_status), patch.object(
            sherpa_streaming_asr,
            "list_voice_models",
            return_value=[
                {
                    "model_id": "ready_streaming",
                    "runtime_id": "sherpa_onnx_streaming",
                    "streaming_ready": True,
                }
            ],
        ):
            self.assertEqual(
                sherpa_streaming_asr.resolve_sherpa_streaming_model_id("sensevoice_small"),
                "ready_streaming",
            )

    def test_voice_capture_result_idle_counts_as_continuous(self) -> None:
        from cccc.daemon.assistants.assistant_ops import _voice_capture_continuity

        self.assertEqual(
            _voice_capture_continuity(segment_count=2, trigger={"trigger_kind": "result_idle"}),
            "continuous",
        )
        self.assertEqual(
            _voice_capture_continuity(segment_count=2, trigger={"trigger_kind": "unknown"}),
            "fragmented",
        )

    def test_state_exposes_builtin_assistants(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            state, _ = self._call("assistant_state", {"group_id": group_id})

            self.assertTrue(state.ok, getattr(state, "error", None))
            assistants_by_id = (state.result or {}).get("assistants_by_id") if isinstance(state.result, dict) else {}
            self.assertIn("pet", assistants_by_id)
            self.assertIn("voice_secretary", assistants_by_id)
            self.assertFalse(bool(assistants_by_id["voice_secretary"].get("enabled")))
            self.assertEqual(assistants_by_id["voice_secretary"].get("lifecycle"), "disabled")
            self.assertEqual((assistants_by_id["voice_secretary"].get("config") or {}).get("recognition_backend"), "browser_asr")
            self.assertEqual((assistants_by_id["voice_secretary"].get("config") or {}).get("recognition_language"), "mixed")
        finally:
            cleanup()

    def test_voice_state_sanitizes_legacy_chunking_config_on_read(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group

            group_id = self._create_group()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["assistants"] = {
                "voice_secretary": {
                    "enabled": False,
                    "config": {
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "recognition_language": "ja-JP",
                        "auto_document_quiet_ms": 300,
                        "auto_document_max_window_seconds": 5,
                        "dispatch_mode": "confirm_then_dispatch",
                        "auto_proposal_enabled": False,
                    },
                }
            }
            group.save()

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})

            self.assertTrue(state.ok, getattr(state, "error", None))
            assistant = (state.result or {}).get("assistant") if isinstance(state.result, dict) else {}
            config = assistant.get("config") if isinstance(assistant.get("config"), dict) else {}
            self.assertEqual(config.get("auto_document_quiet_ms"), 1000)
            self.assertEqual(config.get("auto_document_max_window_seconds"), 10)
            self.assertNotIn("dispatch_mode", config)
            self.assertNotIn("auto_proposal_enabled", config)

            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {"config": {"recognition_language": "en-US"}},
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))
            updated_config = ((update.result or {}).get("assistant") or {}).get("config") or {}
            self.assertEqual(updated_config.get("recognition_language"), "en-US")
            self.assertEqual(updated_config.get("auto_document_quiet_ms"), 1000)
            self.assertEqual(updated_config.get("auto_document_max_window_seconds"), 10)
            self.assertNotIn("dispatch_mode", updated_config)
            self.assertNotIn("auto_proposal_enabled", updated_config)
        finally:
            cleanup()

    def test_voice_state_read_does_not_flush_stale_session_windows(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            with patch(
                "cccc.daemon.assistants.assistant_ops._flush_stale_voice_session_windows",
                side_effect=AssertionError("assistant_state must not flush stale voice windows"),
            ):
                state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})

            self.assertTrue(state.ok, getattr(state, "error", None))
        finally:
            cleanup()

    def test_voice_document_list_does_not_flush_stale_session_windows(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            with patch(
                "cccc.daemon.assistants.assistant_ops._flush_stale_voice_session_windows",
                side_effect=AssertionError("document list must not flush stale voice windows"),
            ):
                documents, _ = self._call("assistant_voice_document_list", {"group_id": group_id})

            self.assertTrue(documents.ok, getattr(documents, "error", None))
        finally:
            cleanup()

    def test_voice_settings_update_enables_group_scoped_assistant(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._add_foreman(group_id)
            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {
                        "enabled": True,
                        "config": {
                            "capture_mode": "service",
                            "recognition_backend": "assistant_service_local_asr",
                            "retention_ttl_seconds": 120,
                            "tts_enabled": False,
                        },
                    },
                },
            )

            self.assertTrue(update.ok, getattr(update, "error", None))
            assistant = (update.result or {}).get("assistant") if isinstance(update.result, dict) else {}
            self.assertTrue(bool(assistant.get("enabled")))
            self.assertEqual((assistant.get("config") or {}).get("retention_ttl_seconds"), 120)
            service = ((assistant.get("health") or {}).get("service") or {}) if isinstance(assistant.get("health"), dict) else {}
            self.assertEqual(service.get("status"), "not_started")
            self.assertFalse(bool(service.get("alive")))
            event = (update.result or {}).get("event") if isinstance(update.result, dict) else {}
            self.assertEqual(event.get("kind"), "assistant.settings_update")
            from cccc.kernel.group import load_group
            from cccc.kernel.voice_secretary_actor import VOICE_SECRETARY_ACTOR_ID, get_voice_secretary_actor

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            actor = get_voice_secretary_actor(group)
            self.assertIsInstance(actor, dict)
            self.assertEqual(actor.get("id"), VOICE_SECRETARY_ACTOR_ID)
            self.assertEqual(actor.get("internal_kind"), "voice_secretary")
            self.assertEqual(actor.get("runtime"), "codex")
            self.assertEqual(actor.get("runner"), "headless")
            from cccc.kernel.system_prompt import render_system_prompt

            prompt = render_system_prompt(group=group, actor=actor)
            self.assertIn("Voice Secretary Runtime Actor", prompt)
            self.assertIn("not the foreman", prompt)
            self.assertIn("secretary-scope", prompt)
            self.assertIn("use MCP tools cccc_bootstrap, then cccc_help", prompt)
            self.assertIn("daemon-delivered input_envelope", prompt)
            self.assertIn("Work order", prompt)
            self.assertIn("Context (not task)", prompt)
            self.assertIn("Do not call read_new_input first when an input_envelope is present", prompt)
            self.assertIn("reply_text", prompt)
            self.assertIn("Console text alone is not delivered", prompt)
            self.assertIn("Latency matters", prompt)
            self.assertNotIn("first instruction after cold start or resume", prompt)
            self.assertNotIn("wait until the first runtime instruction", prompt)
            self.assertIn("Do not edit project code", prompt)
            self.assertIn("cccc_voice_secretary_document", prompt)
            self.assertIn("cccc_voice_secretary_request", prompt)
            self.assertIn("never copy them into user-facing markdown", prompt)
            self.assertIn("Maintain the target document incrementally", prompt)
            self.assertIn("non-lossy editorial pass", prompt)
            self.assertIn("evidence-bounded reconstructions", prompt)
            self.assertIn("Do not fabricate facts", prompt)
            self.assertIn("finished secretary work", prompt)
            self.assertIn("preserve names, dates, numbers", prompt)
            self.assertIn("edit repository markdown directly", prompt)
            self.assertIn("has no save action", prompt)
            self.assertNotIn("Save full revised markdown", prompt)
        finally:
            cleanup()

    def test_voice_secretary_capability_state_exposes_document_tool(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            state, _ = self._call(
                "capability_state",
                {
                    "group_id": group_id,
                    "actor_id": "voice-secretary",
                    "by": "voice-secretary",
                },
            )

            self.assertTrue(state.ok, getattr(state, "error", None))
            visible_tools = set((state.result or {}).get("visible_tools") or [])
            self.assertIn("cccc_voice_secretary_document", visible_tools)
            self.assertIn("cccc_voice_secretary_request", visible_tools)
            self.assertNotIn("cccc_pet_decisions", visible_tools)
            self.assertNotIn("cccc_message_send", visible_tools)
            self.assertNotIn("cccc_message_reply", visible_tools)
        finally:
            cleanup()

    def test_voice_settings_enable_uses_foreman_profile_private_env(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            profile_upsert, _ = self._call(
                "actor_profile_upsert",
                {
                    "by": "user",
                    "caller_id": "user-a",
                    "is_admin": False,
                    "profile": {
                        "id": "voice-profile",
                        "name": "Voice Profile",
                        "scope": "user",
                        "owner_id": "user-a",
                        "runtime": "custom",
                        "runner": "headless",
                        "command": ["voice-runtime", "--stdio"],
                        "submit": "newline",
                    },
                },
            )
            self.assertTrue(profile_upsert.ok, getattr(profile_upsert, "error", None))

            secret_update, _ = self._call(
                "actor_profile_secret_update",
                {
                    "by": "user",
                    "profile_id": "voice-profile",
                    "profile_scope": "user",
                    "profile_owner": "user-a",
                    "caller_id": "user-a",
                    "is_admin": False,
                    "set": {"API_KEY": "voice-secret"},
                },
            )
            self.assertTrue(secret_update.ok, getattr(secret_update, "error", None))

            add_foreman, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "lead",
                    "runtime": "codex",
                    "runner": "headless",
                    "profile_id": "voice-profile",
                    "profile_scope": "user",
                    "profile_owner": "user-a",
                    "caller_id": "user-a",
                    "is_admin": False,
                    "by": "user",
                },
            )
            self.assertTrue(add_foreman.ok, getattr(add_foreman, "error", None))
            stop_foreman, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "lead",
                    "patch": {"enabled": False},
                },
            )
            self.assertTrue(stop_foreman.ok, getattr(stop_foreman, "error", None))

            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "caller_id": "user-a",
                    "is_admin": False,
                    "assistant_id": "voice_secretary",
                    "patch": {"enabled": True},
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))

            from cccc.daemon.actors.private_env_ops import load_actor_private_env
            from cccc.kernel.group import load_group
            from cccc.kernel.voice_secretary_actor import VOICE_SECRETARY_ACTOR_ID, get_voice_secretary_actor

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            actor = get_voice_secretary_actor(group)
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(actor.get("id"), VOICE_SECRETARY_ACTOR_ID)
            self.assertEqual(actor.get("runtime"), "custom")
            self.assertEqual(actor.get("runner"), "headless")
            self.assertEqual(actor.get("command"), ["voice-runtime", "--stdio"])
            self.assertEqual(actor.get("submit"), "newline")
            private_env = load_actor_private_env(group_id, VOICE_SECRETARY_ACTOR_ID)
            self.assertEqual(private_env.get("API_KEY"), "voice-secret")
        finally:
            cleanup()

    def test_voice_settings_sync_preserves_linked_voice_profile_runtime_config(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            profile_upsert, _ = self._call(
                "actor_profile_upsert",
                {
                    "by": "user",
                    "profile": {
                        "id": "voice-profile",
                        "name": "Voice PTY",
                        "scope": "global",
                        "runtime": "codex",
                        "runner": "pty",
                        "command": ["codex", "-m", "gpt-5.3-codex-spark"],
                        "submit": "newline",
                    },
                },
            )
            self.assertTrue(profile_upsert.ok, getattr(profile_upsert, "error", None))

            from cccc.kernel.group import load_group
            from cccc.kernel.actors import INTERNAL_KIND_VOICE_SECRETARY
            from cccc.kernel.voice_secretary_actor import get_voice_secretary_actor

            link, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "voice-secretary",
                    "profile_id": "voice-profile",
                },
            )
            self.assertTrue(link.ok, getattr(link, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            actor = get_voice_secretary_actor(group)
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(actor.get("profile_id"), "voice-profile")
            self.assertEqual(actor.get("internal_kind"), INTERNAL_KIND_VOICE_SECRETARY)
            self.assertEqual(actor.get("runtime"), "codex")
            self.assertEqual(actor.get("runner"), "pty")
            self.assertEqual(actor.get("command"), ["codex", "-m", "gpt-5.3-codex-spark"])
            self.assertEqual(actor.get("submit"), "newline")

            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {"config": {"recognition_language": "zh-CN"}},
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            actor = get_voice_secretary_actor(group)
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(actor.get("profile_id"), "voice-profile")
            self.assertEqual(actor.get("profile_scope"), "global")
            self.assertEqual(actor.get("internal_kind"), INTERNAL_KIND_VOICE_SECRETARY)
            self.assertEqual(actor.get("runtime"), "codex")
            self.assertEqual(actor.get("runner"), "pty")
            self.assertEqual(actor.get("command"), ["codex", "-m", "gpt-5.3-codex-spark"])
            self.assertEqual(actor.get("submit"), "newline")
        finally:
            cleanup()

    def test_voice_actor_update_allows_runtime_profile_link_without_losing_voice_tools(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            profile_upsert, _ = self._call(
                "actor_profile_upsert",
                {
                    "by": "user",
                    "profile": {
                        "id": "voice-pty-profile",
                        "name": "Voice PTY",
                        "scope": "global",
                        "runtime": "codex",
                        "runner": "pty",
                        "command": ["codex"],
                        "submit": "enter",
                    },
                },
            )
            self.assertTrue(profile_upsert.ok, getattr(profile_upsert, "error", None))

            from cccc.kernel.group import load_group
            from cccc.kernel.voice_secretary_actor import get_voice_secretary_actor
            from cccc.ports.mcp.server import list_tools_for_caller

            update_profile, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "voice-secretary",
                    "profile_id": "voice-pty-profile",
                },
            )
            self.assertTrue(update_profile.ok, getattr(update_profile, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            actor = get_voice_secretary_actor(group)
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertEqual(actor.get("profile_id"), "voice-pty-profile")
            self.assertEqual(actor.get("runtime"), "codex")
            self.assertEqual(actor.get("runner"), "pty")
            self.assertEqual(actor.get("internal_kind"), "voice_secretary")

            with patch.dict(os.environ, {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "voice-secretary"}, clear=False), patch(
                "cccc.ports.mcp.server._call_daemon_or_raise",
                side_effect=RuntimeError("daemon unavailable"),
            ):
                tools = list_tools_for_caller()
            names = {str(item.get("name") or "") for item in tools if isinstance(item, dict)}
            self.assertIn("cccc_voice_secretary_document", names)
            self.assertIn("cccc_voice_secretary_request", names)
            self.assertIn("cccc_voice_secretary_composer", names)

            convert, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "voice-secretary",
                    "profile_action": "convert_to_custom",
                },
            )
            self.assertTrue(convert.ok, getattr(convert, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            actor = get_voice_secretary_actor(group)
            self.assertIsInstance(actor, dict)
            assert isinstance(actor, dict)
            self.assertNotIn("profile_id", actor)

            update_runtime, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "voice-secretary",
                    "patch": {"runner": "pty", "runtime": "codex"},
                },
            )
            self.assertTrue(update_runtime.ok, getattr(update_runtime, "error", None))
            actor = (update_runtime.result or {}).get("actor") if isinstance(update_runtime.result, dict) else {}
            self.assertEqual(actor.get("runtime"), "codex")
            self.assertEqual(actor.get("runner"), "pty")
        finally:
            cleanup()

    def test_voice_settings_enable_requires_foreman_runtime_source(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {"enabled": True},
                },
            )

            self.assertFalse(update.ok)
            self.assertEqual(update.error.code, "assistant_settings_update_failed")
            self.assertIn("voice secretary requires a foreman actor", update.error.message)
        finally:
            cleanup()

    def test_voice_service_transcribe_reports_missing_backend_explicitly(self) -> None:
        _, cleanup = self._with_home()
        old_mock = os.environ.pop("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT", None)
        old_command = os.environ.pop("CCCC_VOICE_SECRETARY_ASR_COMMAND", None)
        try:
            group_id = self._create_group()
            self._enable_voice_secretary_service_asr(group_id)
            audio = base64.b64encode(b"fake audio bytes").decode("ascii")

            transcribe, _ = self._call(
                "assistant_voice_transcribe",
                {
                    "group_id": group_id,
                    "by": "user",
                    "audio_base64": audio,
                    "mime_type": "audio/webm",
                    "language": "en-US",
                },
            )

            self.assertFalse(transcribe.ok)
            self.assertEqual(transcribe.error.code, "asr_backend_unavailable")
        finally:
            from cccc.daemon.assistants.voice_service_runtime import stop_voice_service
            from cccc.kernel.group import load_group

            group = load_group(group_id) if "group_id" in locals() else None
            if group is not None:
                stop_voice_service(group)
            if old_mock is not None:
                os.environ["CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT"] = old_mock
            else:
                os.environ.pop("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT", None)
            if old_command is not None:
                os.environ["CCCC_VOICE_SECRETARY_ASR_COMMAND"] = old_command
            else:
                os.environ.pop("CCCC_VOICE_SECRETARY_ASR_COMMAND", None)
            cleanup()

    def test_voice_service_transcribe_uses_first_party_service_process(self) -> None:
        _, cleanup = self._with_home()
        old_mock = os.environ.get("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT")
        old_command = os.environ.get("CCCC_VOICE_SECRETARY_ASR_COMMAND")
        os.environ["CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT"] = "service transcript"
        os.environ.pop("CCCC_VOICE_SECRETARY_ASR_COMMAND", None)
        try:
            from cccc.daemon.assistants.voice_service_runtime import read_voice_service_state, stop_voice_service
            from cccc.kernel.group import load_group

            group_id = self._create_group()
            self._enable_voice_secretary_service_asr(group_id)
            audio = base64.b64encode(b"fake audio bytes").decode("ascii")

            transcribe, _ = self._call(
                "assistant_voice_transcribe",
                {
                    "group_id": group_id,
                    "by": "user",
                    "audio_base64": audio,
                    "mime_type": "audio/webm",
                    "language": "en-US",
                },
            )

            self.assertTrue(transcribe.ok, getattr(transcribe, "error", None))
            result = transcribe.result or {}
            self.assertEqual(result.get("transcript"), "service transcript")
            self.assertEqual(result.get("backend"), "assistant_service_local_asr")
            service = result.get("service") if isinstance(result.get("service"), dict) else {}
            self.assertTrue(bool(service.get("pid")))
            self.assertTrue(bool(service.get("asr_mock_configured")))
            assistant = result.get("assistant") if isinstance(result.get("assistant"), dict) else {}
            self.assertEqual(assistant.get("lifecycle"), "idle")
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            state = read_voice_service_state(group)
            self.assertTrue(bool(state.get("alive")))
            stop_voice_service(group)
        finally:
            if old_mock is not None:
                os.environ["CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT"] = old_mock
            else:
                os.environ.pop("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT", None)
            if old_command is not None:
                os.environ["CCCC_VOICE_SECRETARY_ASR_COMMAND"] = old_command
            else:
                os.environ.pop("CCCC_VOICE_SECRETARY_ASR_COMMAND", None)
            if "group" in locals() and group is not None:
                from cccc.daemon.assistants.voice_service_runtime import stop_voice_service

                stop_voice_service(group)
            cleanup()

    def test_voice_state_exposes_managed_model_status(self) -> None:
        home, cleanup = self._with_home()
        try:
            self._write_voice_model_manifest(home)
            group_id = self._create_group()
            self._ensure_foreman(group_id)
            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {
                        "enabled": True,
                        "config": {
                            "capture_mode": "service",
                            "recognition_backend": "assistant_service_local_asr",
                            "service_model_id": "mock_asr",
                            "tts_enabled": False,
                        },
                    },
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            assistant = (state.result or {}).get("assistant") if isinstance(state.result, dict) else {}
            service = ((assistant.get("health") or {}).get("service") or {}) if isinstance(assistant.get("health"), dict) else {}
            managed = service.get("managed_model") if isinstance(service.get("managed_model"), dict) else {}
            self.assertEqual(str(service.get("selected_model_id") or ""), "mock_asr")
            self.assertEqual(str(managed.get("status") or ""), "not_installed")
            service_models = (state.result or {}).get("service_models") if isinstance(state.result, dict) else []
            service_models_by_id = {
                str(item.get("model_id") or ""): item for item in service_models if isinstance(item, dict)
            }
            self.assertIn("mock_asr", service_models_by_id)
            service_runtimes = (state.result or {}).get("service_runtimes_by_id") if isinstance(state.result, dict) else {}
            self.assertIn("sherpa_onnx_streaming", service_runtimes)
        finally:
            cleanup()

    def test_voice_model_install_and_transcribe_uses_managed_command(self) -> None:
        home, cleanup = self._with_home()
        old_mock = os.environ.pop("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT", None)
        old_command = os.environ.pop("CCCC_VOICE_SECRETARY_ASR_COMMAND", None)
        group = None
        try:
            from cccc.daemon.assistants.voice_service_runtime import stop_voice_service
            from cccc.kernel.group import load_group

            self._write_voice_model_manifest(home)
            group_id = self._create_group()
            self._ensure_foreman(group_id)
            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {
                        "enabled": True,
                        "config": {
                            "capture_mode": "service",
                            "recognition_backend": "assistant_service_local_asr",
                            "service_model_id": "mock_asr",
                            "tts_enabled": False,
                        },
                    },
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))

            install, _ = self._call(
                "assistant_voice_model_install",
                {"group_id": group_id, "model_id": "mock_asr", "by": "user"},
            )
            self.assertTrue(install.ok, getattr(install, "error", None))
            installed_model = (install.result or {}).get("model") if isinstance(install.result, dict) else {}
            self.assertEqual(str(installed_model.get("status") or ""), "ready")

            audio = base64.b64encode(b"fake audio bytes").decode("ascii")
            transcribe, _ = self._call(
                "assistant_voice_transcribe",
                {
                    "group_id": group_id,
                    "by": "user",
                    "audio_base64": audio,
                    "mime_type": "audio/webm",
                    "language": "en-US",
                },
            )
            self.assertTrue(transcribe.ok, getattr(transcribe, "error", None))
            result = transcribe.result or {}
            self.assertEqual(str(result.get("transcript") or ""), "managed transcript:.webm:16")
            service = result.get("service") if isinstance(result.get("service"), dict) else {}
            managed = service.get("managed_model") if isinstance(service.get("managed_model"), dict) else {}
            self.assertEqual(str(managed.get("status") or ""), "ready")

            group = load_group(group_id)
            if group is not None:
                stop_voice_service(group)
        finally:
            if group is not None:
                from cccc.daemon.assistants.voice_service_runtime import stop_voice_service

                stop_voice_service(group)
            if old_mock is not None:
                os.environ["CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT"] = old_mock
            else:
                os.environ.pop("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT", None)
            if old_command is not None:
                os.environ["CCCC_VOICE_SECRETARY_ASR_COMMAND"] = old_command
            else:
                os.environ.pop("CCCC_VOICE_SECRETARY_ASR_COMMAND", None)
            cleanup()

    def test_voice_document_create_requires_attached_repo_scope(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            create, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "title": "Notes",
                    "create_new": True,
                },
            )
            self.assertFalse(create.ok)
            self.assertEqual(create.error.code, "assistant_voice_document_save_failed")
            self.assertIn("attached repository scope", create.error.message)
        finally:
            cleanup()

    def test_voice_document_list_discovers_repo_markdown_documents(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            docs_dir = repo / "docs" / "voice-secretary"
            docs_dir.mkdir(parents=True)
            imported_path = docs_dir / "imported-memo.md"
            imported_path.write_text(
                "---\ntitle: Imported Memo\n---\n\n# Ignored Heading\n\nExternal document body.\n",
                encoding="utf-8",
            )
            archive_dir = docs_dir / "archive"
            archive_dir.mkdir()
            (archive_dir / "old-memo.md").write_text("# Old Memo\n\nArchived by filename location.\n", encoding="utf-8")
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            documents_by_path = (state.result or {}).get("documents_by_path") if isinstance(state.result, dict) else {}
            imported_rel = "docs/voice-secretary/imported-memo.md"
            archived_rel = "docs/voice-secretary/archive/old-memo.md"
            self.assertIn(imported_rel, documents_by_path)
            self.assertNotIn(archived_rel, documents_by_path)
            imported = documents_by_path[imported_rel]
            self.assertEqual(imported.get("title"), "Imported Memo")
            self.assertEqual(imported.get("storage_kind"), "workspace")
            self.assertIn("External document body.", str(imported.get("content") or ""))

            select, _ = self._call(
                "assistant_voice_document_select",
                {"group_id": group_id, "by": "user", "document_path": imported_rel},
            )
            self.assertTrue(select.ok, getattr(select, "error", None))
            selected_doc = (select.result or {}).get("document") if isinstance(select.result, dict) else {}
            self.assertEqual(selected_doc.get("document_path"), imported_rel)

            after_select, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(after_select.ok, getattr(after_select, "error", None))
            self.assertEqual((after_select.result or {}).get("active_document_path"), imported_rel)

            save, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "document_path": imported_rel,
                    "content": "# Imported Memo\n\nUpdated through the Voice Secretary panel.\n",
                },
            )
            self.assertTrue(save.ok, getattr(save, "error", None))
            self.assertIn("Updated through", imported_path.read_text(encoding="utf-8"))

            external_content = "# Imported Memo\n\nEdited directly by an agent.\n"
            imported_path.write_text(external_content, encoding="utf-8")
            lightweight, _ = self._call(
                "assistant_voice_document_list",
                {"group_id": group_id, "include_content": False, "document_path": imported_rel},
            )
            self.assertTrue(lightweight.ok, getattr(lightweight, "error", None))
            lightweight_documents = (lightweight.result or {}).get("documents") if isinstance(lightweight.result, dict) else []
            self.assertEqual(len(lightweight_documents), 1)
            self.assertNotIn("content", lightweight_documents[0])
            self.assertEqual(lightweight_documents[0].get("content_chars"), len(external_content))

            archive, _ = self._call(
                "assistant_voice_document_archive",
                {"group_id": group_id, "by": "user", "document_path": imported_rel},
            )
            self.assertTrue(archive.ok, getattr(archive, "error", None))
            self.assertFalse(imported_path.exists())
            archived_doc = (archive.result or {}).get("document") if isinstance(archive.result, dict) else {}
            self.assertIn("docs/voice-secretary/archive/", str(archived_doc.get("workspace_path") or ""))

            after_archive, _ = self._call("assistant_voice_document_list", {"group_id": group_id})
            self.assertTrue(after_archive.ok, getattr(after_archive, "error", None))
            paths_after_archive = {
                str(item.get("document_path") or "")
                for item in ((after_archive.result or {}).get("documents") or [])
                if isinstance(item, dict)
            }
            self.assertNotIn(imported_rel, paths_after_archive)
            self.assertNotIn(archived_rel, paths_after_archive)
        finally:
            cleanup()

    def test_voice_document_active_target_ignores_deleted_workspace_file(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)
            created_doc, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "title": "Existing Notes",
                    "create_new": True,
                },
            )
            self.assertTrue(created_doc.ok, getattr(created_doc, "error", None))

            created, _ = self._call(
                "assistant_voice_document_save",
                {"group_id": group_id, "by": "user", "title": "Temporary Notes", "create_new": True},
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            document = (created.result or {}).get("document") if isinstance(created.result, dict) else {}
            document_path = str(document.get("document_path") or "")
            self.assertTrue(document_path)
            absolute_path = repo / document_path
            self.assertTrue(absolute_path.exists())

            absolute_path.unlink()

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            self.assertEqual((state.result or {}).get("active_document_id"), "")
            self.assertEqual((state.result or {}).get("active_document_path"), "")
            documents_by_path = (state.result or {}).get("documents_by_path") if isinstance(state.result, dict) else {}
            self.assertNotIn(document_path, documents_by_path)

            appended, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "session_id": "s1",
                    "text": "new note after external deletion",
                    "is_final": True,
                    "flush": True,
                    "flush_reason": "result_idle",
                    "trigger": {"trigger_kind": "push_to_talk_stop"},
                },
            )
            self.assertTrue(appended.ok, getattr(appended, "error", None))
            self.assertFalse(absolute_path.exists())
            next_document = (appended.result or {}).get("document") if isinstance(appended.result, dict) else {}
            self.assertNotEqual(str(next_document.get("document_path") or ""), document_path)
        finally:
            cleanup()

    def test_voice_transcript_append_flush_stores_sidecar_without_legacy_proposal(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import iter_events

            group_id = self._create_group()
            self._add_foreman(group_id)
            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {
                        "enabled": True,
                        "config": {
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "recognition_language": "en-US",
                            "retention_ttl_seconds": 120,
                            "auto_document_enabled": False,
                            "tts_enabled": False,
                        },
                    },
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))

            append, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "session-1",
                    "segment_id": "seg-1",
                    "text": "please inspect the assistant transcript loop",
                    "language": "en-US",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "session-1",
                        "input_device_label": "browser_default",
                        "language": "en-US",
                    },
                },
            )

            self.assertTrue(append.ok, getattr(append, "error", None))
            result = append.result or {}
            self.assertEqual(str(result.get("session_id") or ""), "session-1")
            segment_path = Path(str(result.get("segment_path") or ""))
            self.assertTrue(segment_path.exists(), segment_path)
            self.assertEqual(
                segment_path,
                Path(home) / "voice-secretary" / group_id / "session-1" / "transcripts" / "segments.jsonl",
            )
            stored_segment = json.loads(segment_path.read_text(encoding="utf-8").splitlines()[-1])
            self.assertEqual(stored_segment.get("segment_id"), "seg-1")
            self.assertEqual(stored_segment.get("text"), "please inspect the assistant transcript loop")
            self.assertEqual(stored_segment.get("language"), "en-US")

            assistant = result.get("assistant") if isinstance(result.get("assistant"), dict) else {}
            self.assertEqual(assistant.get("lifecycle"), "idle")

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            events = list(iter_events(group.ledger_path))
            self.assertTrue(any(event.get("kind") == "assistant.settings_update" for event in events))
        finally:
            cleanup()

    def test_voice_transcript_append_creates_repo_backed_working_document_by_default(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import iter_events

            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            append, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "doc-session-1",
                    "segment_id": "seg-doc-1",
                    "text": "summarize the billing API migration plan and keep rollout risks visible",
                    "language": "en-US",
                    "is_final": True,
                    "flush": False,
                    "trigger": {
                        "mode": "meeting",
                            "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "doc-session-1",
                        "input_device_label": "browser_default",
                        "language": "en-US",
                    },
                },
            )

            self.assertTrue(append.ok, getattr(append, "error", None))
            result = append.result or {}
            self.assertFalse(bool(result.get("document_updated")), result)
            self.assertFalse(bool(result.get("input_event_created")), result)

            segment_path = Path(str(result.get("segment_path") or ""))
            self.assertEqual(
                segment_path,
                Path(home) / "voice-secretary" / group_id / "doc-session-1" / "transcripts" / "segments.jsonl",
            )

            flush, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "doc-session-1",
                    "segment_id": "",
                    "text": "",
                    "language": "en-US",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "doc-session-1",
                        "input_device_label": "browser_default",
                        "language": "en-US",
                    },
                },
            )
            self.assertTrue(flush.ok, getattr(flush, "error", None))
            flush_result = flush.result or {}
            self.assertFalse(bool(flush_result.get("document_updated")), flush_result)
            self.assertTrue(bool(flush_result.get("input_event_created")), flush_result)
            self.assertTrue(bool(flush_result.get("input_notify_emitted")), flush_result)
            document = flush_result.get("document") if isinstance(flush_result.get("document"), dict) else {}
            input_event = flush_result.get("input_event") if isinstance(flush_result.get("input_event"), dict) else {}
            self.assertEqual(input_event.get("kind"), "asr_transcript")
            self.assertEqual(input_event.get("document_path"), document.get("document_path"))
            self.assertEqual(document.get("storage_kind"), "workspace")
            original_document_title = str(document.get("title") or "")
            workspace_path = str(document.get("workspace_path") or "")
            self.assertTrue(workspace_path.startswith("docs/voice-secretary/"), workspace_path)
            self.assertTrue(workspace_path.endswith(".md"), workspace_path)
            document_path = repo / workspace_path
            self.assertTrue(document_path.exists(), document_path)
            content = document_path.read_text(encoding="utf-8")
            self.assertEqual(content, "")
            self.assertNotIn("Voice Secretary Notes", content)
            self.assertNotIn("## Working Notes", content)
            self.assertNotIn("summarize the billing API migration plan", content)
            sources_path = Path(home) / "voice-secretary" / group_id / "documents" / str(document.get("document_id")) / "sources.jsonl"
            revisions_path = Path(home) / "voice-secretary" / group_id / "documents" / str(document.get("document_id")) / "revisions.jsonl"
            self.assertTrue(sources_path.exists(), sources_path)
            self.assertFalse(revisions_path.exists())

            input_read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(input_read.ok, getattr(input_read, "error", None))
            input_read_result = input_read.result or {}
            self.assertEqual(input_read_result.get("item_count"), 1)
            self.assertNotIn("items", input_read_result)
            self.assertIn("Voice Secretary input:", str(input_read_result.get("input_text") or ""))
            self.assertNotIn("combined_text", input_read_result)
            self.assertIn("summarize the billing API migration plan", str(input_read_result.get("input_text") or ""))
            self.assertNotIn("documents_by_path", input_read_result)
            self.assertIn(workspace_path, {str(item.get("document_path") or "") for item in (input_read_result.get("documents") or []) if isinstance(item, dict)})
            second_read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(second_read.ok, getattr(second_read, "error", None))
            self.assertEqual((second_read.result or {}).get("item_count"), 0)

            save_from_input, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "document_path": str(document.get("document_path") or ""),
                    "title": "Billing API migration notes",
                    "content": "# Billing API migration notes\n\n## Summary\n\nKeep rollout risks visible.\n",
                },
            )
            self.assertTrue(save_from_input.ok, getattr(save_from_input, "error", None))
            content = document_path.read_text(encoding="utf-8")
            self.assertIn("Keep rollout risks visible", content)
            self.assertTrue(revisions_path.exists(), revisions_path)
            document = (save_from_input.result or {}).get("document") if isinstance(save_from_input.result, dict) else document
            self.assertEqual(str(document.get("title") or ""), original_document_title)

            followup, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "doc-session-2",
                    "segment_id": "seg-doc-2",
                    "document_path": str(document.get("document_path") or ""),
                    "text": "then connect the cutover checklist to support escalation and rollback ownership",
                    "language": "en-US",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "doc-session-2",
                        "language": "en-US",
                    },
                },
            )
            self.assertTrue(followup.ok, getattr(followup, "error", None))
            followup_read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(followup_read.ok, getattr(followup_read, "error", None))
            followup_result = followup_read.result or {}
            followup_input_text = str(followup_result.get("input_text") or "")
            self.assertEqual(followup_result.get("item_count"), 1)
            self.assertIn("Previous input tail", followup_input_text)
            self.assertIn("billing API migration plan", followup_input_text)
            self.assertIn("cutover checklist", followup_input_text)
            followup_batches = followup_result.get("input_batches") if isinstance(followup_result, dict) else []
            self.assertTrue(followup_batches)
            self.assertIn("billing API migration plan", str((followup_batches[0] or {}).get("previous_input_tail") or ""))

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            documents_by_id = (state.result or {}).get("documents_by_id") if isinstance(state.result, dict) else {}
            self.assertIn(str(document.get("document_id") or ""), documents_by_id)
            self.assertFalse(bool((state.result or {}).get("new_input_available")))

            archive, _ = self._call(
                "assistant_voice_document_archive",
                {
                    "group_id": group_id,
                    "by": "user",
                    "document_path": str(document.get("document_path") or ""),
                },
            )
            self.assertTrue(archive.ok, getattr(archive, "error", None))
            archived_document = (archive.result or {}).get("document") if isinstance(archive.result, dict) else {}
            archived_workspace_path = str((archived_document or {}).get("workspace_path") or "")
            self.assertTrue(archived_workspace_path.startswith("docs/voice-secretary/archive/"), archived_workspace_path)
            archived_document_path = repo / archived_workspace_path
            self.assertTrue(archived_document_path.exists(), archived_document_path)
            self.assertFalse(document_path.exists(), document_path)
            archived_sources_path = Path(home) / "voice-secretary" / group_id / "documents" / "archive" / str(document.get("document_id")) / "sources.jsonl"
            archived_revisions_path = Path(home) / "voice-secretary" / group_id / "documents" / "archive" / str(document.get("document_id")) / "revisions.jsonl"
            self.assertTrue(archived_sources_path.exists(), archived_sources_path)
            self.assertTrue(archived_revisions_path.exists(), archived_revisions_path)

            stale_save, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "document_path": str(document.get("document_path") or ""),
                    "title": str(document.get("title") or ""),
                    "content": "# stale update should not land\n",
                },
            )
            self.assertFalse(stale_save.ok)
            self.assertEqual(stale_save.error.code, "assistant_voice_document_save_failed")

            append_after_archive, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "doc-session-1",
                    "segment_id": "seg-doc-2",
                    "text": "capture the follow-up owner list in a fresh working document",
                    "language": "en-US",
                    "is_final": True,
                    "flush": False,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "doc-session-1",
                        "language": "en-US",
                    },
                },
            )
            self.assertTrue(append_after_archive.ok, getattr(append_after_archive, "error", None))
            self.assertFalse(bool((append_after_archive.result or {}).get("document_updated")), append_after_archive.result)
            self.assertFalse(bool((append_after_archive.result or {}).get("input_event_created")), append_after_archive.result)
            flush_after_archive, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "doc-session-1",
                    "segment_id": "",
                    "text": "",
                    "language": "en-US",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "doc-session-1",
                        "language": "en-US",
                    },
                },
            )
            self.assertTrue(flush_after_archive.ok, getattr(flush_after_archive, "error", None))
            self.assertTrue(bool((flush_after_archive.result or {}).get("input_event_created")), flush_after_archive.result)
            next_document = (flush_after_archive.result or {}).get("document") if isinstance(flush_after_archive.result, dict) else {}
            next_input = (flush_after_archive.result or {}).get("input_event") if isinstance(flush_after_archive.result, dict) else {}
            self.assertIsInstance(next_document, dict)
            self.assertNotEqual(next_document.get("document_id"), document.get("document_id"))
            self.assertNotIn(
                "capture the follow-up owner list",
                (repo / str(next_document.get("workspace_path") or "")).read_text(encoding="utf-8"),
            )
            self.assertIn("capture the follow-up owner list", str(next_input.get("text") or ""))
            self.assertNotIn("capture the follow-up owner list", archived_document_path.read_text(encoding="utf-8"))

            state_after_archive, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state_after_archive.ok, getattr(state_after_archive, "error", None))
            state_after_archive_result = state_after_archive.result or {}
            self.assertEqual(state_after_archive_result.get("active_document_id"), next_document.get("document_id"))
            documents_after_archive = state_after_archive_result.get("documents_by_id") if isinstance(state_after_archive_result, dict) else {}
            self.assertIn(str(next_document.get("document_id") or ""), documents_after_archive)
            self.assertNotIn(str(document.get("document_id") or ""), documents_after_archive)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            events = list(iter_events(group.ledger_path))
            self.assertTrue(any(event.get("kind") == "assistant.voice.document" for event in events))
            input_notifies = [
                event
                for event in events
                if event.get("kind") == "system.notify"
                and ((event.get("data") or {}).get("context") or {}).get("kind") == "voice_secretary_input"
            ]
            self.assertTrue(input_notifies)
            self.assertEqual((input_notifies[-1].get("data") or {}).get("target_actor_id"), "voice-secretary")
        finally:
            cleanup()

    def test_voice_transcript_short_flush_and_tiny_filler_filter(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            filler, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "short-flush",
                    "segment_id": "seg-filler",
                    "text": "嗯",
                    "language": "zh-CN",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "short-flush",
                        "language": "zh-CN",
                    },
                },
            )
            self.assertTrue(filler.ok, getattr(filler, "error", None))
            self.assertFalse(bool((filler.result or {}).get("input_event_created")), filler.result)

            short, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "short-flush",
                    "segment_id": "seg-short",
                    "text": "查东京天气",
                    "language": "zh-CN",
                    "is_final": True,
                    "flush": False,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "short-flush",
                        "language": "zh-CN",
                    },
                },
            )
            self.assertTrue(short.ok, getattr(short, "error", None))
            self.assertFalse(bool((short.result or {}).get("input_event_created")), short.result)

            quiet_flush, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "short-flush",
                    "segment_id": "",
                    "text": "",
                    "language": "zh-CN",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "short-flush",
                        "language": "zh-CN",
                    },
                },
            )
            self.assertTrue(quiet_flush.ok, getattr(quiet_flush, "error", None))
            result = quiet_flush.result or {}
            self.assertTrue(bool(result.get("input_event_created")), result)
            input_event = result.get("input_event") if isinstance(result.get("input_event"), dict) else {}
            self.assertEqual(input_event.get("kind"), "asr_transcript")
            self.assertIn("查东京天气", str(input_event.get("text") or ""))
            self.assertNotIn("嗯", str(input_event.get("text") or ""))
        finally:
            cleanup()

    def test_voice_transcript_atomic_text_flush_survives_empty_flush_race(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            early_flush, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "atomic-race",
                    "segment_id": "",
                    "text": "",
                    "language": "zh-CN",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "atomic-race",
                        "language": "zh-CN",
                    },
                },
            )
            self.assertTrue(early_flush.ok, getattr(early_flush, "error", None))
            self.assertFalse(bool((early_flush.result or {}).get("input_event_created")), early_flush.result)

            atomic_flush, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "atomic-race",
                    "segment_id": "seg-atomic",
                    "text": "查东京天气",
                    "language": "zh-CN",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "atomic-race",
                        "language": "zh-CN",
                    },
                },
            )
            self.assertTrue(atomic_flush.ok, getattr(atomic_flush, "error", None))
            result = atomic_flush.result or {}
            self.assertTrue(bool(result.get("input_event_created")), result)
            input_event = result.get("input_event") if isinstance(result.get("input_event"), dict) else {}
            self.assertIn("查东京天气", str(input_event.get("text") or ""))
        finally:
            cleanup()

    def test_voice_stale_window_flushes_on_existing_daemon_entrypoint(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.assistants import assistant_ops
            from cccc.kernel.group import load_group

            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            append, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "stale-window",
                    "segment_id": "seg-stale",
                    "text": "这段短语音应该在兜底入口被发送给小秘书",
                    "language": "zh-CN",
                    "is_final": True,
                    "flush": False,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "stale-window",
                        "language": "zh-CN",
                    },
                },
            )
            self.assertTrue(append.ok, getattr(append, "error", None))
            self.assertFalse(bool((append.result or {}).get("input_event_created")), append.result)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            runtime_state = assistant_ops._load_runtime_state(group)
            session = runtime_state["voice_sessions"]["stale-window"]
            session["updated_at"] = "2000-01-01T00:00:00+00:00"
            session["window_started_at"] = "2000-01-01T00:00:00+00:00"
            assistant_ops._save_runtime_state(group, runtime_state)

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            self.assertFalse(bool((state.result or {}).get("new_input_available")), state.result)
            input_timing = (state.result or {}).get("input_timing") if isinstance(state.result, dict) else {}
            self.assertEqual(input_timing.get("secretary_delivery_cursor"), 0)

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            input_text = str((read.result or {}).get("input_text") or "")
            self.assertIn("兜底入口", input_text)
            group_after_read = load_group(group_id)
            self.assertIsNotNone(group_after_read)
            assert group_after_read is not None
            input_timing_after_read = assistant_ops._load_voice_input_state(group_after_read)
            self.assertEqual(input_timing_after_read.get("secretary_delivery_cursor"), 1)
        finally:
            cleanup()

    def test_voice_input_notify_is_lightweight_and_read_returns_language_intent_and_policy(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import iter_events

            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            append, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "intent-session-1",
                    "segment_id": "seg-intent-1",
                    "text": "Tell foreman to review the weather plan and create a fresh working document for notes.",
                    "language": "en-US",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "intent-session-1",
                        "language": "en-US",
                    },
                },
            )

            self.assertTrue(append.ok, getattr(append, "error", None))
            result = append.result or {}
            segment = result.get("segment") if isinstance(result.get("segment"), dict) else {}
            self.assertEqual(segment.get("intent_hint"), "mixed")
            self.assertEqual(segment.get("language"), "en-US")

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            events = list(iter_events(group.ledger_path))
            input_notifies = [
                event
                for event in events
                if event.get("kind") == "system.notify"
                and ((event.get("data") or {}).get("context") or {}).get("kind") == "voice_secretary_input"
            ]
            self.assertTrue(input_notifies)
            notify_data = input_notifies[-1].get("data") or {}
            notify_text = f"{notify_data.get('title') or ''}\n{notify_data.get('message') or ''}"
            self.assertIn("input_envelope", notify_text)
            self.assertNotIn("Tell foreman", notify_text)
            context = (notify_data.get("context") or {})
            self.assertEqual(context.get("reason"), "new_input")
            envelope = context.get("input_envelope") if isinstance(context.get("input_envelope"), dict) else {}
            self.assertEqual(envelope.get("delivery_mode"), "daemon_input_envelope")
            self.assertEqual(envelope.get("seq_start"), 1)
            self.assertEqual(envelope.get("seq_end"), 1)
            self.assertIn("Tell foreman", str(envelope.get("input_text") or ""))

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            read_result = read.result or {}
            batches = read_result.get("input_batches") if isinstance(read_result, dict) else []
            self.assertEqual(len(batches), 1)
            input_timing = read_result.get("input_timing") if isinstance(read_result, dict) else {}
            self.assertEqual(input_timing.get("latest_seq"), 1)
            self.assertEqual(input_timing.get("secretary_read_cursor"), 1)
            self.assertEqual(input_timing.get("secretary_delivery_cursor"), 1)
            self.assertTrue(str(input_timing.get("last_input_appended_at") or ""))
            self.assertTrue(str(input_timing.get("last_notify_emitted_at") or ""))
            self.assertTrue(str(input_timing.get("last_input_envelope_at") or ""))
            self.assertTrue(str(input_timing.get("last_read_new_input_at") or ""))
            group_item = batches[0] if isinstance(batches[0], dict) else {}
            self.assertIn("en-US", group_item.get("languages") or [])
            self.assertIn("mixed", group_item.get("intent_hints") or [])
            self.assertNotIn("items", read_result)
            self.assertNotIn("instruction_policy", read_result)
            self.assertNotIn("client_session_id", read_result)
        finally:
            cleanup()

    def test_voice_intent_distinguishes_secretary_scope_from_peer_handoff(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            secretary, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "intent-secretary-session",
                    "segment_id": "seg-secretary-task",
                    "text": "Please investigate why the notes are confusing and summarize better options for the user.",
                    "language": "en-US",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "intent-secretary-session",
                        "language": "en-US",
                    },
                },
            )
            self.assertTrue(secretary.ok, getattr(secretary, "error", None))
            secretary_segment = (secretary.result or {}).get("segment") if isinstance(secretary.result, dict) else {}
            self.assertEqual(secretary_segment.get("intent_hint"), "secretary_task")

            peer, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "intent-peer-session",
                    "segment_id": "seg-peer-task",
                    "text": "Please fix the code and run tests for the runtime bug.",
                    "language": "en-US",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "intent-peer-session",
                        "language": "en-US",
                    },
                },
            )
            self.assertTrue(peer.ok, getattr(peer, "error", None))
            peer_segment = (peer.result or {}).get("segment") if isinstance(peer.result, dict) else {}
            self.assertEqual(peer_segment.get("intent_hint"), "peer_task")
        finally:
            cleanup()

    def test_voice_fragmented_speech_segments_merge_into_input_stream_batch(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import iter_events

            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            fragments = [
                ("seg-282", "日本会議と保守政治の関係について、話者は選択的夫婦別姓制度をめぐる議論を紹介しています。"),
                ("seg-283", "続いてLGBTの権利や政策パッケージが取り上げられ、家族制度との関係が論点になっています。"),
                ("seg-284", "発言全体は政治勢力の影響や社会政策の対立軸を扱う評論として聞こえます。"),
            ]
            last_result = {}
            for segment_id, text in fragments:
                append, _ = self._call(
                    "assistant_voice_transcript_append",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "session_id": "jp-speech-session",
                        "segment_id": segment_id,
                        "text": text,
                        "language": "ja-JP",
                        "is_final": True,
                        "flush": False,
                        "trigger": {
                            "mode": "meeting",
                            "trigger_kind": "meeting_window",
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "client_session_id": "jp-speech-session",
                            "language": "ja-JP",
                        },
                    },
                )
                self.assertTrue(append.ok, getattr(append, "error", None))
                last_result = append.result or {}
                self.assertFalse(bool(last_result.get("input_event_created")), last_result)

            flush, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "jp-speech-session",
                    "segment_id": "",
                    "text": "",
                    "language": "ja-JP",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "jp-speech-session",
                        "language": "ja-JP",
                    },
                },
            )
            self.assertTrue(flush.ok, getattr(flush, "error", None))
            last_result = flush.result or {}

            self.assertTrue(bool(last_result.get("input_event_created")), last_result)
            input_event = last_result.get("input_event") if isinstance(last_result.get("input_event"), dict) else {}
            self.assertEqual(input_event.get("kind"), "asr_transcript")
            self.assertEqual(input_event.get("language"), "ja-JP")
            self.assertNotIn("segment_id", input_event)
            self.assertNotIn("session_id", input_event)
            self.assertNotIn("trigger", input_event)
            metadata = input_event.get("metadata") if isinstance(input_event.get("metadata"), dict) else {}
            self.assertNotIn("source_segment_count", metadata)
            self.assertNotIn("source_segment_range", metadata)
            self.assertEqual(metadata.get("capture_continuity"), "continuous")
            self.assertEqual(metadata.get("suggested_document_mode"), "speech_summary")
            document = last_result.get("document") if isinstance(last_result.get("document"), dict) else {}
            self.assertNotIn("last_source_segment_id", document)
            self.assertNotIn("source_segment_count", document)

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            source_text = str((read.result or {}).get("input_text") or "")
            self.assertIn("日本会議", source_text)
            self.assertIn("LGBT", source_text)
            self.assertIn("政治勢力", source_text)
            batches = (read.result or {}).get("input_batches") if isinstance(read.result, dict) else []
            self.assertEqual(len(batches), 1)
            self.assertEqual((batches[0] or {}).get("item_count"), 1)
            self.assertNotIn("items", batches[0] or {})
            self.assertNotIn("segment_id", batches[0] or {})
            self.assertNotIn("source_segment_range", str(batches[0] or {}))
            input_text = str((read.result or {}).get("input_text") or "")
            self.assertIn("Voice Secretary input:", input_text)
            self.assertNotIn("--- Item", input_text)
            self.assertNotIn("source_segment_range", input_text)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            events = list(iter_events(group.ledger_path))
            input_notifies = [
                event
                for event in events
                if event.get("kind") == "system.notify"
                and ((event.get("data") or {}).get("context") or {}).get("kind") == "voice_secretary_input"
            ]
            self.assertTrue(input_notifies)
            context = ((input_notifies[-1].get("data") or {}).get("context") or {})
            self.assertEqual(context.get("kind"), "voice_secretary_input")
            self.assertNotIn("source_segment_range", context)
        finally:
            cleanup()

    def test_voice_secretary_prompt_refine_round_trip_uses_composer_draft_channel(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            enqueue, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "prompt_refine",
                    "request_id": "voice-prompt-test",
                    "composer_text": "请帮我检查这个方案",
                    "voice_transcript": "重点看看风险和副作用，语气专业一点",
                    "composer_snapshot_hash": "abc123",
                    "composer_context": {
                        "recipients": ["@foreman"],
                        "message_mode": "normal",
                        "recent_chat_excerpt": "user: 之前这个方案主要担心回滚成本。\nforeman: 需要补验收标准。",
                    },
                    "language": "zh-CN",
                },
            )
            self.assertTrue(enqueue.ok, getattr(enqueue, "error", None))
            result = enqueue.result or {}
            self.assertEqual(result.get("request_id"), "voice-prompt-test")
            input_event = result.get("input_event") if isinstance(result.get("input_event"), dict) else {}
            self.assertEqual(input_event.get("kind"), "prompt_refine")
            self.assertEqual(((input_event.get("metadata") or {}).get("target_kind")), "composer")
            self.assertTrue(bool(result.get("input_notify_emitted")), result)

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            input_text = str((read.result or {}).get("input_text") or "")
            self.assertIn("Target: composer", input_text)
            self.assertIn("Request id: voice-prompt-test", input_text)
            self.assertIn("请帮我检查这个方案", input_text)
            self.assertIn("重点看看风险和副作用", input_text)
            self.assertIn("Recent context", input_text)
            self.assertIn("需要补验收标准", input_text)
            self.assertIn("Use MCP tool cccc_voice_secretary_composer", input_text)
            self.assertIn("draft_text must contain only the text to add", input_text)
            self.assertNotIn("Return only the composer text to insert", input_text)
            self.assertNotIn("Execution rules:", input_text)
            self.assertNotIn("Do not write meta lead-ins", input_text)

            submit, _ = self._call(
                "assistant_voice_prompt_draft_submit",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "request_id": "voice-prompt-test",
                    "draft_text": "请基于第一性原理检查这套方案，重点评估风险、副作用和验证路径。",
                    "summary": "Clarified the review ask.",
                },
            )
            self.assertTrue(submit.ok, getattr(submit, "error", None))
            draft = (submit.result or {}).get("prompt_draft") if isinstance(submit.result, dict) else {}
            self.assertEqual(draft.get("status"), "pending")
            self.assertEqual(draft.get("composer_snapshot_hash"), "abc123")

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            pending = (state.result or {}).get("prompt_draft") if isinstance(state.result, dict) else {}
            self.assertEqual(pending.get("request_id"), "voice-prompt-test")
            self.assertIn("第一性原理", str(pending.get("draft_text") or ""))

            ack, _ = self._call(
                "assistant_voice_prompt_draft_ack",
                {
                    "group_id": group_id,
                    "by": "user",
                    "request_id": "voice-prompt-test",
                    "status": "applied",
                },
            )
            self.assertTrue(ack.ok, getattr(ack, "error", None))
            state_after, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state_after.ok, getattr(state_after, "error", None))
            self.assertFalse(bool((state_after.result or {}).get("prompt_draft")))
        finally:
            cleanup()

    def test_voice_secretary_output_completion_auto_fills_missing_composer_draft(self) -> None:
        from cccc.daemon.assistants.voice_secretary_output_completion import complete_missing_composer_drafts

        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            enqueue, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "prompt_refine",
                    "request_id": "voice-prompt-auto",
                    "composer_text": "这个如何优化？",
                    "voice_transcript": "",
                    "operation": "replace_with_refined_prompt",
                    "composer_snapshot_hash": "hash-auto",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(enqueue.ok, getattr(enqueue, "error", None))
            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))

            repaired = complete_missing_composer_drafts(
                group_id=group_id,
                diagnostics={"missing": ["composer_draft:voice-prompt-auto"]},
            )

            self.assertEqual(repaired.get("completed_request_ids"), ["voice-prompt-auto"])
            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            pending = (state.result or {}).get("prompt_draft") if isinstance(state.result, dict) else {}
            self.assertEqual(pending.get("request_id"), "voice-prompt-auto")
            self.assertEqual(pending.get("draft_text"), "这个如何优化？")
            self.assertEqual(pending.get("operation"), "replace_with_refined_prompt")
            self.assertEqual(pending.get("composer_snapshot_hash"), "hash-auto")
        finally:
            cleanup()

    def test_voice_secretary_output_completion_refreshes_existing_composer_draft_timestamp(self) -> None:
        from cccc.daemon.assistants.voice_secretary_output_completion import complete_missing_composer_drafts

        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            enqueue, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "prompt_refine",
                    "request_id": "voice-prompt-stale",
                    "composer_text": "这个如何优化？",
                    "voice_transcript": "",
                    "operation": "replace_with_refined_prompt",
                    "composer_snapshot_hash": "hash-stale",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(enqueue.ok, getattr(enqueue, "error", None))

            submit, _ = self._call(
                "assistant_voice_prompt_draft_submit",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "request_id": "voice-prompt-stale",
                    "composer_snapshot_hash": "hash-stale",
                    "draft_text": "这个如何优化？",
                },
            )
            self.assertTrue(submit.ok, getattr(submit, "error", None))
            before = (submit.result or {}).get("prompt_draft") if isinstance(submit.result, dict) else {}
            before_updated_at = str(before.get("updated_at") or "")
            self.assertTrue(before_updated_at)

            repaired = complete_missing_composer_drafts(
                group_id=group_id,
                diagnostics={"missing": ["composer_draft_updated_at:voice-prompt-stale"]},
            )

            self.assertEqual(repaired.get("completed_request_ids"), ["voice-prompt-stale"])
            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            pending = (state.result or {}).get("prompt_draft") if isinstance(state.result, dict) else {}
            self.assertEqual(pending.get("request_id"), "voice-prompt-stale")
            self.assertEqual(pending.get("draft_text"), "这个如何优化？")
            self.assertNotEqual(str(pending.get("updated_at") or ""), before_updated_at)
        finally:
            cleanup()

    def test_voice_secretary_prompt_refine_replacement_requests_complete_draft(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            enqueue, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "prompt_refine",
                    "request_id": "voice-prompt-replace",
                    "composer_text": "请帮我检查这个方案",
                    "voice_transcript": "重点看看风险和副作用，补上验收标准。",
                    "operation": "replace_with_refined_prompt",
                    "composer_snapshot_hash": "replace-hash",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(enqueue.ok, getattr(enqueue, "error", None))

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            input_text = str((read.result or {}).get("input_text") or "")
            self.assertIn("Operation: replace_with_refined_prompt", input_text)
            self.assertIn("Use MCP tool cccc_voice_secretary_composer", input_text)
            self.assertIn("draft_text must contain the complete replacement prompt", input_text)
            self.assertNotIn("Do not return only an incremental addition", input_text)
            self.assertNotIn("Append mode", input_text)

            submit, _ = self._call(
                "assistant_voice_prompt_draft_submit",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "request_id": "voice-prompt-replace",
                    "composer_snapshot_hash": "replace-hash",
                    "draft_text": "请检查这个方案，重点评估风险、副作用，并补上验收标准。",
                },
            )
            self.assertTrue(submit.ok, getattr(submit, "error", None))
            draft = (submit.result or {}).get("prompt_draft") if isinstance(submit.result, dict) else {}
            self.assertEqual(draft.get("operation"), "replace_with_refined_prompt")
        finally:
            cleanup()

    def test_voice_secretary_prompt_refine_can_optimize_composer_without_speech(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            enqueue, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "prompt_refine",
                    "request_id": "voice-prompt-composer-only",
                    "composer_text": "帮我让大家汇报进展",
                    "operation": "replace_with_refined_prompt",
                    "composer_snapshot_hash": "composer-only-hash",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(enqueue.ok, getattr(enqueue, "error", None))

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            input_text = str((read.result or {}).get("input_text") or "")
            self.assertIn("Operation: replace_with_refined_prompt", input_text)
            self.assertIn("帮我让大家汇报进展", input_text)
            self.assertIn("optimize current composer draft directly", input_text)
            self.assertIn("draft_text must contain the complete replacement prompt", input_text)
        finally:
            cleanup()

    def test_voice_secretary_prompt_refine_followup_reuses_request_and_stales_old_draft(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            first, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "prompt_refine",
                    "request_id": "voice-prompt-merge",
                    "composer_text": "请帮我检查这个方案",
                    "voice_transcript": "先强调风险和副作用。",
                    "composer_snapshot_hash": "hash-1",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))
            read_first, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read_first.ok, getattr(read_first, "error", None))
            first_text = str((read_first.result or {}).get("input_text") or "")
            self.assertIn("先强调风险", first_text)

            submit_first, _ = self._call(
                "assistant_voice_prompt_draft_submit",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "request_id": "voice-prompt-merge",
                    "composer_snapshot_hash": "hash-1",
                    "draft_text": "请检查方案风险与副作用。",
                },
            )
            self.assertTrue(submit_first.ok, getattr(submit_first, "error", None))

            state_with_first_draft, _ = self._call(
                "assistant_state",
                {
                    "group_id": group_id,
                    "assistant_id": "voice_secretary",
                    "prompt_request_id": "voice-prompt-merge",
                },
            )
            self.assertTrue(state_with_first_draft.ok, getattr(state_with_first_draft, "error", None))
            self.assertEqual(((state_with_first_draft.result or {}).get("prompt_draft") or {}).get("request_id"), "voice-prompt-merge")

            followup, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "prompt_refine",
                    "request_id": "voice-prompt-merge",
                    "composer_text": "请帮我检查这个方案",
                    "voice_transcript": "再补充验证步骤和回滚路径。",
                    "composer_snapshot_hash": "hash-1",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(followup.ok, getattr(followup, "error", None))

            state_after_followup, _ = self._call(
                "assistant_state",
                {
                    "group_id": group_id,
                    "assistant_id": "voice_secretary",
                    "prompt_request_id": "voice-prompt-merge",
                },
            )
            self.assertTrue(state_after_followup.ok, getattr(state_after_followup, "error", None))
            self.assertFalse(bool((state_after_followup.result or {}).get("prompt_draft")))

            read_followup, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read_followup.ok, getattr(read_followup, "error", None))
            merged_text = str((read_followup.result or {}).get("input_text") or "")
            self.assertIn("Request id: voice-prompt-merge", merged_text)
            self.assertIn("先强调风险", merged_text)
            self.assertIn("再补充验证步骤", merged_text)
        finally:
            cleanup()

    def test_voice_document_input_read_stales_expired_pending_prompt_draft(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            state_dir = Path(home) / "groups" / group_id / "state"
            state_dir.mkdir(parents=True, exist_ok=True)
            assistants_path = state_dir / "assistants.json"
            assistants_path.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": group_id,
                        "assistants": {},
                        "voice_sessions": {},
                        "voice_ask_requests": {},
                        "voice_prompt_requests": {},
                        "voice_prompt_drafts": {
                            "voice-prompt-stale": {
                                "request_id": "voice-prompt-stale",
                                "created_at": "2026-04-20T09:00:00Z",
                                "updated_at": "2026-04-20T09:00:00Z",
                                "draft_text": "old pending draft",
                                "status": "pending",
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )
            voice_state_dir = Path(home) / "voice-secretary" / group_id
            voice_state_dir.mkdir(parents=True, exist_ok=True)
            (voice_state_dir / "input_state.json").write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": group_id,
                        "latest_seq": 0,
                        "secretary_read_cursor": 0,
                    }
                ),
                encoding="utf-8",
            )

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))

            saved_state = json.loads(assistants_path.read_text(encoding="utf-8"))
            draft = ((saved_state.get("voice_prompt_drafts") or {}).get("voice-prompt-stale") or {})
            self.assertEqual(draft.get("status"), "stale")
            self.assertNotEqual(draft.get("updated_at"), "2026-04-20T09:00:00Z")
        finally:
            cleanup()

    def test_voice_secretary_input_append_auto_wakes_actor_when_group_not_running(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_input_append
            from cccc.kernel.group import load_group

            started: list[dict] = []

            def fake_start_actor_process(group, actor_id, **kwargs):
                started.append({"group_id": group.group_id, "actor_id": actor_id, **kwargs})
                return {"success": True}

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            with patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False):
                resp = handle_assistant_voice_input_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "prompt_refine",
                        "request_id": "voice-prompt-wake",
                        "composer_text": "请帮我润色这个提示词",
                        "voice_transcript": "语气更直接，结论先行。",
                        "language": "zh-CN",
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=fake_start_actor_process,
                )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            self.assertEqual(len(started), 1)
            self.assertEqual(started[0]["group_id"], group_id)
            self.assertEqual(started[0]["actor_id"], "voice-secretary")
            self.assertTrue(bool((resp.result or {}).get("actor_woken")), resp.result)
            self.assertEqual((resp.result or {}).get("actor_wake_error"), "")
        finally:
            cleanup()

    def test_voice_secretary_input_append_delivers_headless_notify_after_auto_wake(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_input_append
            from cccc.kernel.group import load_group

            running = {"value": False}
            submitted: list[dict] = []

            def fake_start_actor_process(group, actor_id, **kwargs):
                running["value"] = True
                return {"success": True}

            def fake_actor_running(group_id_arg, actor_id_arg):
                self.assertEqual(group_id_arg, group_id)
                self.assertEqual(actor_id_arg, "voice-secretary")
                return bool(running["value"])

            def fake_submit_control_message(**kwargs):
                submitted.append(dict(kwargs))
                return True

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            with (
                patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False),
                patch("cccc.daemon.codex_app_sessions.SUPERVISOR.actor_running", side_effect=fake_actor_running),
                patch(
                    "cccc.daemon.codex_app_sessions.SUPERVISOR.submit_control_message",
                    side_effect=fake_submit_control_message,
                ),
            ):
                resp = handle_assistant_voice_input_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "prompt_refine",
                        "request_id": "voice-prompt-wake-deliver",
                        "composer_text": "Keep the prompt concise.",
                        "voice_transcript": "post-wake delivery must include the request payload",
                        "language": "en-US",
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=fake_start_actor_process,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result or {}
            self.assertTrue(bool(result.get("actor_woken")), result)
            self.assertTrue(bool(result.get("actor_notify_delivered")), result)
            self.assertEqual(result.get("actor_notify_delivery_error"), "")
            self.assertEqual(len(submitted), 1)
            submitted_kwargs = submitted[0]
            self.assertEqual(submitted_kwargs.get("control_kind"), "system_notify")
            self.assertEqual(submitted_kwargs.get("actor_id"), "voice-secretary")
            control_text = str(submitted_kwargs.get("text") or "")
            self.assertIn("Voice Secretary input:", control_text)
            self.assertNotIn("Input envelope:", control_text)
            self.assertNotIn("seq_range=", control_text)
            self.assertIn("voice-prompt-wake-deliver", control_text)
            self.assertIn("post-wake delivery must include the request payload", control_text)
        finally:
            cleanup()

    def test_voice_secretary_input_append_emits_current_notify_when_actor_disabled(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_input_append
            from cccc.kernel.actors import update_actor
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            update_actor(group, "voice-secretary", {"enabled": False})
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            running = {"value": False}
            submitted: list[dict] = []

            def fake_start_actor_process(group, actor_id, **kwargs):
                running["value"] = True
                return {"success": True}

            def fake_actor_running(group_id_arg, actor_id_arg):
                self.assertEqual(group_id_arg, group_id)
                self.assertEqual(actor_id_arg, "voice-secretary")
                return bool(running["value"])

            def fake_submit_control_message(**kwargs):
                submitted.append(dict(kwargs))
                return True

            with (
                patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False),
                patch("cccc.daemon.codex_app_sessions.SUPERVISOR.actor_running", side_effect=fake_actor_running),
                patch(
                    "cccc.daemon.codex_app_sessions.SUPERVISOR.submit_control_message",
                    side_effect=fake_submit_control_message,
                ),
            ):
                resp = handle_assistant_voice_input_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "prompt_refine",
                        "request_id": "voice-prompt-current-after-stop",
                        "composer_text": "Polish this prompt.",
                        "voice_transcript": "Use the current request id, not a stale one.",
                        "language": "en-US",
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=fake_start_actor_process,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result or {}
            self.assertTrue(bool(result.get("input_notify_emitted")), result)
            self.assertTrue(bool(result.get("actor_woken")), result)
            self.assertTrue(bool(result.get("actor_notify_delivered")), result)
            self.assertEqual(len(submitted), 1)
            control_text = str(submitted[0].get("text") or "")
            self.assertIn("voice-prompt-current-after-stop", control_text)
            self.assertIn("Use the current request id, not a stale one.", control_text)
        finally:
            cleanup()

    def test_voice_secretary_input_append_does_not_deliver_stale_notify_after_wake(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_input_append
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            with patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=True):
                old_resp = handle_assistant_voice_input_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "prompt_refine",
                        "request_id": "voice-prompt-old-notify",
                        "composer_text": "Old prompt.",
                        "voice_transcript": "Old request.",
                        "language": "en-US",
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=lambda *args, **kwargs: {"success": True},
                )
            self.assertTrue(old_resp.ok, getattr(old_resp, "error", None))
            self.assertTrue(bool((old_resp.result or {}).get("input_notify_emitted")), old_resp.result)

            running = {"value": False}

            def fake_start_actor_process(group, actor_id, **kwargs):
                running["value"] = True
                return {"success": True}

            def fake_actor_running(group_id_arg, actor_id_arg):
                return bool(running["value"])

            with (
                patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False),
                patch(
                    "cccc.daemon.assistants.assistant_ops._try_emit_voice_input_notify_after_input",
                    return_value=(False, "emit_failed", {}),
                ),
                patch("cccc.daemon.codex_app_sessions.SUPERVISOR.actor_running", side_effect=fake_actor_running),
                patch("cccc.daemon.codex_app_sessions.SUPERVISOR.submit_control_message") as submit_control,
            ):
                resp = handle_assistant_voice_input_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "prompt_refine",
                        "request_id": "voice-prompt-current-without-notify",
                        "composer_text": "Current prompt.",
                        "voice_transcript": "A missing current notify must not dispatch the old request.",
                        "language": "en-US",
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=fake_start_actor_process,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result or {}
            self.assertTrue(bool(result.get("actor_woken")), result)
            self.assertFalse(bool(result.get("actor_notify_delivered")), result)
            self.assertEqual(result.get("actor_notify_delivery_error"), "missing_voice_input_notify_event")
            submit_control.assert_not_called()
        finally:
            cleanup()

    def test_voice_secretary_input_append_queues_pty_notify_after_auto_wake(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_input_append
            from cccc.kernel.actors import update_actor
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            update_actor(group, "voice-secretary", {"runner": "pty", "runtime": "codex", "enabled": True})
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            running = {"value": False}

            def fake_start_actor_process(group, actor_id, **kwargs):
                running["value"] = True
                return {"success": True}

            def fake_actor_running(group_id_arg, actor_id_arg):
                self.assertEqual(group_id_arg, group_id)
                self.assertEqual(actor_id_arg, "voice-secretary")
                return bool(running["value"])

            with (
                patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False),
                patch("cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running", side_effect=fake_actor_running),
                patch("cccc.daemon.messaging.delivery.queue_system_notify") as queue_system_notify,
                patch("cccc.daemon.messaging.delivery.request_flush_pending_messages", return_value=True) as request_flush,
            ):
                resp = handle_assistant_voice_input_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "voice_instruction",
                        "request_id": "voice-ask-wake-pty",
                        "instruction": "Check whether the stopped PTY runtime receives this request.",
                        "language": "en-US",
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=fake_start_actor_process,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result or {}
            self.assertTrue(bool(result.get("actor_woken")), result)
            self.assertTrue(bool(result.get("actor_notify_delivered")), result)
            self.assertEqual(result.get("actor_notify_delivery_error"), "")
            queue_system_notify.assert_called_once()
            queue_kwargs = queue_system_notify.call_args.kwargs
            self.assertEqual(queue_kwargs.get("actor_id"), "voice-secretary")
            self.assertEqual(queue_kwargs.get("notify_kind"), "info")
            self.assertIn("voice-ask-wake-pty", str(queue_kwargs.get("message") or ""))
            self.assertIn("stopped PTY runtime receives this request", str(queue_kwargs.get("message") or ""))
            request_flush.assert_called_once()
            self.assertEqual(request_flush.call_args.kwargs.get("actor_id"), "voice-secretary")
        finally:
            cleanup()

    def test_voice_secretary_input_append_persists_input_when_actor_wake_fails(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_input_append
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            with patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False):
                resp = handle_assistant_voice_input_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "prompt_refine",
                        "request_id": "voice-prompt-wake-fail",
                        "composer_text": "Polish this prompt.",
                        "voice_transcript": "Make the acceptance criteria explicit.",
                        "language": "en-US",
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=lambda *_args, **_kwargs: {"success": False, "error": "spawn failed"},
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            self.assertFalse(bool((resp.result or {}).get("actor_woken")), resp.result)
            self.assertIn("spawn failed", str((resp.result or {}).get("actor_wake_error") or ""))
            self.assertTrue(bool((resp.result or {}).get("input_event_created")), resp.result)
            self.assertTrue(bool((resp.result or {}).get("input_notify_emitted")), resp.result)

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            self.assertEqual((read.result or {}).get("item_count"), 1, read.result)
            self.assertIn("Make the acceptance criteria explicit", str((read.result or {}).get("input_text") or ""))
        finally:
            cleanup()

    def test_voice_secretary_transcript_append_auto_wakes_actor_when_input_created(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_transcript_append
            from cccc.kernel.group import load_group

            started: list[dict] = []

            def fake_start_actor_process(group, actor_id, **kwargs):
                started.append({"group_id": group.group_id, "actor_id": actor_id, **kwargs})
                return {"success": True}

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            append = handle_assistant_voice_transcript_append(
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "doc-wake-session",
                    "segment_id": "seg-doc-wake-1",
                    "text": "summarize the launch checklist and keep unresolved delivery risks visible",
                    "language": "en-US",
                    "is_final": True,
                    "flush": False,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "doc-wake-session",
                        "language": "en-US",
                    },
                }
            )
            self.assertTrue(append.ok, getattr(append, "error", None))
            self.assertFalse(bool((append.result or {}).get("input_event_created")), append.result)

            with patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False):
                flush = handle_assistant_voice_transcript_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "session_id": "doc-wake-session",
                        "text": "",
                        "language": "en-US",
                        "is_final": True,
                        "flush": True,
                        "trigger": {
                            "mode": "meeting",
                            "trigger_kind": "meeting_window",
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "client_session_id": "doc-wake-session",
                            "language": "en-US",
                        },
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=fake_start_actor_process,
                )

            self.assertTrue(flush.ok, getattr(flush, "error", None))
            self.assertTrue(bool((flush.result or {}).get("input_event_created")), flush.result)
            self.assertEqual(len(started), 1)
            self.assertEqual(started[0]["group_id"], group_id)
            self.assertEqual(started[0]["actor_id"], "voice-secretary")
            self.assertTrue(bool((flush.result or {}).get("actor_woken")), flush.result)
            self.assertEqual((flush.result or {}).get("actor_wake_error"), "")
        finally:
            cleanup()

    def test_voice_secretary_transcript_append_delivers_latest_notify_after_auto_wake(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_transcript_append
            from cccc.kernel.group import load_group

            dispatched: list[dict] = []

            def fake_start_actor_process(group, actor_id, **kwargs):
                return {"success": True}

            def fake_dispatch(group, *, event, actor_id, async_flush=False):
                dispatched.append(
                    {
                        "group_id": group.group_id,
                        "actor_id": actor_id,
                        "async_flush": async_flush,
                        "event": event,
                    }
                )
                return True

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            with (
                patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False),
                patch(
                    "cccc.daemon.assistants.assistant_ops.dispatch_system_notify_event_to_actor",
                    side_effect=fake_dispatch,
                ),
            ):
                resp = handle_assistant_voice_transcript_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "session_id": "doc-wake-dispatch-session",
                        "segment_id": "seg-doc-wake-dispatch-1",
                        "text": "record this stopped ASR wake request into the working notes",
                        "language": "en-US",
                        "is_final": True,
                        "flush": True,
                        "trigger": {
                            "mode": "meeting",
                            "trigger_kind": "meeting_window",
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "client_session_id": "doc-wake-dispatch-session",
                            "language": "en-US",
                        },
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=fake_start_actor_process,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result or {}
            self.assertTrue(bool(result.get("input_event_created")), result)
            self.assertTrue(bool(result.get("actor_woken")), result)
            self.assertTrue(bool(result.get("actor_notify_delivered")), result)
            self.assertEqual(len(dispatched), 1)
            self.assertEqual(dispatched[0].get("actor_id"), "voice-secretary")
            self.assertTrue(bool(dispatched[0].get("async_flush")))
            event = dispatched[0].get("event") if isinstance(dispatched[0].get("event"), dict) else {}
            self.assertEqual(event.get("kind"), "system.notify")
            context = ((event.get("data") or {}).get("context") or {}) if isinstance(event.get("data"), dict) else {}
            self.assertEqual(context.get("kind"), "voice_secretary_input")
            envelope = context.get("input_envelope") if isinstance(context.get("input_envelope"), dict) else {}
            self.assertIn("stopped ASR wake request", str(envelope.get("input_text") or ""))
        finally:
            cleanup()

    def test_voice_secretary_transcript_append_persists_input_when_actor_wake_fails(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_transcript_append
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            with patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False):
                resp = handle_assistant_voice_transcript_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "session_id": "doc-wake-fail-session",
                        "segment_id": "seg-doc-wake-fail-1",
                        "text": "capture the launch decision, owner, and unresolved rollout risks",
                        "language": "en-US",
                        "is_final": True,
                        "flush": True,
                        "trigger": {
                            "mode": "meeting",
                            "trigger_kind": "meeting_window",
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "client_session_id": "doc-wake-fail-session",
                            "language": "en-US",
                        },
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=lambda *_args, **_kwargs: {"success": False, "error": "spawn failed"},
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            self.assertTrue(bool((resp.result or {}).get("input_event_created")), resp.result)
            self.assertFalse(bool((resp.result or {}).get("actor_woken")), resp.result)
            self.assertIn("spawn failed", str((resp.result or {}).get("actor_wake_error") or ""))

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            self.assertEqual((read.result or {}).get("item_count"), 1, read.result)
            self.assertIn("unresolved rollout risks", str((read.result or {}).get("input_text") or ""))
        finally:
            cleanup()

    def test_voice_secretary_document_instruction_auto_wakes_actor_when_input_created(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_document_instruction
            from cccc.kernel.group import load_group

            created, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "title": "Launch Notes",
                    "content": "# Launch Notes\n\n## Working Notes\nExisting summary.\n",
                    "create_new": True,
                },
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            document = (created.result or {}).get("document") if isinstance(created.result, dict) else {}
            document_path = str((document or {}).get("document_path") or "")
            self.assertTrue(document_path)

            started: list[dict] = []

            def fake_start_actor_process(group, actor_id, **kwargs):
                started.append({"group_id": group.group_id, "actor_id": actor_id, **kwargs})
                return {"success": True}

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            with patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False):
                instruction = handle_assistant_voice_document_instruction(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "document_path": document_path,
                        "instruction": "Rewrite this into a concise launch-risk summary.",
                        "trigger": {
                            "trigger_kind": "user_instruction",
                            "mode": "meeting",
                            "recognition_backend": "browser_asr",
                            "language": "en-US",
                        },
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=fake_start_actor_process,
                )

            self.assertTrue(instruction.ok, getattr(instruction, "error", None))
            self.assertTrue(bool((instruction.result or {}).get("input_event_created")), instruction.result)
            self.assertEqual(len(started), 1)
            self.assertEqual(started[0]["group_id"], group_id)
            self.assertEqual(started[0]["actor_id"], "voice-secretary")
            self.assertTrue(bool((instruction.result or {}).get("actor_woken")), instruction.result)
            self.assertEqual((instruction.result or {}).get("actor_wake_error"), "")
        finally:
            cleanup()

    def test_voice_secretary_document_instruction_persists_input_when_actor_wake_fails(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_document_instruction
            from cccc.kernel.group import load_group

            created, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "title": "Launch Notes",
                    "content": "# Launch Notes\n\n## Working Notes\nExisting summary.\n",
                    "create_new": True,
                },
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            document = (created.result or {}).get("document") if isinstance(created.result, dict) else {}
            document_path = str((document or {}).get("document_path") or "")
            self.assertTrue(document_path)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["running"] = False
            group.save()

            with patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False):
                resp = handle_assistant_voice_document_instruction(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "document_path": document_path,
                        "instruction": "Turn this into a concise launch-risk summary.",
                        "trigger": {
                            "trigger_kind": "user_instruction",
                            "mode": "meeting",
                            "recognition_backend": "browser_asr",
                            "language": "en-US",
                        },
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=lambda *_args, **_kwargs: {"success": False, "error": "spawn failed"},
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            self.assertTrue(bool((resp.result or {}).get("input_event_created")), resp.result)
            self.assertFalse(bool((resp.result or {}).get("actor_woken")), resp.result)
            self.assertIn("spawn failed", str((resp.result or {}).get("actor_wake_error") or ""))

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            self.assertEqual((read.result or {}).get("item_count"), 1, read.result)
            self.assertIn("launch-risk summary", str((read.result or {}).get("input_text") or ""))
        finally:
            cleanup()

    def test_voice_secretary_input_append_re_emits_notify_after_wake_with_existing_unread(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_input_append

            first = handle_assistant_voice_input_append(
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "prompt_refine",
                    "request_id": "voice-prompt-old",
                    "composer_text": "",
                    "voice_transcript": "old request",
                    "language": "en",
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))

            started: list[dict] = []
            notify_reasons: list[str] = []

            def fake_start_actor_process(group, actor_id, **kwargs):
                started.append({"group_id": group.group_id, "actor_id": actor_id, **kwargs})
                return {"success": True}

            def fake_emit(group, *, reason):
                notify_reasons.append(str(reason))

            with (
                patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=False),
                patch("cccc.daemon.assistants.assistant_ops._emit_voice_input_notify", side_effect=fake_emit),
            ):
                second = handle_assistant_voice_input_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "prompt_refine",
                        "request_id": "voice-prompt-new",
                        "composer_text": "",
                        "voice_transcript": "new request",
                        "language": "en",
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=fake_start_actor_process,
                )

            self.assertTrue(second.ok, getattr(second, "error", None))
            self.assertEqual(len(started), 1)
            self.assertEqual(notify_reasons, ["new_input"])
        finally:
            cleanup()

    def test_voice_secretary_input_append_emits_notify_when_actor_already_running(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            from cccc.daemon.assistants.assistant_ops import handle_assistant_voice_input_append

            notify_reasons: list[str] = []

            def fake_emit(group, *, reason):
                notify_reasons.append(str(reason))

            with (
                patch("cccc.daemon.assistants.assistant_ops.is_voice_secretary_actor_running", return_value=True),
                patch("cccc.daemon.assistants.assistant_ops._emit_voice_input_notify", side_effect=fake_emit),
            ):
                resp = handle_assistant_voice_input_append(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "prompt_refine",
                        "request_id": "voice-prompt-running",
                        "composer_text": "",
                        "voice_transcript": "running actor request",
                        "language": "en",
                    },
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    start_actor_process=lambda *_args, **_kwargs: {"success": True},
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            self.assertEqual(notify_reasons, ["new_input"])
        finally:
            cleanup()

    def test_assistant_state_can_return_prompt_draft_for_requested_id(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            for request_id, draft in (
                ("voice-prompt-first", "第一条优化结果"),
                ("voice-prompt-second", "第二条优化结果"),
            ):
                enqueue, _ = self._call(
                    "assistant_voice_input_append",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "kind": "prompt_refine",
                        "request_id": request_id,
                        "composer_text": "请帮我润色这个提示词",
                        "voice_transcript": f"{request_id} 的补充说明",
                    },
                )
                self.assertTrue(enqueue.ok, getattr(enqueue, "error", None))
                submit, _ = self._call(
                    "assistant_voice_prompt_draft_submit",
                    {
                        "group_id": group_id,
                        "by": "assistant:voice_secretary",
                        "request_id": request_id,
                        "draft_text": draft,
                    },
                )
                self.assertTrue(submit.ok, getattr(submit, "error", None))

            latest_state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(latest_state.ok, getattr(latest_state, "error", None))
            self.assertEqual(((latest_state.result or {}).get("prompt_draft") or {}).get("request_id"), "voice-prompt-second")

            requested_state, _ = self._call(
                "assistant_state",
                {
                    "group_id": group_id,
                    "assistant_id": "voice_secretary",
                    "prompt_request_id": "voice-prompt-first",
                },
            )
            self.assertTrue(requested_state.ok, getattr(requested_state, "error", None))
            requested = (requested_state.result or {}).get("prompt_draft") or {}
            self.assertEqual(requested.get("request_id"), "voice-prompt-first")
            self.assertIn("第一条", str(requested.get("draft_text") or ""))
        finally:
            cleanup()

    def test_voice_secretary_prompt_draft_submit_rejects_orphan_request_id(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            submit, _ = self._call(
                "assistant_voice_prompt_draft_submit",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "request_id": "voice-prompt-missing",
                    "draft_text": "不会被任何 composer 接收的草稿",
                },
            )

            self.assertFalse(submit.ok)
            self.assertIsNotNone(submit.error)
            self.assertEqual(submit.error.code if submit.error else "", "prompt_request_not_found")
        finally:
            cleanup()

    def test_voice_secretary_general_instruction_does_not_require_document(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            enqueue, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "voice_instruction",
                    "instruction": "帮我检查一下刚才的总结有没有遗漏。",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(enqueue.ok, getattr(enqueue, "error", None))
            input_event = (enqueue.result or {}).get("input_event") if isinstance(enqueue.result, dict) else {}
            self.assertEqual(input_event.get("kind"), "voice_instruction")
            self.assertEqual(str(input_event.get("document_path") or ""), "")
            self.assertEqual(((input_event.get("metadata") or {}).get("target_kind")), "secretary")
            request_id = str((enqueue.result or {}).get("request_id") or "")
            self.assertTrue(request_id.startswith("voice-ask-"))
            self.assertEqual(((input_event.get("metadata") or {}).get("request_id")), request_id)

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            input_text = str((read.result or {}).get("input_text") or "")
            self.assertIn("Mode: ask", input_text)
            self.assertIn("Target: secretary", input_text)
            self.assertIn(f"Request id: {request_id}", input_text)
            self.assertIn("Required output:", input_text)
            self.assertIn("Use MCP tool cccc_voice_secretary_request(action=\"report\"", input_text)
            self.assertIn("Task:", input_text)
            self.assertIn("刚才的总结有没有遗漏", input_text)
            self.assertEqual((read.result or {}).get("documents"), [])
            batches = (read.result or {}).get("input_batches") if isinstance(read.result, dict) else []
            self.assertTrue(batches)
            self.assertEqual(batches[0].get("request_kind"), "ask_request")
            self.assertTrue(batches[0].get("requires_report"))
            self.assertEqual(batches[0].get("report_channel"), 'cccc_voice_secretary_request(action="report")')

            state, _ = self._call(
                "assistant_state",
                {"group_id": group_id, "assistant_id": "voice_secretary"},
            )
            self.assertTrue(state.ok, getattr(state, "error", None))
            ask_requests = (state.result or {}).get("ask_requests") if isinstance(state.result, dict) else []
            self.assertTrue(ask_requests)
            self.assertEqual(ask_requests[0].get("request_id"), request_id)
            self.assertEqual(ask_requests[0].get("status"), "working")
            self.assertTrue(str(ask_requests[0].get("input_appended_at") or ""))
            self.assertTrue(str(ask_requests[0].get("first_read_at") or ""))

            feedback, _ = self._call(
                "assistant_voice_instruction_feedback",
                {
                    "group_id": group_id,
                    "by": "voice-secretary",
                    "request_id": request_id,
                    "status": "done",
                    "reply_text": "Checked the recent summary and found no obvious omission.",
                    "document_path": "docs/voice-secretary/ask-answer.md",
                    "artifact_paths": [
                        "docs/voice-secretary/ask-answer.md",
                        "docs/voice-secretary/evidence.md",
                    ],
                    "source_summary": "Checked the current workspace notes and linked evidence document.",
                    "checked_at": "2026-04-22T12:00:00Z",
                    "source_urls": [
                        "https://example.com/source-a",
                        "https://example.com/source-b",
                    ],
                },
            )
            self.assertTrue(feedback.ok, getattr(feedback, "error", None))
            feedback_request = (feedback.result or {}).get("ask_request") if isinstance(feedback.result, dict) else {}
            self.assertEqual(feedback_request.get("status"), "done")
            self.assertIn("no obvious omission", str(feedback_request.get("reply_text") or ""))
            self.assertEqual(
                feedback_request.get("artifact_paths"),
                ["docs/voice-secretary/ask-answer.md", "docs/voice-secretary/evidence.md"],
            )
            self.assertEqual(feedback_request.get("source_summary"), "Checked the current workspace notes and linked evidence document.")
            self.assertEqual(feedback_request.get("checked_at"), "2026-04-22T12:00:00Z")
            self.assertEqual(
                feedback_request.get("source_urls"),
                ["https://example.com/source-a", "https://example.com/source-b"],
            )
            self.assertNotIn("result_summary", feedback_request)

            correction, _ = self._call(
                "assistant_voice_instruction_feedback",
                {
                    "group_id": group_id,
                    "by": "voice-secretary",
                    "request_id": request_id,
                    "status": "done",
                    "reply_text": "Correction: the linked evidence also notes one follow-up risk.",
                    "source_summary": "Amended after rechecking the evidence document.",
                },
            )
            self.assertTrue(correction.ok, getattr(correction, "error", None))
            correction_request = (correction.result or {}).get("ask_request") if isinstance(correction.result, dict) else {}
            self.assertEqual(correction_request.get("status"), "done")
            self.assertIn("Correction", str(correction_request.get("reply_text") or ""))
            self.assertEqual(correction_request.get("source_summary"), "Amended after rechecking the evidence document.")
            self.assertEqual(
                correction_request.get("artifact_paths"),
                ["docs/voice-secretary/ask-answer.md", "docs/voice-secretary/evidence.md"],
            )

            enqueue_active, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "voice_instruction",
                    "instruction": "继续帮我留意后续问题。",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(enqueue_active.ok, getattr(enqueue_active, "error", None))
            active_request_id = str((enqueue_active.result or {}).get("request_id") or "")
            self.assertTrue(active_request_id.startswith("voice-ask-"))

            cleared, _ = self._call(
                "assistant_voice_ask_requests_clear",
                {"group_id": group_id, "by": "user"},
            )
            self.assertTrue(cleared.ok, getattr(cleared, "error", None))
            remaining_requests = (cleared.result or {}).get("ask_requests") if isinstance(cleared.result, dict) else []
            remaining_ids = {str(item.get("request_id") or "") for item in remaining_requests if isinstance(item, dict)}
            self.assertNotIn(request_id, remaining_ids)
            self.assertNotIn(active_request_id, remaining_ids)

            late_feedback, _ = self._call(
                "assistant_voice_instruction_feedback",
                {
                    "group_id": group_id,
                    "by": "voice-secretary",
                    "request_id": active_request_id,
                    "status": "done",
                    "reply_text": "后续问题已整理。",
                },
            )
            self.assertTrue(late_feedback.ok, getattr(late_feedback, "error", None))

            state_after_feedback, _ = self._call(
                "assistant_state",
                {"group_id": group_id, "assistant_id": "voice_secretary"},
            )
            self.assertTrue(state_after_feedback.ok, getattr(state_after_feedback, "error", None))
            after_requests = (state_after_feedback.result or {}).get("ask_requests") if isinstance(state_after_feedback.result, dict) else []
            after_by_id = {str(item.get("request_id") or ""): item for item in after_requests if isinstance(item, dict)}
            self.assertIn(active_request_id, after_by_id)
            self.assertIn("后续问题", str(after_by_id[active_request_id].get("reply_text") or ""))
            self.assertNotIn("cleared_at", after_by_id[active_request_id])
        finally:
            cleanup()

    def test_voice_secretary_working_reply_is_not_reused_as_final_reply(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            enqueue, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "voice_instruction",
                    "instruction": "帮我查一下今天的日程。",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(enqueue.ok, getattr(enqueue, "error", None))
            request_id = str((enqueue.result or {}).get("request_id") or "")
            self.assertTrue(request_id)

            working, _ = self._call(
                "assistant_voice_instruction_feedback",
                {
                    "group_id": group_id,
                    "by": "voice-secretary",
                    "request_id": request_id,
                    "status": "working",
                    "reply_text": "正在整理日程。",
                },
            )
            self.assertTrue(working.ok, getattr(working, "error", None))
            working_request = (working.result or {}).get("ask_request") if isinstance(working.result, dict) else {}
            self.assertEqual(working_request.get("status"), "working")
            self.assertEqual(working_request.get("reply_text"), "正在整理日程。")

            done_without_reply, _ = self._call(
                "assistant_voice_instruction_feedback",
                {
                    "group_id": group_id,
                    "by": "voice-secretary",
                    "request_id": request_id,
                    "status": "done",
                },
            )
            self.assertTrue(done_without_reply.ok, getattr(done_without_reply, "error", None))
            done_request = (done_without_reply.result or {}).get("ask_request") if isinstance(done_without_reply.result, dict) else {}
            self.assertEqual(done_request.get("status"), "done")
            self.assertNotIn("reply_text", done_request)
        finally:
            cleanup()

    def test_voice_secretary_late_feedback_preserves_newer_active_request_state(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            first, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "voice_instruction",
                    "request_id": "voice-ask-old",
                    "instruction": "先查一下今天的日程。",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))

            second, _ = self._call(
                "assistant_voice_input_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "kind": "voice_instruction",
                    "request_id": "voice-ask-new",
                    "instruction": "再提醒小抠处理图片任务。",
                    "language": "zh-CN",
                },
            )
            self.assertTrue(second.ok, getattr(second, "error", None))

            done_old, _ = self._call(
                "assistant_voice_instruction_feedback",
                {
                    "group_id": group_id,
                    "by": "voice-secretary",
                    "request_id": "voice-ask-old",
                    "status": "done",
                    "reply_text": "今天日程已整理。",
                },
            )
            self.assertTrue(done_old.ok, getattr(done_old, "error", None))
            assistant = (done_old.result or {}).get("assistant") if isinstance(done_old.result, dict) else {}
            health = assistant.get("health") if isinstance(assistant, dict) and isinstance(assistant.get("health"), dict) else {}
            self.assertEqual(assistant.get("lifecycle"), "working")
            self.assertEqual(health.get("last_ask_request_id"), "voice-ask-new")
            self.assertEqual(health.get("active_request_id"), "voice-ask-new")
            self.assertEqual(health.get("active_request_status"), "pending")

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            requests = (state.result or {}).get("ask_requests") if isinstance(state.result, dict) else []
            by_id = {str(item.get("request_id") or ""): item for item in requests if isinstance(item, dict)}
            self.assertEqual(by_id["voice-ask-old"].get("status"), "done")
            self.assertEqual(by_id["voice-ask-new"].get("status"), "pending")
        finally:
            cleanup()

    def test_voice_secretary_request_uses_targeted_system_notify(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import iter_events

            group_id = self._create_group()
            self._enable_voice_secretary(group_id)
            add_peer, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "Peer",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": True,
                },
            )
            self.assertTrue(add_peer.ok, getattr(add_peer, "error", None))
            stop_foreman, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "lead",
                    "patch": {"enabled": False},
                },
            )
            self.assertTrue(stop_foreman.ok, getattr(stop_foreman, "error", None))

            request, _ = self._call(
                "assistant_voice_request",
                {
                    "group_id": group_id,
                    "by": "voice-secretary",
                    "target": "@foreman",
                    "request_text": "Please review the weather-plan request and decide who should execute it.",
                    "summary": "Voice Secretary detected a spoken action request.",
                    "document_path": "docs/voice-secretary/weather-plan.md",
                    "source_request_id": "voice-ask-weather-plan",
                    "requires_ack": True,
                },
            )

            self.assertTrue(request.ok, getattr(request, "error", None))
            result = request.result or {}
            request_payload = result.get("request") if isinstance(result.get("request"), dict) else {}
            self.assertEqual(request_payload.get("target_actor_id"), "lead")
            self.assertEqual(request_payload.get("source_request_id"), "voice-ask-weather-plan")
            self.assertTrue(bool(request_payload.get("requires_ack")))
            ask_request = result.get("ask_request") if isinstance(result.get("ask_request"), dict) else {}
            self.assertEqual(ask_request.get("status"), "handed_off")
            self.assertEqual(ask_request.get("handoff_target"), "@foreman")

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            events = list(iter_events(group.ledger_path))
            request_events = [event for event in events if event.get("kind") == "assistant.voice.request"]
            self.assertTrue(request_events)
            notify_events = [
                event
                for event in events
                if event.get("kind") == "system.notify"
                and ((event.get("data") or {}).get("context") or {}).get("kind") == "voice_secretary_action_request"
            ]
            self.assertTrue(notify_events)
            data = notify_events[-1].get("data") or {}
            self.assertEqual(data.get("target_actor_id"), "lead")
            self.assertTrue(bool(data.get("requires_ack")))
            context = data.get("context") if isinstance(data.get("context"), dict) else {}
            self.assertEqual(context.get("document_path"), "docs/voice-secretary/weather-plan.md")
            self.assertEqual(context.get("source_request_id"), "voice-ask-weather-plan")
            self.assertNotIn("document_id", context)
            self.assertNotIn("job_id", context)
            self.assertIn("weather-plan", str(context.get("request_text") or ""))
        finally:
            cleanup()

    def test_voice_secretary_request_requires_explicit_target(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._enable_voice_secretary(group_id)

            request, _ = self._call(
                "assistant_voice_request",
                {
                    "group_id": group_id,
                    "by": "voice-secretary",
                    "request_text": "Please review the weather-plan request.",
                },
            )

            self.assertFalse(request.ok)
            self.assertEqual(request.error.code, "assistant_voice_request_failed")
            self.assertIn("target is required", request.error.message)
        finally:
            cleanup()

    def test_voice_input_stream_reads_all_unread_without_pending_job_backpressure(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            texts = [
                "First semantic window covers billing migration risks, staged rollout, owner follow-up, and rollback constraints.",
                "Second semantic window is a separate follow-up about launch metrics, customer feedback, and escalation thresholds.",
                "Third semantic window tracks meeting commitments, follow-up dates, and implementation constraints.",
                "Fourth semantic window should not wait on any job completion state.",
            ]
            document_path = ""
            for idx, text in enumerate(texts, start=1):
                append, _ = self._call(
                    "assistant_voice_transcript_append",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "session_id": "semantic-stream",
                        "segment_id": f"seg-stream-{idx}",
                        "text": text,
                        "language": "en-US",
                        "is_final": True,
                        "flush": True,
                        "document_path": document_path,
                        "trigger": {
                            "mode": "meeting",
                            "trigger_kind": "meeting_window",
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "client_session_id": "semantic-stream",
                            "language": "en-US",
                        },
                    },
                )
                self.assertTrue(append.ok, getattr(append, "error", None))
                result = append.result or {}
                self.assertTrue(bool(result.get("input_event_created")), result)
                document = result.get("document") if isinstance(result.get("document"), dict) else {}
                document_path = str(document.get("document_path") or "")
                self.assertTrue(document_path)

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            self.assertFalse(bool((state.result or {}).get("new_input_available")))

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            read_result = read.result or {}
            self.assertEqual(read_result.get("item_count"), len(texts))
            self.assertNotIn("combined_text", read_result)
            combined_text = str(read_result.get("input_text") or "")
            for text in texts:
                self.assertIn(text, combined_text)
            input_text = str(read_result.get("input_text") or "")
            self.assertIn("Voice Secretary input:", input_text)
            self.assertNotIn("--- Item", input_text)
            self.assertEqual(input_text.count("Document:"), 1)
            batches = read_result.get("input_batches") if isinstance(read_result, dict) else []
            self.assertEqual(len(batches), 1)
            self.assertEqual((batches[0] or {}).get("document_path"), document_path)
            self.assertEqual((batches[0] or {}).get("item_count"), len(texts))
            self.assertNotIn("items", batches[0] or {})

            second_read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(second_read.ok, getattr(second_read, "error", None))
            self.assertEqual((second_read.result or {}).get("item_count"), 0)
        finally:
            cleanup()

    def test_voice_settings_use_canonical_recognition_backend_names(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            legacy, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {"config": {"recognition_backend": "remote_asr"}},
                },
            )
            self.assertFalse(legacy.ok)
            self.assertEqual(legacy.error.code, "assistant_settings_update_failed")

            browser_local, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {"config": {"recognition_backend": "browser_local_asr"}},
                },
            )
            self.assertFalse(browser_local.ok)
            self.assertEqual(browser_local.error.code, "assistant_settings_update_failed")

            browser, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {"config": {"recognition_backend": "browser_asr"}},
                },
            )
            self.assertTrue(browser.ok, getattr(browser, "error", None))
            assistant = (browser.result or {}).get("assistant") if isinstance(browser.result, dict) else {}
            self.assertEqual((assistant.get("config") or {}).get("recognition_backend"), "browser_asr")

            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "voice_secretary",
                    "patch": {"config": {"recognition_backend": "external_provider_asr"}},
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))
            assistant = (update.result or {}).get("assistant") if isinstance(update.result, dict) else {}
            self.assertEqual((assistant.get("config") or {}).get("recognition_backend"), "external_provider_asr")
        finally:
            cleanup()

    def test_voice_document_save_create_new_keeps_multiple_documents(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            first, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "title": "Launch Plan",
                    "create_new": True,
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))
            first_doc = (first.result or {}).get("document") if isinstance(first.result, dict) else {}
            first_id = str((first_doc or {}).get("document_id") or "")
            self.assertTrue(first_id)
            self.assertEqual((first_doc or {}).get("title"), "Launch Plan")
            first_path = repo / str((first_doc or {}).get("workspace_path") or "")
            first_content = first_path.read_text(encoding="utf-8")
            self.assertEqual(first_content, "")
            self.assertNotIn("# Launch Plan", first_content)

            second, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "title": "Weather Follow-up",
                    "content": "# Weather Follow-up\n\n## Summary\nTokyo and Osaka weather follow-up.\n",
                    "create_new": True,
                },
            )
            self.assertTrue(second.ok, getattr(second, "error", None))
            second_doc = (second.result or {}).get("document") if isinstance(second.result, dict) else {}
            second_id = str((second_doc or {}).get("document_id") or "")
            self.assertTrue(second_id)
            self.assertNotEqual(first_id, second_id)

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            self.assertEqual((state.result or {}).get("active_document_id"), second_id)
            documents_by_id = (state.result or {}).get("documents_by_id") if isinstance(state.result, dict) else {}
            self.assertIn(first_id, documents_by_id)
            self.assertIn(second_id, documents_by_id)

            archive, _ = self._call(
                "assistant_voice_document_archive",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "document_path": str((second_doc or {}).get("document_path") or ""),
                },
            )
            self.assertTrue(archive.ok, getattr(archive, "error", None))
            archived_doc = (archive.result or {}).get("document") if isinstance(archive.result, dict) else {}
            self.assertIn("/archive/", f"/{str((archived_doc or {}).get('workspace_path') or '')}")

            state_after, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state_after.ok, getattr(state_after, "error", None))
            self.assertEqual((state_after.result or {}).get("active_document_id"), first_id)
        finally:
            cleanup()

    def test_voice_document_create_new_preserves_localized_title_in_workspace_path(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            created, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "assistant:voice_secretary",
                    "title": "東京会議メモ 測試",
                    "create_new": True,
                },
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            document = (created.result or {}).get("document") if isinstance(created.result, dict) else {}
            workspace_path = str((document or {}).get("workspace_path") or "")
            self.assertIn("東京会議メモ", workspace_path)
            self.assertIn("測試", workspace_path)
            self.assertTrue((repo / workspace_path).exists(), workspace_path)
        finally:
            cleanup()

    def test_voice_transcript_append_uses_explicit_document_target(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            first, _ = self._call(
                "assistant_voice_document_save",
                {"group_id": group_id, "by": "user", "title": "First Notes", "content": "# First\n", "create_new": True},
            )
            self.assertTrue(first.ok, getattr(first, "error", None))
            first_doc = ((first.result or {}).get("document") or {})
            first_id = str(first_doc.get("document_id") or "")
            first_path = str(first_doc.get("document_path") or "")
            second, _ = self._call(
                "assistant_voice_document_save",
                {"group_id": group_id, "by": "user", "title": "Second Notes", "content": "# Second\n", "create_new": True},
            )
            self.assertTrue(second.ok, getattr(second, "error", None))
            second_doc = ((second.result or {}).get("document") or {})
            second_id = str(second_doc.get("document_id") or "")
            second_path = str(second_doc.get("document_path") or "")
            self.assertTrue(first_id and second_id and first_id != second_id)
            self.assertTrue(first_path and second_path and first_path != second_path)

            select, _ = self._call(
                "assistant_voice_document_select",
                {"group_id": group_id, "by": "user", "document_path": first_path},
            )
            self.assertTrue(select.ok, getattr(select, "error", None))

            append, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "doc-target-session",
                    "segment_id": "seg-target",
                    "document_path": second_path,
                    "text": "put this short note into the explicitly targeted second document",
                    "language": "en-US",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "doc-target-session",
                        "language": "en-US",
                    },
                },
            )
            self.assertTrue(append.ok, getattr(append, "error", None))
            result = append.result or {}
            input_event = result.get("input_event") if isinstance(result.get("input_event"), dict) else {}
            self.assertEqual(input_event.get("document_path"), second_path)
            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            self.assertEqual((state.result or {}).get("active_document_id"), second_id)
            self.assertEqual((state.result or {}).get("active_document_path"), second_path)
        finally:
            cleanup()

    def test_voice_document_save_does_not_change_capture_target(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            first, _ = self._call(
                "assistant_voice_document_save",
                {"group_id": group_id, "by": "user", "title": "First Notes", "content": "# First\n", "create_new": True},
            )
            self.assertTrue(first.ok, getattr(first, "error", None))
            first_doc = ((first.result or {}).get("document") or {})
            first_id = str(first_doc.get("document_id") or "")
            first_path = str(first_doc.get("document_path") or "")
            second, _ = self._call(
                "assistant_voice_document_save",
                {"group_id": group_id, "by": "user", "title": "Second Notes", "content": "# Second\n", "create_new": True},
            )
            self.assertTrue(second.ok, getattr(second, "error", None))
            second_doc = ((second.result or {}).get("document") or {})
            second_id = str(second_doc.get("document_id") or "")
            second_path = str(second_doc.get("document_path") or "")
            self.assertTrue(first_id and second_id and first_id != second_id)
            self.assertTrue(first_path and second_path and first_path != second_path)

            select, _ = self._call(
                "assistant_voice_document_select",
                {"group_id": group_id, "by": "user", "document_path": first_path},
            )
            self.assertTrue(select.ok, getattr(select, "error", None))

            save_second, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "document_path": second_path,
                    "title": "Second Notes Updated",
                    "content": "# Second\n\nUpdated while capture target is first.\n",
                },
            )
            self.assertTrue(save_second.ok, getattr(save_second, "error", None))

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            self.assertEqual((state.result or {}).get("active_document_id"), first_id)
            self.assertEqual((state.result or {}).get("capture_target_document_id"), first_id)
            self.assertEqual((state.result or {}).get("active_document_path"), first_path)
            self.assertEqual((state.result or {}).get("capture_target_document_path"), first_path)
            documents = (state.result or {}).get("documents") if isinstance(state.result, dict) else []
            self.assertEqual([str(item.get("document_id") or "") for item in documents if isinstance(item, dict)], [second_id, first_id])
        finally:
            cleanup()

    def test_voice_document_save_does_not_rename_existing_document(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            created, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "title": "Original Notes",
                    "content": "# Original Notes\n\n## Working Notes\n",
                    "create_new": True,
                },
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            created_doc = (created.result or {}).get("document") if isinstance(created.result, dict) else {}
            document_id = str((created_doc or {}).get("document_id") or "")
            document_path = str((created_doc or {}).get("document_path") or "")
            self.assertTrue(document_id)
            self.assertTrue(document_path)

            saved, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "document_path": document_path,
                    "title": "Renamed Notes",
                    "content": "# Renamed Notes\n\n## Working Notes\nKeep the original document id.\n",
                },
            )
            self.assertTrue(saved.ok, getattr(saved, "error", None))
            saved_doc = (saved.result or {}).get("document") if isinstance(saved.result, dict) else {}
            self.assertEqual(str((saved_doc or {}).get("document_id") or ""), document_id)
            self.assertEqual(str((saved_doc or {}).get("title") or ""), "Original Notes")
            self.assertEqual(str((saved_doc or {}).get("workspace_path") or ""), str((created_doc or {}).get("workspace_path") or ""))

            stale, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "document_path": "docs/voice-secretary/does-not-exist.md",
                    "title": "Should Not Exist",
                    "content": "# Should Not Exist\n",
                },
            )
            self.assertFalse(stale.ok)
            self.assertEqual(stale.error.code, "assistant_voice_document_save_failed")

            state, _ = self._call("assistant_state", {"group_id": group_id, "assistant_id": "voice_secretary"})
            self.assertTrue(state.ok, getattr(state, "error", None))
            documents_by_id = (state.result or {}).get("documents_by_id") if isinstance(state.result, dict) else {}
            self.assertEqual(list(documents_by_id.keys()), [document_id])
        finally:
            cleanup()

    def test_voice_document_instruction_appends_input_and_notifies_actor(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import iter_events

            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            created, _ = self._call(
                "assistant_voice_document_save",
                {
                    "group_id": group_id,
                    "by": "user",
                    "title": "Launch Notes",
                    "content": "# Launch Notes\n\n## Working Notes\nExisting summary.\n",
                    "create_new": True,
                },
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            document = (created.result or {}).get("document") if isinstance(created.result, dict) else {}
            document_id = str((document or {}).get("document_id") or "")
            document_path = str((document or {}).get("document_path") or "")
            self.assertTrue(document_id)
            self.assertTrue(document_path)

            instruction, _ = self._call(
                "assistant_voice_document_instruction",
                {
                    "group_id": group_id,
                    "by": "user",
                    "document_path": document_path,
                    "instruction": "Rewrite this into a concise launch-risk summary.",
                    "trigger": {
                        "trigger_kind": "user_instruction",
                        "mode": "meeting",
                        "recognition_backend": "browser_asr",
                        "language": "en-US",
                    },
                },
            )
            self.assertTrue(instruction.ok, getattr(instruction, "error", None))
            result = instruction.result or {}
            request_id = str(result.get("request_id") or "")
            self.assertTrue(request_id.startswith("voice-ask-"))
            input_event = result.get("input_event") if isinstance(result, dict) else {}
            self.assertEqual(input_event.get("kind"), "user_instruction")
            self.assertEqual(input_event.get("document_path"), document_path)
            self.assertTrue(bool(result.get("input_notify_emitted")), result)
            self.assertIn("Existing summary.", str(((result.get("document") or {}).get("content")) or ""))
            self.assertNotIn("Rewrite this into", str(((result.get("document") or {}).get("content")) or ""))

            read, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(read.ok, getattr(read, "error", None))
            document_input_text = str((read.result or {}).get("input_text") or "")
            self.assertIn("Mode: document", document_input_text)
            self.assertIn("Required output:", document_input_text)
            self.assertIn("Edit the repository markdown directly, then use MCP tool cccc_voice_secretary_request", document_input_text)
            self.assertIn("Task:", document_input_text)
            self.assertIn("concise launch-risk summary", document_input_text)
            batches = (read.result or {}).get("input_batches") if isinstance(read.result, dict) else []
            self.assertTrue(batches)
            self.assertEqual(batches[0].get("mode"), "document")
            self.assertEqual(batches[0].get("request_kind"), "document_request")
            self.assertTrue(batches[0].get("requires_report"))
            self.assertTrue(batches[0].get("required_outputs"))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            events = list(iter_events(group.ledger_path))
            input_notifies = [
                event
                for event in events
                if event.get("kind") == "system.notify"
                and ((event.get("data") or {}).get("context") or {}).get("kind") == "voice_secretary_input"
            ]
            self.assertTrue(input_notifies)
            notify_context = ((input_notifies[-1].get("data") or {}).get("context") or {})
            self.assertEqual(notify_context.get("reason"), "new_input")
            envelope = notify_context.get("input_envelope") if isinstance(notify_context.get("input_envelope"), dict) else {}
            self.assertIn(request_id, envelope.get("report_request_ids") or [])
            self.assertIn(request_id, envelope.get("secretary_request_ids") or [])
        finally:
            cleanup()

    def test_voice_idle_review_follows_input_stream_stop_and_count_triggers(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.assistants import assistant_ops

            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            old_threshold = assistant_ops._VOICE_IDLE_REVIEW_FLUSH_THRESHOLD
            old_cooldown = assistant_ops._VOICE_IDLE_REVIEW_GROUP_COOLDOWN_SECONDS
            assistant_ops._VOICE_IDLE_REVIEW_FLUSH_THRESHOLD = 2
            assistant_ops._VOICE_IDLE_REVIEW_GROUP_COOLDOWN_SECONDS = 0
            try:
                first, _ = self._call(
                    "assistant_voice_transcript_append",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "session_id": "idle-review-session",
                        "segment_id": "seg-idle-review-1",
                        "text": "summarize the onboarding risks and keep the follow-up owner visible",
                        "language": "en-US",
                        "is_final": True,
                        "flush": True,
                        "trigger": {
                            "mode": "meeting",
                            "trigger_kind": "meeting_window",
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "client_session_id": "idle-review-session",
                            "language": "en-US",
                        },
                    },
                )
                self.assertTrue(first.ok, getattr(first, "error", None))
                self.assertTrue(((first.result or {}).get("input_event") or {}).get("text"))

                second, _ = self._call(
                    "assistant_voice_transcript_append",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "session_id": "idle-review-session",
                        "segment_id": "seg-idle-review-2",
                        "text": "capture the launch-blocker list and turn the raw notes into a polished brief",
                        "language": "en-US",
                        "is_final": True,
                        "flush": True,
                        "trigger": {
                            "mode": "meeting",
                            "trigger_kind": "meeting_window",
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "client_session_id": "idle-review-session",
                            "language": "en-US",
                        },
                    },
                )
                self.assertTrue(second.ok, getattr(second, "error", None))

                batch, _ = self._call(
                    "assistant_voice_document_input_read",
                    {"group_id": group_id, "by": "assistant:voice_secretary"},
                )
                self.assertTrue(batch.ok, getattr(batch, "error", None))
                self.assertEqual((batch.result or {}).get("item_count"), 3, batch.result)
                idle_text = str((batch.result or {}).get("input_text") or "")
                self.assertIn("Publishable document refinement request", idle_text)
                self.assertIn("evidence-bounded reconstruction", idle_text)
                self.assertIn("Correct likely ASR term errors", idle_text)
                self.assertIn("without lossy compression", idle_text)
                self.assertIn("Preserve useful concrete details", idle_text)
                self.assertIn("Do not replace detail-rich material with a short executive summary", idle_text)
                self.assertIn("Pending Inputs", idle_text)
                self.assertIn("Never include transcript segment ids", idle_text)
                self.assertNotIn("items", batch.result or {})

                third, _ = self._call(
                    "assistant_voice_transcript_append",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "session_id": "idle-review-session",
                        "segment_id": "seg-idle-review-3",
                        "text": "one more short note before the user stops recording",
                        "language": "en-US",
                        "is_final": True,
                        "flush": True,
                        "trigger": {
                            "mode": "meeting",
                            "trigger_kind": "meeting_window",
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "client_session_id": "idle-review-session",
                            "language": "en-US",
                        },
                    },
                )
                self.assertTrue(third.ok, getattr(third, "error", None))

                stop, _ = self._call(
                    "assistant_voice_transcript_append",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "session_id": "idle-review-session",
                        "segment_id": "",
                        "text": "",
                        "language": "en-US",
                        "is_final": True,
                        "flush": True,
                        "trigger": {
                            "mode": "meeting",
                            "trigger_kind": "push_to_talk_stop",
                            "capture_mode": "browser",
                            "recognition_backend": "browser_asr",
                            "client_session_id": "idle-review-session",
                            "language": "en-US",
                        },
                    },
                )
                self.assertTrue(stop.ok, getattr(stop, "error", None))

                stop_batch, _ = self._call(
                    "assistant_voice_document_input_read",
                    {"group_id": group_id, "by": "assistant:voice_secretary"},
                )
                self.assertTrue(stop_batch.ok, getattr(stop_batch, "error", None))
                self.assertEqual((stop_batch.result or {}).get("item_count"), 2, stop_batch.result)
                stop_text = str((stop_batch.result or {}).get("input_text") or "")
                self.assertIn("one more short note", stop_text)
                self.assertIn("Publishable document refinement request", stop_text)
                self.assertIn("coherent publishable artifact", stop_text)
                self.assertIn("detail-rich", stop_text)
            finally:
                assistant_ops._VOICE_IDLE_REVIEW_FLUSH_THRESHOLD = old_threshold
                assistant_ops._VOICE_IDLE_REVIEW_GROUP_COOLDOWN_SECONDS = old_cooldown
        finally:
            cleanup()

    def test_voice_idle_review_does_not_advance_cursor_before_readable_input_exists(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.assistants import assistant_ops

            group_id = self._create_group()
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))
            self._enable_voice_secretary(group_id)

            append, _ = self._call(
                "assistant_voice_transcript_append",
                {
                    "group_id": group_id,
                    "by": "user",
                    "session_id": "idle-review-failure-session",
                    "segment_id": "seg-idle-review-failure-1",
                    "text": "capture the key launch risks and turn this into a polished working document",
                    "language": "en-US",
                    "is_final": True,
                    "flush": True,
                    "trigger": {
                        "mode": "meeting",
                        "trigger_kind": "meeting_window",
                        "capture_mode": "browser",
                        "recognition_backend": "browser_asr",
                        "client_session_id": "idle-review-failure-session",
                        "language": "en-US",
                    },
                },
            )
            self.assertTrue(append.ok, getattr(append, "error", None))

            with patch("cccc.daemon.assistants.assistant_ops._append_voice_input_event", side_effect=RuntimeError("append failed")):
                dispatched = assistant_ops.dispatch_voice_idle_review(group_id, {"manual"}, "voice-input-1", "event")
            self.assertFalse(dispatched)

            group = assistant_ops.load_group(group_id)
            self.assertIsNotNone(group)
            state_after_failed_dispatch = assistant_ops._load_voice_input_state(group)
            self.assertEqual(int(state_after_failed_dispatch.get("last_idle_review_input_seq") or 0), 0)

            dispatched = assistant_ops.dispatch_voice_idle_review(group_id, {"manual"}, "voice-input-1", "event")
            self.assertTrue(dispatched)

            batch, _ = self._call(
                "assistant_voice_document_input_read",
                {"group_id": group_id, "by": "assistant:voice_secretary"},
            )
            self.assertTrue(batch.ok, getattr(batch, "error", None))
            self.assertEqual((batch.result or {}).get("item_count"), 2, batch.result)
            input_text = str((batch.result or {}).get("input_text") or "")
            self.assertIn("capture the key launch risks", input_text)
            self.assertIn("Publishable document refinement request", input_text)
        finally:
            cleanup()

    def test_pet_settings_are_read_only_in_assistant_seam(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            update, _ = self._call(
                "assistant_settings_update",
                {
                    "group_id": group_id,
                    "by": "user",
                    "assistant_id": "pet",
                    "patch": {"enabled": True},
                },
            )

            self.assertFalse(update.ok)
            self.assertEqual(update.error.code, "assistant_settings_read_only")
        finally:
            cleanup()

if __name__ == "__main__":
    unittest.main()
