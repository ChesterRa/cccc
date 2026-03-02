from __future__ import annotations

import unittest


class TestPromptDefaults(unittest.TestCase):
    def test_default_preamble_is_compact_and_actionable(self) -> None:
        from cccc.kernel.prompt_files import DEFAULT_PREAMBLE_BODY

        body = str(DEFAULT_PREAMBLE_BODY or "")
        self.assertIn("Quick start:", body)
        self.assertIn("Execution checklist:", body)
        self.assertIn("Gap routing:", body)
        self.assertIn("Memory boundary:", body)
        self.assertIn("cccc_bootstrap", body)
        self.assertIn("cccc_context_agent(action=update", body)
        self.assertIn("cccc_capability_use(...)", body)
        self.assertIn("cccc_capability_search(...)", body)
        self.assertIn("retry guidance", body)
        self.assertIn("real env/permission blockers", body)
        self.assertLessEqual(len(body.split()), 220)

    def test_default_preamble_avoids_long_rule_duplication(self) -> None:
        from cccc.kernel.prompt_files import DEFAULT_PREAMBLE_BODY

        body = str(DEFAULT_PREAMBLE_BODY or "")
        self.assertNotIn("Todo loop (runtime-first):", body)
        self.assertNotIn("Completion gate: no full-done summary", body)
        self.assertNotIn("For non-trivial plans, run a 6D check", body)

    def test_builtin_help_is_compact(self) -> None:
        from cccc.kernel.prompt_files import load_builtin_help_markdown

        body = str(load_builtin_help_markdown() or "")
        self.assertLessEqual(len(body.split()), 1300)
        self.assertIn("This document is on-demand operational guidance.", body)


if __name__ == "__main__":
    unittest.main()
