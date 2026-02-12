from __future__ import annotations

from typing import Dict, List, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field

from ...util.time import utc_now_iso


# ActorRole is now determined automatically by position in the actors list
# First enabled actor = foreman, rest = peer
# Kept for type hints and backward compatibility
ActorRole = Literal["foreman", "peer"]
ActorSubmit = Literal["enter", "newline", "none"]
RunnerKind = Literal["pty", "headless"]
AgentRuntime = Literal[
    "amp",
    "auggie",
    "claude",
    "codex",
    "cursor",
    "droid",
    "gemini",
    "kilocode",
    "neovate",
    "opencode",
    "copilot",
    "custom",
]

# Group state controls automation/runtime behavior.
# - active/idle/paused are logical workflow states
# - stopped means all runtimes are stopped
GroupState = Literal["active", "idle", "paused", "stopped"]


class Actor(BaseModel):
    v: int = 1
    id: str
    # role is now auto-determined by position, but kept for backward compat
    # First enabled actor in list = foreman, rest = peer
    role: Optional[ActorRole] = None  # Deprecated: ignored, auto-determined
    title: str = ""
    command: List[str] = Field(default_factory=list)
    env: Dict[str, str] = Field(default_factory=dict)
    default_scope_key: str = ""
    submit: ActorSubmit = "enter"
    enabled: bool = True
    runner: RunnerKind = "pty"  # "pty" for interactive, "headless" for MCP-driven
    runtime: AgentRuntime = "codex"  # Agent CLI runtime
    created_at: str = Field(default_factory=utc_now_iso)
    updated_at: str = Field(default_factory=utc_now_iso)

    model_config = ConfigDict(extra="ignore")  # Changed to ignore for backward compat


class HeadlessState(BaseModel):
    """Runtime state for a headless actor session."""
    v: int = 1
    group_id: str
    actor_id: str
    status: Literal["idle", "working", "waiting", "stopped"] = "idle"
    current_task_id: Optional[str] = None
    last_message_id: Optional[str] = None
    started_at: str = Field(default_factory=utc_now_iso)
    updated_at: str = Field(default_factory=utc_now_iso)

    model_config = ConfigDict(extra="forbid")
