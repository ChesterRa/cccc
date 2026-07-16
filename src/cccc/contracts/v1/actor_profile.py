from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, List, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field, model_validator
from ...util.time import utc_now_iso
from .actor import ActorSubmit, AgentRuntime, RunnerKind

class CapabilityDefaults(BaseModel):
    autoload_capabilities: List[str] = Field(default_factory=list)
    default_scope: Literal["actor", "session"] = "actor"
    session_ttl_seconds: int = 3600

    model_config = ConfigDict(extra="forbid")


@dataclass(frozen=True)
class ActorProfileRef:
    profile_id: str
    profile_scope: Literal["global", "user"] = "global"
    profile_owner: str = ""


class ActorProfile(BaseModel):
    """Reusable actor runtime configuration."""

    v: int = 1
    id: str
    name: str = ""
    scope: Literal["global", "user"] = "global"
    owner_id: str = ""
    runtime: AgentRuntime = "codex"
    runner: RunnerKind = "pty"
    command: List[str] = Field(default_factory=list)
    submit: ActorSubmit = "enter"
    env: Dict[str, str] = Field(default_factory=dict)
    created_at: str = Field(default_factory=utc_now_iso)
    updated_at: str = Field(default_factory=utc_now_iso)
    revision: int = 1
    capability_defaults: Optional[CapabilityDefaults] = None

    model_config = ConfigDict(extra="forbid")

    @model_validator(mode="after")
    def _validate_scope_owner(self) -> "ActorProfile":
        if self.runtime == "web_model":
            self.runner = "headless"
            self.command = []
        if self.scope == "global":
            self.owner_id = ""
            return self
        if not str(self.owner_id or "").strip():
            raise ValueError("user scope profile requires owner_id")
        self.owner_id = str(self.owner_id).strip()
        return self
