import unittest

from cccc.ports.im.adapters.feishu.text import compose_safe_text


class TestFeishuText(unittest.TestCase):
    def test_compose_safe_text_uses_summarizer_limits(self) -> None:
        calls: list[tuple[str, int, int]] = []

        def summarize(text: str, max_chars: int, max_lines: int) -> str:
            calls.append((text, max_chars, max_lines))
            return text.upper()

        result = compose_safe_text("hello", max_chars=10, max_lines=2, summarize_fn=summarize)

        self.assertEqual(result, "HELLO")
        self.assertEqual(calls, [("hello", 10, 2)])

    def test_compose_safe_text_clamps_to_feishu_message_limit(self) -> None:
        result = compose_safe_text(
            "ignored",
            max_chars=50_000,
            max_lines=10_000,
            summarize_fn=lambda _text, _max_chars, _max_lines: "x" * 31_000,
        )

        self.assertEqual(len(result), 30_722)
        self.assertTrue(result.endswith("..."))
