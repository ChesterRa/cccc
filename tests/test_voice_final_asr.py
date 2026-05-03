from __future__ import annotations

import asyncio
import unittest
from unittest.mock import AsyncMock, patch

from cccc.daemon.assistants.voice_final_asr import iter_final_asr_events
from cccc.daemon.assistants.voice_final_asr_debug import voice_final_asr_quality_flags
from cccc.daemon.assistants.voice_pcm_segments import VoicePcmSegment


class _FakeOfflineSession:
    def __init__(self) -> None:
        self.calls = 0
        self.closed = False

    async def transcribe_pcm16(self, pcm16_audio: bytes, *, sample_rate: int = 16000) -> str:
        self.calls += 1
        return f"text-{self.calls}"

    async def close(self) -> None:
        self.closed = True


class VoiceFinalAsrTests(unittest.TestCase):
    def test_reuses_one_offline_session_for_all_segments(self) -> None:
        async def run_case() -> None:
            session = _FakeOfflineSession()
            with (
                patch("cccc.daemon.assistants.voice_final_asr.detect_sherpa_vad_segments", AsyncMock(return_value=[])),
                patch(
                    "cccc.daemon.assistants.voice_final_asr.get_voice_model_status",
                    return_value={
                        "model_id": "sense_voice",
                        "runtime_id": "sherpa_onnx_streaming",
                        "offline": {"engine": "sense_voice", "language": "auto"},
                        "offline_ready": True,
                    },
                ),
                patch(
                    "cccc.daemon.assistants.voice_final_asr.resolve_installed_voice_model_offline_config",
                    return_value={"engine": "sense_voice", "language": "auto", "sample_rate": 16000},
                ),
                patch(
                    "cccc.daemon.assistants.voice_final_asr.build_pcm16_segments_from_ranges",
                    return_value=[
                        VoicePcmSegment(start_ms=0, end_ms=1000, audio=b"\x01\x00" * 16000),
                        VoicePcmSegment(start_ms=1200, end_ms=2400, audio=b"\x02\x00" * 16000),
                    ],
                ),
                patch("cccc.daemon.assistants.voice_final_asr.open_local_offline_asr_session", AsyncMock(return_value=session)) as open_session,
            ):
                events = [
                    event
                    async for event in iter_final_asr_events(
                        b"\x01\x00" * 32000,
                        selected_model_id="sense_voice",
                        sample_rate=16000,
                        seq=7,
                    )
                ]

            open_session.assert_awaited_once()
            self.assertEqual(session.calls, 2)
            self.assertTrue(session.closed)
            self.assertEqual([event.text for event in events if event.text], ["text-1", "text-2"])
            self.assertIn("model_loading", [str(event.payload.get("stage") or "") for event in events])
            final_payloads = [event.payload for event in events if event.payload.get("type") == "final"]
            self.assertEqual(final_payloads[0]["model_id"], "sense_voice")
            self.assertEqual(final_payloads[0]["engine"], "sense_voice")
            self.assertEqual(final_payloads[0]["language"], "auto")
            self.assertIn("quality_flags", final_payloads[0])

        asyncio.run(run_case())

    def test_final_asr_keeps_normal_recordings_as_longer_context_chunks(self) -> None:
        async def run_case() -> None:
            with (
                patch("cccc.daemon.assistants.voice_final_asr.detect_sherpa_vad_segments", AsyncMock(return_value=[])),
                patch("cccc.daemon.assistants.voice_final_asr.open_local_offline_asr_session", AsyncMock(side_effect=AssertionError("stop after segment planning"))),
                patch("cccc.daemon.assistants.voice_final_asr.split_pcm16_voice_segments", return_value=[]) as split_segments,
            ):
                events = []
                async for event in iter_final_asr_events(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                    seq=7,
                ):
                    events.append(event)
                    if event.payload.get("stage") == "segments_ready":
                        break

            self.assertTrue(events)
            split_segments.assert_called_once()
            self.assertEqual(split_segments.call_args.kwargs["max_segment_ms"], 60000)

        asyncio.run(run_case())

    def test_quality_flags_marks_suspicious_ascii_fragments(self) -> None:
        flags = voice_final_asr_quality_flags("所谓 chanchanl thought 对不同的 computer 产生 pass")

        self.assertGreaterEqual(flags["suspicious_ascii_fragment_count"], 2)
        self.assertIn("chanchanl", flags["suspicious_ascii_fragments"])


if __name__ == "__main__":
    unittest.main()
