from cccc.kernel.pty_terminal_state import derive_pty_terminal_override


def test_codex_prompt_visible_is_idle() -> None:
    assert derive_pty_terminal_override(runtime="codex", terminal_text="› Explain this codebase\n") == {
        "effective_working_state": "idle",
        "effective_working_reason": "pty_terminal_prompt_visible",
    }


def test_codex_model_footer_does_not_stand_in_for_prompt_visibility() -> None:
    terminal_text = """
› Explain this codebase

  gpt-5.5 medium · ~/Desktop/waterbang/ai/cccc
"""

    assert derive_pty_terminal_override(runtime="codex", terminal_text=terminal_text) is None


def test_claude_boxed_prompt_visible_is_idle() -> None:
    terminal_text = """
╭────────────────────────────────────────────────────────────╮
│ >                                                          │
╰────────────────────────────────────────────────────────────╯
  ? for shortcuts
"""

    assert derive_pty_terminal_override(runtime="claude", terminal_text=terminal_text) == {
        "effective_working_state": "idle",
        "effective_working_reason": "pty_terminal_claude_prompt_visible",
    }


def test_claude_interrupt_hint_is_working() -> None:
    terminal_text = """
✻ Thinking…
esc to interrupt
"""

    assert derive_pty_terminal_override(runtime="claude", terminal_text=terminal_text) == {
        "effective_working_state": "working",
        "effective_working_reason": "pty_terminal_claude_working_indicator",
    }


def test_claude_markdown_quote_does_not_stand_in_for_prompt_visibility() -> None:
    terminal_text = """
Assistant:
> quoted text from a document
"""

    assert derive_pty_terminal_override(runtime="claude", terminal_text=terminal_text) is None
