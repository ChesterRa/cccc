from __future__ import annotations

import asyncio
import unittest
from unittest.mock import AsyncMock, patch

from cccc.daemon.assistants.voice_final_asr import (
    FinalAsrEvent,
    FinalAsrResult,
    build_final_asr_text_event,
    collect_final_asr_result,
    collect_final_asr_text,
    iter_final_asr_events,
)
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
                        language="zh-CN",
                    )
                ]

            open_session.assert_awaited_once_with("sense_voice", sample_rate=16000, language="zh-CN")
            self.assertEqual(session.calls, 2)
            self.assertTrue(session.closed)
            self.assertEqual([event.text for event in events if event.text], ["text-1", "text-2"])
            self.assertIn("model_loading", [str(event.payload.get("stage") or "") for event in events])
            final_payloads = [event.payload for event in events if event.payload.get("type") == "final"]
            self.assertEqual(final_payloads[0]["model_id"], "sense_voice")
            self.assertEqual(final_payloads[0]["engine"], "sense_voice")
            self.assertEqual(final_payloads[0]["language"], "zh")
            self.assertIn("quality_flags", final_payloads[0])
            self.assertEqual([payload["index"] for payload in final_payloads], [1, 2])
            self.assertEqual([payload["recording_segment_index"] for payload in final_payloads], [1, 1])
            self.assertEqual(final_payloads[0]["bytes"], len(b"\x01\x00" * 16000))

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


class CollectFinalAsrTextTests(unittest.TestCase):
    @staticmethod
    def _final_event(text: str) -> FinalAsrEvent:
        return FinalAsrEvent({"type": "final", "text": text}, text=text)

    @staticmethod
    def _progress_event(stage: str) -> FinalAsrEvent:
        return FinalAsrEvent({"type": "final_asr_progress", "stage": stage}, text="")

    def test_empty_audio_returns_empty_string(self) -> None:
        async def run_case() -> None:
            async def fake_iter(*_args: object, **_kwargs: object):
                if False:
                    yield FinalAsrEvent({})

            with patch("cccc.daemon.assistants.voice_final_asr.iter_final_asr_events", fake_iter):
                text = await collect_final_asr_text(
                    b"",
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                )
            self.assertEqual(text, "")

        asyncio.run(run_case())

    def test_joins_chinese_segments_without_space(self) -> None:
        async def run_case() -> None:
            events = [
                self._progress_event("segments_ready"),
                self._final_event("今天苏州"),
                self._progress_event("transcribing"),
                self._final_event("天气怎么样"),
            ]

            async def fake_iter(*_args: object, **_kwargs: object):
                for event in events:
                    yield event

            with patch("cccc.daemon.assistants.voice_final_asr.iter_final_asr_events", fake_iter):
                text = await collect_final_asr_text(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                )
            self.assertEqual(text, "今天苏州天气怎么样")

        asyncio.run(run_case())

    def test_joins_mixed_language_with_space(self) -> None:
        async def run_case() -> None:
            events = [
                self._final_event("Open"),
                self._final_event("the door"),
            ]

            async def fake_iter(*_args: object, **_kwargs: object):
                for event in events:
                    yield event

            with patch("cccc.daemon.assistants.voice_final_asr.iter_final_asr_events", fake_iter):
                text = await collect_final_asr_text(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                )
            self.assertEqual(text, "Open the door")

        asyncio.run(run_case())

    def test_preserves_space_after_ascii_sentence_punctuation(self) -> None:
        async def run_case() -> None:
            events = [
                self._final_event("Hello."),
                self._final_event("How are you?"),
            ]

            async def fake_iter(*_args: object, **_kwargs: object):
                for event in events:
                    yield event

            with patch("cccc.daemon.assistants.voice_final_asr.iter_final_asr_events", fake_iter):
                text = await collect_final_asr_text(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                )
            self.assertEqual(text, "Hello. How are you?")

        asyncio.run(run_case())

    def test_skips_empty_and_whitespace_finals(self) -> None:
        async def run_case() -> None:
            events = [
                self._final_event(""),
                self._final_event("   "),
                self._final_event("你好"),
            ]

            async def fake_iter(*_args: object, **_kwargs: object):
                for event in events:
                    yield event

            with patch("cccc.daemon.assistants.voice_final_asr.iter_final_asr_events", fake_iter):
                text = await collect_final_asr_text(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                )
            self.assertEqual(text, "你好")

        asyncio.run(run_case())


