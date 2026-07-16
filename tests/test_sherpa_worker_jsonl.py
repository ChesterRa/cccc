import importlib
import io
import json
import sys
import types
import unittest
from unittest.mock import patch

from cccc.daemon.assistants.sherpa_offline_asr import normalize_sherpa_sense_voice_language


class TestSherpaWorkerJsonl(unittest.TestCase):
    def _assert_send_handles_non_utf8_stdout(self, module_name: str) -> None:
        with patch.dict(sys.modules, {"numpy": types.SimpleNamespace(), "sherpa_onnx": types.SimpleNamespace()}):
            module = importlib.import_module(module_name)

        raw = io.BytesIO()
        stdout = io.TextIOWrapper(raw, encoding="cp932", write_through=True)
        with patch.object(sys, "stdout", stdout):
            module._send({"type": "partial", "text": "你好，语音秘书"})

        payload = json.loads(raw.getvalue().decode("utf-8"))
        self.assertEqual(payload["text"], "你好，语音秘书")

    def test_streaming_worker_send_writes_utf8_bytes(self) -> None:
        self._assert_send_handles_non_utf8_stdout("cccc.daemon.assistants.sherpa_streaming_worker")

    def test_offline_worker_send_writes_utf8_bytes(self) -> None:
        self._assert_send_handles_non_utf8_stdout("cccc.daemon.assistants.sherpa_offline_worker")

    def test_sense_voice_language_normalization(self) -> None:
        self.assertEqual(normalize_sherpa_sense_voice_language("zh-CN"), "zh")
        self.assertEqual(normalize_sherpa_sense_voice_language("en-US"), "en")
        self.assertEqual(normalize_sherpa_sense_voice_language("ja-JP"), "ja")
        self.assertEqual(normalize_sherpa_sense_voice_language("ko-KR"), "ko")
        self.assertEqual(normalize_sherpa_sense_voice_language("mixed"), "auto")
        self.assertEqual(normalize_sherpa_sense_voice_language("zh-HK"), "yue")
