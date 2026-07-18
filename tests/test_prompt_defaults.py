from __future__ import annotations

import unittest


class TestPromptDefaults(unittest.TestCase):
    def test_default_preamble_is_compact_and_actionable(self) -> None:
        from cccc.kernel.prompt_files import DEFAULT_PREAMBLE_BODY

        body = str(DEFAULT_PREAMBLE_BODY or "")
        self.assertIn("Startup:", body)
        self.assertIn("cccc_bootstrap", body)
        self.assertIn("cccc_help", body)
        self.assertIn("only when", body)
        self.assertNotIn("context_hygiene", body)
        self.assertNotIn("memory_recall_gate", body)
        self.assertNotIn("cccc_context_get", body)
        self.assertNotIn("cccc_project_info", body)
        self.assertNotIn("Execution default:", body)
        self.assertLessEqual(len(body.split()), 30)

    def test_default_preamble_avoids_long_rule_duplication(self) -> None:
        from cccc.kernel.prompt_files import DEFAULT_PREAMBLE_BODY

        body = str(DEFAULT_PREAMBLE_BODY or "")
        self.assertNotIn("Execution checklist:", body)
        self.assertNotIn("Gap routing:", body)
        self.assertNotIn("Memory boundary:", body)
        self.assertNotIn("cccc_capability_search", body)
        self.assertNotIn("cccc_agent_state(action=update", body)

    def test_builtin_help_is_compact_and_protocol_complete(self) -> None:
        from cccc.kernel.prompt_files import load_builtin_help_markdown

        body = str(load_builtin_help_markdown() or "")
        common_body = body.split("\n## Actor Notes\n", 1)[0]
        self.assertLessEqual(len(common_body.split()), 650)
        self.assertIn("CCCC routes and shared-state reference", body)
        self.assertIn("## Core Routes", body)
        self.assertIn("## Collaboration State", body)
        self.assertIn("### State Layers", body)
        self.assertIn("### Durable Coordination", body)
        self.assertIn("### Recovery and Recall", body)
        self.assertIn("## Capabilities", body)
        self.assertIn("## Actor Notes", body)
        self.assertIn("## Appendix", body)
        self.assertIn("daemon and append-only group ledger are the source of truth", body)
        self.assertIn("`cccc_agent_state` is per-actor recovery state, not chat status", body)
        self.assertIn("keep runtime-local todo private", body)
        self.assertIn("not automatically a task switch", body)
        self.assertIn('tool_name="cccc_project_info"', body)
        self.assertIn('tool_name="cccc_memory"', body)
        self.assertIn("invokes hidden tools without exposing the full pack", body)
        self.assertIn("including the peer collaboration contract", body)
        self.assertNotIn("## CCCC Creed", body)
        self.assertNotIn("## Working Stance", body)
        self.assertNotIn("## Communication Patterns", body)
        self.assertNotIn("## Memory and Recall", body)
        self.assertNotIn("### Todo and Scope Discipline", body)
        self.assertNotIn("### Planning and Scope Gates", body)

    def test_mcp_reminder_line_stays_single_purpose(self) -> None:
        from cccc.daemon.messaging.delivery import MCP_REMINDER_LINE

        self.assertIn("Use cccc_message_reply for replies", MCP_REMINDER_LINE)
        self.assertIn("use cccc_message_send for new messages", MCP_REMINDER_LINE)
        self.assertIn("Terminal output is not delivered.", MCP_REMINDER_LINE)
        self.assertIn("Verify reply_to/to", MCP_REMINDER_LINE)
        self.assertIn("avoid routine @all", MCP_REMINDER_LINE)
        self.assertIn("Use cccc_help if unsure", MCP_REMINDER_LINE)
        self.assertNotIn("Reply via MCP", MCP_REMINDER_LINE)
        self.assertNotIn("cccc_message_send / cccc_message_reply", MCP_REMINDER_LINE)
        self.assertNotIn("not the job", MCP_REMINDER_LINE)
        self.assertNotIn("resume active work", MCP_REMINDER_LINE)
        self.assertNotIn("open loops", MCP_REMINDER_LINE)
        self.assertNotIn("highest-value", MCP_REMINDER_LINE)
        self.assertNotIn("Serve the real objective", MCP_REMINDER_LINE)
        self.assertNotIn("natural #group requests", MCP_REMINDER_LINE)
        self.assertNotIn('cccc_group(action="resolve"', MCP_REMINDER_LINE)
        self.assertNotIn("dst_group_id", MCP_REMINDER_LINE)
        self.assertNotIn("#group", MCP_REMINDER_LINE)
        self.assertIn("to", MCP_REMINDER_LINE)
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

    def test_builtin_help_scopes_interrupt_recovery_to_durable_state(self) -> None:
        from cccc.kernel.prompt_files import load_builtin_help_markdown

        body = str(load_builtin_help_markdown() or "")
        self.assertIn("coordination interrupt is not automatically a task switch", body)
        self.assertIn("resume the recorded task unless priority actually changed", body)
        self.assertIn("do not replace active state with the interrupt itself", body)


class TestForemanRoleHelpSection(unittest.TestCase):
    def test_builtin_help_foreman_section_stays_protocol_focused(self) -> None:
        from cccc.kernel.prompt_files import load_builtin_help_markdown
        from cccc.ports.mcp.utils.help_markdown import parse_help_markdown

        parsed = parse_help_markdown(str(load_builtin_help_markdown() or ""))
        foreman = str(parsed.get("foreman") or "")
        self.assertIn("Own integration and acceptance", foreman)
        self.assertIn("durable tasks or tracked sends", foreman)
        self.assertNotIn("repeated failures", foreman)
        self.assertNotIn("question the objective", foreman)

    def test_builtin_help_does_not_inject_actor_notes_doctrine(self) -> None:
        from cccc.kernel.prompt_files import load_builtin_help_markdown
        from cccc.ports.mcp.utils.help_markdown import parse_help_markdown

        parsed = parse_help_markdown(str(load_builtin_help_markdown() or ""))
        foreman = str(parsed.get("foreman") or "")
        self.assertNotIn("repeatedly observed collaboration preferences", foreman)
        self.assertNotIn("never record one-off mistakes", foreman)

    def test_builtin_help_chat_section_does_not_duplicate_core_routes_reply_line(self) -> None:
        from cccc.kernel.prompt_files import load_builtin_help_markdown

        body = str(load_builtin_help_markdown() or "")
        self.assertNotIn("for new visible coordination messages", body)
        self.assertEqual(body.count("Reply with `cccc_message_reply`"), 1)

    def test_role_system_prompt_stays_role_agnostic(self) -> None:
        import inspect

        from cccc.kernel import system_prompt

        source = inspect.getsource(system_prompt.render_role_system_prompt)
        self.assertNotIn("Foreman lens", source)
        self.assertNotIn("repeated failures", source)


if __name__ == "__main__":
    unittest.main()
