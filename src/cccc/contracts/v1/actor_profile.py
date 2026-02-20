from __future__ import annotations

from typing import Dict, List

from pydantic import BaseModel, ConfigDict, Field

from ...util.time import utc_now_iso
from .actor import ActorSubmit, AgentRuntime, RunnerKind


class ActorProfile(BaseModel):
    """Reusable actor runtime configuration (global asset)."""

    v: int = 1
    id: str
    name: str = ""
    runtime: AgentRuntime = "codex"
    runner: RunnerKind = "pty"
    command: List[str] = Field(default_factory=list)
    submit: ActorSubmit = "enter"
    env: Dict[str, str] = Field(default_factory=dict)
    created_at: str = Field(default_factory=utc_now_iso)
    updated_at: str = Field(default_factory=utc_now_iso)
    revision: int = 1

    model_config = ConfigDict(extra="forbid")