class CollectFinalAsrResultTests(unittest.TestCase):
    @staticmethod
    def _final_event(text: str, *, index: int = 0, start_ms: int = 0, end_ms: int = 1000) -> FinalAsrEvent:
        payload = {"type": "final", "text": text, "start_ms": start_ms, "end_ms": end_ms}
        if index:
            payload["index"] = index
        return FinalAsrEvent(payload, text=text)

    def test_accumulates_segments_with_recording_segment_index(self) -> None:
        async def run_case() -> None:
            events = [
                self._final_event("今天苏州", index=1, start_ms=0, end_ms=1000),
                self._final_event("天气怎么样", index=2, start_ms=1200, end_ms=2400),
            ]

            async def fake_iter(*_args: object, **_kwargs: object):
                for event in events:
                    yield event

            with patch("cccc.daemon.assistants.voice_final_asr.iter_final_asr_events", fake_iter):
                result = await collect_final_asr_result(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                )
            self.assertEqual(result.text, "今天苏州天气怎么样")
            self.assertEqual([segment["index"] for segment in result.segments], [1, 2])
            self.assertEqual(
                [segment["recording_segment_index"] for segment in result.segments],
                [1, 1],
            )
            self.assertEqual(result.segments[0]["start_ms"], 0)
            self.assertEqual(result.segments[1]["end_ms"], 2400)
            self.assertTrue(all(segment["ok"] for segment in result.segments))
            self.assertEqual(result.failed_segment_count, 0)

        asyncio.run(run_case())

    def test_legacy_fallback_restarts_segment_accumulation(self) -> None:
        async def run_case() -> None:
            events = [
                self._final_event("offline 第一段", index=1),
                FinalAsrEvent({"type": "final_asr_progress", "stage": "legacy_fallback"}, text=""),
                self._final_event("fallback 第一段", index=1),
                self._final_event("fallback 第二段", index=2),
            ]

            async def fake_iter(*_args: object, **_kwargs: object):
                for event in events:
                    yield event

            with patch("cccc.daemon.assistants.voice_final_asr.iter_final_asr_events", fake_iter):
                result = await collect_final_asr_result(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                )
            self.assertEqual(result.text, "fallback 第一段 fallback 第二段")
            self.assertEqual([segment["text"] for segment in result.segments], ["fallback 第一段", "fallback 第二段"])

        asyncio.run(run_case())

    def test_reports_partial_result_when_a_later_segment_fails(self) -> None:
        async def run_case() -> None:
            events = [
                self._final_event("第一段", index=1, start_ms=0, end_ms=1000),
                FinalAsrEvent(
                    {
                        "type": "final_asr_failed",
                        "index": 2,
                        "start_ms": 1200,
                        "end_ms": 2400,
                        "bytes": 38400,
                        "model_id": "sense_voice",
                        "sample_rate": 16000,
                        "error": {"code": "decode_failed", "message": "segment failed"},
                    }
                ),
            ]

            async def fake_iter(*_args: object, **_kwargs: object):
                for event in events:
                    yield event

            with patch("cccc.daemon.assistants.voice_final_asr.iter_final_asr_events", fake_iter):
                result = await collect_final_asr_result(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                )

            self.assertEqual(result.text, "第一段")
            self.assertEqual(result.failed_segment_count, 1)
            self.assertEqual([segment["ok"] for segment in result.segments], [True, False])
            self.assertEqual(result.segments[1]["index"], 2)
            self.assertEqual(result.segments[1]["error"]["code"], "decode_failed")

        asyncio.run(run_case())


class BuildFinalAsrTextEventTests(unittest.TestCase):
    def test_returns_none_when_collect_raises(self) -> None:
        async def run_case() -> None:
            with patch(
                "cccc.daemon.assistants.voice_final_asr.collect_final_asr_result",
                AsyncMock(side_effect=RuntimeError("boom")),
            ):
                event = await build_final_asr_text_event(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                    seq=42,
                )
            self.assertIsNone(event)

        asyncio.run(run_case())

    def test_returns_none_when_text_empty(self) -> None:
        async def run_case() -> None:
            with patch(
                "cccc.daemon.assistants.voice_final_asr.collect_final_asr_result",
                AsyncMock(return_value=FinalAsrResult(text="   ")),
            ):
                event = await build_final_asr_text_event(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                    seq=42,
                )
            self.assertIsNone(event)

        asyncio.run(run_case())

    def test_returns_event_payload_when_text_available(self) -> None:
        async def run_case() -> None:
            segments = (
                {
                    "index": 1,
                    "recording_segment_index": 1,
                    "start_ms": 0,
                    "end_ms": 1000,
                    "bytes": 32000,
                    "ok": True,
                    "text": "今天苏州天气怎么样",
                    "model_id": "sense_voice",
                    "sample_rate": 16000,
                },
            )
            with patch(
                "cccc.daemon.assistants.voice_final_asr.collect_final_asr_result",
                AsyncMock(return_value=FinalAsrResult(text="今天苏州天气怎么样", segments=segments)),
            ):
                event = await build_final_asr_text_event(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                    seq=42,
                )
            self.assertEqual(event, {
                "type": "final_asr_text",
                "ok": True,
                "seq": 42,
                "text": "今天苏州天气怎么样",
                "source": "assistant_service_local_asr_final",
                "model_id": "sense_voice",
                "language": "auto",
                "segments": list(segments),
                "segment_count": 1,
                "partial": False,
                "failed_segment_count": 0,
            })

        asyncio.run(run_case())

    def test_returns_event_payload_with_effective_language(self) -> None:
        async def run_case() -> None:
            with patch(
                "cccc.daemon.assistants.voice_final_asr.collect_final_asr_result",
                AsyncMock(return_value=FinalAsrResult(text="今天苏州天气怎么样")),
            ) as collect_result:
                event = await build_final_asr_text_event(
                    b"\x01\x00" * 16000,
                    selected_model_id="sense_voice",
                    sample_rate=16000,
                    seq=42,
                    language="zh-CN",
                )
            collect_result.assert_awaited_once_with(
                b"\x01\x00" * 16000,
                selected_model_id="sense_voice",
                sample_rate=16000,
                language="zh-CN",
            )
            self.assertEqual(event["language"], "zh")
            self.assertEqual(event["segments"], [])

        asyncio.run(run_case())


if __name__ == "__main__":
    unittest.main()
