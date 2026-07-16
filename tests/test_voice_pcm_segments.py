from __future__ import annotations

import unittest

from cccc.daemon.assistants.voice_pcm_segments import build_pcm16_segments_from_ranges, split_pcm16_voice_segments


def _pcm(samples: list[int]) -> bytes:
    return b"".join(int(sample).to_bytes(2, "little", signed=True) for sample in samples)


def _tone(sample_count: int, amplitude: int = 3000) -> list[int]:
    return [amplitude if index % 2 == 0 else -amplitude for index in range(sample_count)]


class VoicePcmSegmentsTests(unittest.TestCase):
    def test_silence_returns_no_segments(self) -> None:
        segments = split_pcm16_voice_segments(_pcm([0] * 16000), sample_rate=16000)

        self.assertEqual(segments, [])

    def test_separates_speech_islands_by_silence(self) -> None:
        audio = _pcm(
            [0] * 1600
            + _tone(9600)
            + [0] * 16000
            + _tone(9600)
            + [0] * 1600
        )

        segments = split_pcm16_voice_segments(audio, sample_rate=16000)

        self.assertEqual(len(segments), 2)
        self.assertLess(segments[0].end_ms, segments[1].start_ms)
        self.assertGreaterEqual(segments[0].end_ms - segments[0].start_ms, 600)
        self.assertGreaterEqual(segments[1].end_ms - segments[1].start_ms, 600)

    def test_ignores_short_noise(self) -> None:
        audio = _pcm([0] * 1600 + _tone(1600) + [0] * 1600)

        segments = split_pcm16_voice_segments(audio, sample_rate=16000, min_speech_ms=400)

        self.assertEqual(segments, [])

    def test_splits_long_speech_by_max_duration(self) -> None:
        audio = _pcm(_tone(32000))

        segments = split_pcm16_voice_segments(
            audio,
            sample_rate=16000,
            min_silence_ms=700,
            max_segment_ms=700,
        )

        self.assertGreaterEqual(len(segments), 2)
        self.assertTrue(all(segment.end_ms - segment.start_ms <= 700 for segment in segments))

    def test_build_segments_from_vad_ranges_pads_merges_and_splits(self) -> None:
        audio = _pcm(_tone(32000))

        segments = build_pcm16_segments_from_ranges(
            audio,
            [
                {"start_ms": 100, "end_ms": 700},
                {"start_ms": 850, "end_ms": 1500},
            ],
            sample_rate=16000,
            pad_ms=100,
            merge_gap_ms=250,
            max_segment_ms=900,
        )

        self.assertEqual([(segment.start_ms, segment.end_ms) for segment in segments], [(0, 900), (900, 1600)])

    def test_build_segments_from_vad_ranges_merges_short_neighboring_turns(self) -> None:
        audio = _pcm(_tone(96000))

        segments = build_pcm16_segments_from_ranges(
            audio,
            [
                {"start_ms": 1000, "end_ms": 2200},
                {"start_ms": 2800, "end_ms": 3900},
                {"start_ms": 4700, "end_ms": 5600},
            ],
            sample_rate=16000,
            pad_ms=240,
            merge_gap_ms=1200,
            max_segment_ms=15000,
        )

        self.assertEqual(len(segments), 1)
        self.assertEqual((segments[0].start_ms, segments[0].end_ms), (760, 5840))


if __name__ == "__main__":
    unittest.main()
