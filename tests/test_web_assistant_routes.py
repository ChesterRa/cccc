import base64
import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebAssistantRoutes(unittest.TestCase):
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

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

    def _create_group(self) -> str:
        resp = self._local_call_daemon({"op": "group_create", "args": {"title": "assistant-web", "topic": "", "by": "user"}})
        self.assertTrue(bool(resp.get("ok")), resp)
        group_id = str(((resp.get("result") or {}).get("group_id")) or "").strip()
        self.assertTrue(group_id)
        return group_id

    def _add_foreman(self, group_id: str) -> None:
        resp = self._local_call_daemon(
            {
                "op": "actor_add",
                "args": {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "lead",
                    "title": "Foreman",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": True,
                },
            }
        )
        self.assertTrue(bool(resp.get("ok")), resp)

    def _attach_scope(self, group_id: str, path: str) -> None:
        resp = self._local_call_daemon({"op": "attach", "args": {"group_id": group_id, "path": path, "by": "user"}})
        self.assertTrue(bool(resp.get("ok")), resp)

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
            "print(json.dumps({'text': f'web managed transcript:{audio.suffix}:{audio.stat().st_size}'}))\n"
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

    def test_web_voice_secretary_transcription_route_uses_service_asr(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        old_mock = os.environ.get("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT")
        old_command = os.environ.get("CCCC_VOICE_SECRETARY_ASR_COMMAND")
        os.environ["CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT"] = "web service transcript"
        os.environ.pop("CCCC_VOICE_SECRETARY_ASR_COMMAND", None)
        group = None
        try:
            from cccc.daemon.assistants.voice_service_runtime import stop_voice_service
            from cccc.kernel.group import load_group

            group_id = self._create_group()
            self._add_foreman(group_id)

            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                with TestClient(create_app()) as client:
                    settings_resp = client.put(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/settings",
                        json={
                            "enabled": True,
                            "config": {
                                "capture_mode": "service",
                                "recognition_backend": "assistant_service_local_asr",
                                "retention_ttl_seconds": 120,
                                "tts_enabled": False,
                            },
                        },
                    )
                    self.assertEqual(settings_resp.status_code, 200)
                    self.assertTrue(bool(settings_resp.json().get("ok")), settings_resp.json())

                    transcribe_resp = client.post(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/transcriptions",
                        json={
                            "audio_base64": base64.b64encode(b"fake audio bytes").decode("ascii"),
                            "mime_type": "audio/webm",
                            "language": "en-US",
                            "by": "user",
                        },
                    )
                    self.assertEqual(transcribe_resp.status_code, 200)
                    transcribe_body = transcribe_resp.json()
                    self.assertTrue(bool(transcribe_body.get("ok")), transcribe_body)
                    result = transcribe_body.get("result") or {}
                    self.assertEqual(str(result.get("transcript") or ""), "web service transcript")
                    self.assertEqual(str(result.get("backend") or ""), "assistant_service_local_asr")
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

    def test_web_voice_secretary_model_install_route_downloads_and_enables_managed_model(self) -> None:
        from cccc.ports.web.app import create_app

        home, cleanup = self._with_home()
        old_mock = os.environ.pop("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT", None)
        old_command = os.environ.pop("CCCC_VOICE_SECRETARY_ASR_COMMAND", None)
        group = None
        try:
            from cccc.daemon.assistants.voice_service_runtime import stop_voice_service
            from cccc.kernel.group import load_group

            self._write_voice_model_manifest(home)
            group_id = self._create_group()
            self._add_foreman(group_id)

            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                with TestClient(create_app()) as client:
                    settings_resp = client.put(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/settings",
                        json={
                            "enabled": True,
                            "config": {
                                "capture_mode": "service",
                                "recognition_backend": "assistant_service_local_asr",
                                "service_model_id": "mock_asr",
                                "tts_enabled": False,
                            },
                        },
                    )
                    self.assertEqual(settings_resp.status_code, 200)
                    self.assertTrue(bool(settings_resp.json().get("ok")), settings_resp.json())

                    install_resp = client.post(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/models/install",
                        json={"model_id": "mock_asr", "by": "user"},
                    )
                    self.assertEqual(install_resp.status_code, 200)
                    install_body = install_resp.json()
                    self.assertTrue(bool(install_body.get("ok")), install_body)
                    self.assertEqual(str(((install_body.get("result") or {}).get("model") or {}).get("status") or ""), "ready")

                    transcribe_resp = client.post(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/transcriptions",
                        json={
                            "audio_base64": base64.b64encode(b"fake audio bytes").decode("ascii"),
                            "mime_type": "audio/webm",
                            "language": "en-US",
                            "by": "user",
                        },
                    )
                    self.assertEqual(transcribe_resp.status_code, 200)
                    transcribe_body = transcribe_resp.json()
                    self.assertTrue(bool(transcribe_body.get("ok")), transcribe_body)
                    result = transcribe_body.get("result") or {}
                    self.assertEqual(str(result.get("transcript") or ""), "web managed transcript:.webm:16")
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

    def test_web_voice_secretary_transcript_segment_route_stores_sidecar_without_legacy_proposal(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._add_foreman(group_id)

            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                with TestClient(create_app()) as client:
                    settings_resp = client.put(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/settings",
                        json={
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
                    )
                    self.assertEqual(settings_resp.status_code, 200)
                    self.assertTrue(bool(settings_resp.json().get("ok")), settings_resp.json())

                    append_resp = client.post(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/transcript_segments",
                        json={
                            "session_id": "web-session-1",
                            "segment_id": "seg-1",
                            "text": "please inspect the web transcript segment route",
                            "language": "en-US",
                            "is_final": True,
                            "flush": True,
                            "trigger": {
                                "mode": "meeting",
                                "trigger_kind": "push_to_talk_stop",
                                "capture_mode": "browser",
                                "recognition_backend": "browser_asr",
                                "input_device_label": "browser_default",
                                "language": "en-US",
                            },
                            "by": "user",
                        },
                    )
                    self.assertEqual(append_resp.status_code, 200)
                    append_body = append_resp.json()
                    self.assertTrue(bool(append_body.get("ok")), append_body)
                    result = append_body.get("result") or {}
                    self.assertTrue(str(result.get("segment_path") or "").endswith("segments.jsonl"))
        finally:
            cleanup()

    def test_web_voice_secretary_transcript_segment_route_updates_working_document(self) -> None:
        from cccc.ports.web.app import create_app

        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._add_foreman(group_id)
            repo = Path(home) / "repo"
            repo.mkdir()
            self._attach_scope(group_id, str(repo))

            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                with TestClient(create_app()) as client:
                    settings_resp = client.put(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/settings",
                        json={
                            "enabled": True,
                            "config": {
                                "capture_mode": "browser",
                                "recognition_backend": "browser_asr",
                                "recognition_language": "en-US",
                                "retention_ttl_seconds": 120,
                                "auto_document_enabled": True,
                                "document_default_dir": "docs/voice-secretary",
                                "tts_enabled": False,
                            },
                        },
                    )
                    self.assertEqual(settings_resp.status_code, 200)
                    self.assertTrue(bool(settings_resp.json().get("ok")), settings_resp.json())

                    append_resp = client.post(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/transcript_segments",
                        json={
                            "session_id": "web-doc-session-1",
                            "segment_id": "seg-doc-1",
                            "text": "capture the launch checklist and keep unresolved risks visible",
                            "language": "en-US",
                            "is_final": True,
                            "flush": False,
                            "trigger": {
                                "mode": "meeting",
                                "trigger_kind": "meeting_window",
                                "capture_mode": "browser",
                                "recognition_backend": "browser_asr",
                                "input_device_label": "browser_default",
                                "language": "en-US",
                            },
                            "by": "user",
                        },
                    )
                    self.assertEqual(append_resp.status_code, 200)
                    append_body = append_resp.json()
                    self.assertTrue(bool(append_body.get("ok")), append_body)
                    result = append_body.get("result") or {}
                    self.assertFalse(bool(result.get("document_updated")), result)

                    flush_resp = client.post(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/transcript_segments",
                        json={
                            "session_id": "web-doc-session-1",
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
                                "input_device_label": "browser_default",
                                "language": "en-US",
                            },
                            "by": "user",
                        },
                    )
                    self.assertEqual(flush_resp.status_code, 200)
                    flush_body = flush_resp.json()
                    self.assertTrue(bool(flush_body.get("ok")), flush_body)
                    flush_result = flush_body.get("result") or {}
                    self.assertFalse(bool(flush_result.get("document_updated")), flush_result)
                    self.assertTrue(bool(flush_result.get("input_event_created")), flush_result)
                    document = flush_result.get("document") or {}
                    document_id = str(document.get("document_id") or "")
                    document_path = str(document.get("document_path") or "")
                    workspace_path = str(document.get("workspace_path") or "")
                    self.assertTrue(workspace_path.startswith("docs/voice-secretary/"), workspace_path)
                    self.assertEqual(document_path, workspace_path)
                    self.assertNotIn("capture the launch checklist", str(document.get("content") or ""))
                    self.assertTrue((repo / workspace_path).exists())

                    instruction_resp = client.post(
                        f"/api/v1/groups/{group_id}/assistants/voice_secretary/documents/instructions",
                        json={
                            "document_path": document_path,
                            "instruction": "add an owner field",
                            "by": "user",
                        },
                    )
                    self.assertEqual(instruction_resp.status_code, 200)
                    instruction_body = instruction_resp.json()
                    self.assertTrue(bool(instruction_body.get("ok")), instruction_body)
                    instruction_result = instruction_body.get("result") or {}
                    updated_document = (instruction_result.get("document")) or {}
                    self.assertTrue(bool(instruction_result.get("input_event_created")), instruction_result)
                    self.assertNotIn("Instruction: add an owner field", str(updated_document.get("content") or ""))

                    state_resp = client.get(f"/api/v1/groups/{group_id}/assistants/voice_secretary")
                    self.assertEqual(state_resp.status_code, 200)
                    state_body = state_resp.json()
            self.assertTrue(bool(state_body.get("ok")), state_body)
            documents = ((state_body.get("result") or {}).get("documents_by_id")) or {}
            self.assertIn(document_id, documents)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
