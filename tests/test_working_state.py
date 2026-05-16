from cccc.kernel.pty_terminal_state import derive_pty_terminal_override
from cccc.kernel.working_state import derive_effective_working_state


def test_codex_terminal_text_is_not_used_for_working_state() -> None:
    terminal_text = """
› Run /review on my current changes
◦ Working (6s • esc to interrupt)
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


def test_app_server_headless_idle_ignores_pty_terminal_override() -> None:
    state = derive_effective_working_state(
        running=True,
        effective_runner="headless",
        runtime="codex",
        headless_state={"status": "idle", "updated_at": "2026-05-14T00:00:00Z"},
        pty_terminal_override={
            "effective_working_state": "working",
            "effective_working_reason": "ignored_terminal_override",
        },
    )

    assert state["effective_working_state"] == "idle"
    assert state["effective_working_reason"] == "headless_idle"
    assert state["effective_working_updated_at"] == "2026-05-14T00:00:00Z"


def test_app_server_headless_working_wins_over_terminal_idle() -> None:
    state = derive_effective_working_state(
        running=True,
        effective_runner="headless",
        runtime="codex",
        headless_state={"status": "working", "current_task_id": "turn-1"},
        pty_terminal_override={
            "effective_working_state": "idle",
            "effective_working_reason": "ignored_terminal_override",
        },
    )

    assert state["effective_working_state"] == "working"
    assert state["effective_working_reason"] == "headless_working"
    assert state["effective_active_task_id"] == "turn-1"
