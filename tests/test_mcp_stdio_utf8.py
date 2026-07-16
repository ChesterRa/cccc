from __future__ import annotations

import io
import json
import unittest
from unittest.mock import patch


class _FakeBinaryStdin:
    def __init__(self, data: bytes) -> None:
        self.buffer = io.BytesIO(data)

    def readline(self) -> str:
        raise AssertionError("text stdin path should not be used when a binary buffer exists")


class _FakeBinaryStdout:
    def __init__(self) -> None:
        self.buffer = io.BytesIO()

    def write(self, _data: str) -> int:
        raise AssertionError("text stdout path should not be used when a binary buffer exists")

    def flush(self) -> None:
        return None


class TestMcpStdioUtf8(unittest.TestCase):
    def setUp(self) -> None:
        from cccc.ports.mcp import main as mcp_main

        mcp_main._reset_session_state_for_tests()

    def test_read_message_uses_binary_utf8_stdin(self) -> None:
        from cccc.ports.mcp import main as mcp_main

        stdin = _FakeBinaryStdin(b'{"jsonrpc":"2.0","id":1,"method":"ping","params":{"text":"\xe5\xbc\x80"}}\n')
        with patch.object(mcp_main.sys, "stdin", stdin):
            msg = mcp_main._read_message()

        self.assertIsInstance(msg, dict)
        self.assertEqual(str((msg or {}).get("method") or ""), "ping")
        params = (msg or {}).get("params") if isinstance((msg or {}).get("params"), dict) else {}
        self.assertEqual(str(params.get("text") or ""), "开")

    def test_write_message_uses_binary_utf8_stdout(self) -> None:
        from cccc.ports.mcp import main as mcp_main

        stdout = _FakeBinaryStdout()
        with patch.object(mcp_main.sys, "stdout", stdout):
            mcp_main._write_message({"jsonrpc": "2.0", "id": 1, "result": {"text": "开始"}})

        payload = stdout.buffer.getvalue().decode("utf-8")
        parsed = json.loads(payload.strip())
        result = parsed.get("result") if isinstance(parsed.get("result"), dict) else {}
        self.assertEqual(str(result.get("text") or ""), "开始")

    def test_read_message_content_length_framing(self) -> None:
        from cccc.ports.mcp import main as mcp_main

        body = json.dumps(
            {"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {"text": "hi"}},
            ensure_ascii=False,
        ).encode("utf-8")
        stdin = _FakeBinaryStdin(
            f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8") + body
        )
        with patch.object(mcp_main.sys, "stdin", stdin):
            msg = mcp_main._read_message()

        self.assertIsInstance(msg, dict)
        self.assertEqual(str((msg or {}).get("method") or ""), "ping")
        self.assertTrue(mcp_main._STDIO_WRITE_CONTENT_LENGTH)

    def test_write_message_content_length_when_write_flag_set(self) -> None:
        from cccc.ports.mcp import main as mcp_main

        mcp_main._STDIO_WRITE_CONTENT_LENGTH = True
        stdout = _FakeBinaryStdout()
        with patch.object(mcp_main.sys, "stdout", stdout):
            mcp_main._write_message({"jsonrpc": "2.0", "id": 1, "result": {"ok": True}})

        raw = stdout.buffer.getvalue()
        self.assertTrue(raw.startswith(b"Content-Length:"))
        body_start = raw.find(b"\r\n\r\n")
        self.assertGreater(body_start, 0)
        parsed = json.loads(raw[body_start + 4 :].decode("utf-8"))
        result = parsed.get("result") if isinstance(parsed.get("result"), dict) else {}
        self.assertTrue(bool(result.get("ok")))

    def test_initialize_negotiates_grok_protocol_version_without_forcing_content_length(self) -> None:
        from cccc.ports.mcp import main as mcp_main

        resp = mcp_main.handle_request(
            {
                "jsonrpc": "2.0",
                "id": 0,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "clientInfo": {"name": "grok-shell-collab", "version": "0.2.16"},
                },
            }
        )
        result = resp.get("result") if isinstance(resp.get("result"), dict) else {}
        self.assertEqual(str(result.get("protocolVersion") or ""), "2025-06-18")
        self.assertFalse(mcp_main._STDIO_WRITE_CONTENT_LENGTH)

        stdout = _FakeBinaryStdout()
        with patch.object(mcp_main.sys, "stdout", stdout):
            mcp_main._write_message(resp)

        raw = stdout.buffer.getvalue()
        self.assertFalse(raw.startswith(b"Content-Length:"))
        parsed = json.loads(raw.decode("utf-8").strip())
        parsed_result = parsed.get("result") if isinstance(parsed.get("result"), dict) else {}
        self.assertEqual(str(parsed_result.get("protocolVersion") or ""), "2025-06-18")

    def test_initialize_keeps_legacy_protocol_version(self) -> None:
        from cccc.ports.mcp import main as mcp_main

        resp = mcp_main.handle_request(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"protocolVersion": "2024-11-05"},
            }
        )
        result = resp.get("result") if isinstance(resp.get("result"), dict) else {}
        self.assertEqual(str(result.get("protocolVersion") or ""), "2024-11-05")

    def test_text_stream_fallback_still_works_without_binary_buffer(self) -> None:
        from cccc.ports.mcp import main as mcp_main

        stdin = io.StringIO('{"jsonrpc":"2.0","id":2,"method":"ping"}\n')
        stdout = io.StringIO()
        with patch.object(mcp_main.sys, "stdin", stdin), patch.object(mcp_main.sys, "stdout", stdout):
            msg = mcp_main._read_message()
            mcp_main._write_message({"jsonrpc": "2.0", "id": 2, "result": {"ok": True}})

        self.assertEqual(str((msg or {}).get("method") or ""), "ping")
        parsed = json.loads(stdout.getvalue().strip())
        result = parsed.get("result") if isinstance(parsed.get("result"), dict) else {}
        self.assertTrue(bool(result.get("ok")))


if __name__ == "__main__":
    unittest.main()
