from __future__ import annotations

import unittest

from cccc.daemon.assistants.voice_final_document_apply import (
    build_final_transcript_append_request,
    final_speaker_transcript_text,
)


class VoiceFinalDocumentApplyTests(unittest.TestCase):
    def test_final_speaker_transcript_text_keeps_speaker_context(self) -> None:
        text = final_speaker_transcript_text(
            [
                {"speaker_label": "Speaker 1", "text": "  hello   world "},
                {"speaker_label": "Speaker 2", "text": "こんにちは"},
                {"speaker_label": "", "text": "tail"},
                {"speaker_label": "Speaker 3", "text": ""},
            ]
        )

        self.assertEqual(text, "Speaker 1: hello world\nSpeaker 2: こんにちは\ntail")

    def test_build_final_transcript_append_request_uses_final_backend(self) -> None:
        request = build_final_transcript_append_request(
            group_id="g1",
            session_id="s1",
            document_path="docs/voice.md",
            speaker_transcript_segments=[
                {"speaker_label": "Speaker 1", "text": "hello", "start_ms": 100, "end_ms": 900},
                {"speaker_label": "Speaker 2", "text": "world", "start_ms": 1200, "end_ms": 2200},
            ],
            sample_rate=16000,
            language="mixed",
            final_asr_model_id="sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8",
            audio_duration_ms=3000,
        )

        self.assertIsNotNone(request)
        assert request is not None
        args = request["args"]
        self.assertEqual(request["op"], "assistant_voice_transcript_append")
        self.assertTrue(args["flush"])
        self.assertEqual(args["text"], "Speaker 1: hello\nSpeaker 2: world")
        self.assertEqual(args["document_path"], "docs/voice.md")
        self.assertEqual(args["start_ms"], 100)
        self.assertEqual(args["end_ms"], 2200)
        self.assertEqual(args["trigger"]["recognition_backend"], "assistant_service_local_asr_final")
        self.assertEqual(args["trigger"]["trigger_kind"], "service_transcript")
        self.assertEqual(args["trigger"]["final_asr_model_id"], "sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8")
        self.assertEqual(args["by"], "user")

    def test_build_final_transcript_append_request_skips_without_document_or_text(self) -> None:
        self.assertIsNone(
            build_final_transcript_append_request(
                group_id="g1",
                session_id="s1",
                document_path="",
                speaker_transcript_segments=[{"text": "hello"}],
                sample_rate=16000,
            )
        )
        self.assertIsNone(
            build_final_transcript_append_request(
                group_id="g1",
                session_id="s1",
                document_path="docs/voice.md",
                speaker_transcript_segments=[{"text": ""}],
                sample_rate=16000,
            )
        )


if __name__ == "__main__":
    unittest.main()
