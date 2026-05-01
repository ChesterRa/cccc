from __future__ import annotations

import unittest


class TestPromptDefaults(unittest.TestCase):
    def test_default_preamble_is_compact_and_actionable(self) -> None:
        from cccc.kernel.prompt_files import DEFAULT_PREAMBLE_BODY

        body = str(DEFAULT_PREAMBLE_BODY or "")
        self.assertIn("Startup routes:", body)
        self.assertIn("Execution default:", body)
        self.assertIn("cccc_bootstrap", body)
        self.assertIn("context_hygiene", body)
        self.assertIn("cccc_help", body)
        self.assertIn("cccc_context_get", body)
        self.assertIn("cccc_project_info", body)
        self.assertIn("Reuse working paths first.", body)
        self.assertNotIn("Working stance:", body)
        self.assertNotIn("Work like a teammate, not a script.", body)
        self.assertNotIn("Prefer silence over low-signal chatter", body)
        self.assertNotIn("routine `@all` updates", body)
        self.assertNotIn("finish it end-to-end", body)
        self.assertNotIn("intent is not progress", body)
        self.assertLessEqual(len(body.split()), 90)

    def test_default_preamble_avoids_long_rule_duplication(self) -> None:
        from cccc.kernel.prompt_files import DEFAULT_PREAMBLE_BODY

        body = str(DEFAULT_PREAMBLE_BODY or "")
        self.assertNotIn("Execution checklist:", body)
        self.assertNotIn("Gap routing:", body)
        self.assertNotIn("Memory boundary:", body)
        self.assertNotIn("cccc_capability_search", body)
        self.assertNotIn("cccc_agent_state(action=update", body)

    def test_builtin_help_is_compact(self) -> None:
        from cccc.kernel.prompt_files import load_builtin_help_markdown

        body = str(load_builtin_help_markdown() or "")
        common_body = body.split("\n## Role Notes\n", 1)[0]
        self.assertLessEqual(len(common_body.split()), 1700)
        self.assertIn("This is your working playbook for this group.", body)
        self.assertIn("## Working Stance", body)
        self.assertIn("## Communication Patterns", body)
        self.assertIn("## Core Routes", body)
        self.assertIn("## Control Plane", body)
        self.assertIn("## Memory and Recall", body)
        self.assertIn("## Capability", body)
        self.assertIn("search capability before inventing a new workflow", body)
        self.assertIn('`enable_hint="enable_now"`', body)
        self.assertIn("### Skill Evolution Proposals", body)
        self.assertIn("## Role Notes", body)
        self.assertIn("## Appendix", body)
        self.assertIn("present the post-review version, not the first draft", body)
        self.assertIn("Prefer silence over low-signal chatter.", body)
        self.assertIn("This user is not generic. Learn their bar and dislikes; let that shape your defaults.", body)
        self.assertIn('"standing by"', body)
        self.assertIn('"received"', body)
        self.assertIn("routine status, acknowledgements", body)
        self.assertIn("Do not drip-feed obvious in-scope next steps", body)
        self.assertIn('if nothing changed, stay silent, not "received" or "standing by"', body)
        self.assertNotIn("## Quick Card", body)
        self.assertNotIn("## Where Things Live", body)
        self.assertNotIn("### NotebookLM Work vs Memory Lane", body)
        self.assertNotIn("### NotebookLM Artifact Runs", body)
        self.assertNotIn("### Capsule Skill Boundary", body)
        self.assertNotIn("### Terminal Transcript", body)
        self.assertNotIn("### Automation Tools", body)
        self.assertNotIn("## Quick Card", body)
        self.assertIn("Treat `done`, `idle`, and silence as evaluation signals, not closure truth.", body)
        self.assertIn("Protect verifier boundaries unless changing the verifier is explicitly in scope.", body)
        self.assertIn("`source_id=agent_self_proposed`", body)
        self.assertIn("`skill:agent_self_proposed:<stable-slug>`", body)
        self.assertIn("`cccc_capability_state.active_capsule_skills`", body)
        self.assertIn("Direct import works for low-risk proposals", body)
        self.assertIn("invalid real imports preserve the last active version", body)
        self.assertIn('Use `scope="session"` for one-off trials', body)
        self.assertIn("reuse the existing `capability_id` with revised `capsule_text`", body)
        self.assertIn("do not create a near-duplicate or silently delete it", body)
        self.assertIn("`qualification_status=blocked`", body)
        self.assertNotIn("manual_only", body)

    def test_mcp_reminder_line_stays_single_purpose(self) -> None:
        from cccc.daemon.messaging.delivery import MCP_REMINDER_LINE

        self.assertIn("use MCP", MCP_REMINDER_LINE)
        self.assertIn("terminal output isn't delivered.", MCP_REMINDER_LINE)
        self.assertIn("Verify reply_to/to", MCP_REMINDER_LINE)
        self.assertIn("avoid routine @all", MCP_REMINDER_LINE)
        self.assertIn("communication obligation, not the whole job", MCP_REMINDER_LINE)
        self.assertIn("resume active work unless priority changed", MCP_REMINDER_LINE)
        self.assertIn("use MCP tool cccc_help", MCP_REMINDER_LINE)
        self.assertNotIn("Help: cccc_help", MCP_REMINDER_LINE)

    def test_default_standup_stays_short_ritual(self) -> None:
        from cccc.kernel.group import _DEFAULT_AUTOMATION_STANDUP_SNIPPET

        body = str(_DEFAULT_AUTOMATION_STANDUP_SNIPPET or "")
        self.assertIn("Keep this short.", body)
        self.assertIn("current status, next step, blocker", body)
        self.assertIn("not a task switch", body)
        self.assertIn("Do not answer from fuzzy memory.", body)
        self.assertIn("grounded in fresh context", body)
        self.assertIn("`cccc_bootstrap`", body)
        self.assertIn("`memory_recall_gate`", body)
        self.assertIn("before replying", body)
        self.assertIn("return to your prior active task", body)
        self.assertIn("cccc_help", body)
        self.assertNotIn("Recall:", body)
        self.assertNotIn("cccc_capability_use(...)", body)
        self.assertNotIn("diagnostics", body)

    def test_builtin_help_marks_coordination_interrupts_as_non_switches(self) -> None:
        from cccc.kernel.prompt_files import load_builtin_help_markdown

        body = str(load_builtin_help_markdown() or "")
        self.assertIn("`standup` and `help_nudge` are coordination interrupts, not task switches", body)
        self.assertIn("Do not overwrite `active_task_id`, `focus`, or `next_action`", body)


if __name__ == "__main__":
    unittest.main()
