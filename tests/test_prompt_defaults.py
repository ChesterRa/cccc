from __future__ import annotations

import unittest


class TestPromptDefaults(unittest.TestCase):
    def test_default_preamble_includes_gap_policy(self) -> None:
        from cccc.kernel.prompt_files import DEFAULT_PREAMBLE_BODY

        body = str(DEFAULT_PREAMBLE_BODY or "")
        self.assertIn("Gap handling (default policy):", body)
        self.assertIn("If information is insufficient, investigate first", body)
        self.assertIn("If capability is insufficient, use capability control plane", body)
        self.assertIn('cccc_capability_search(kind="mcp_toolpack"|"skill"', body)


if __name__ == "__main__":
    unittest.main()
