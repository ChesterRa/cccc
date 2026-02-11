from __future__ import annotations

from typing import List, Literal, Optional, Union

from pydantic import BaseModel, ConfigDict, Field

from .actor import ActorSubmit, AgentRuntime, RunnerKind
from .automation import AutomationRuleSet


TerminalTranscriptVisibility = Literal["off", "foreman", "all"]
DefaultSendTo = Literal["foreman", "broadcast"]


class GroupTemplateActor(BaseModel):
    """Portable actor config for group templates.

    Notes:
    - actor_id is the identity key (stable across imports).
    - env is intentionally excluded (templates never carry environment secrets).
    """

    actor_id: str = Field(alias="id")
    title: str = ""
    runtime: AgentRuntime = "codex"
    runner: RunnerKind = "pty"
    command: Union[str, List[str]] = Field(default_factory=list)
    submit: ActorSubmit = "enter"
    enabled: bool = True

    model_config = ConfigDict(extra="ignore", populate_by_name=True)


class GroupTemplateSettings(BaseModel):
    """Group-scoped settings that are safe to carry across projects."""

    default_send_to: DefaultSendTo = "foreman"
    nudge_after_seconds: int = 300
    reply_required_nudge_after_seconds: int = 300
    attention_ack_nudge_after_seconds: int = 600
    unread_nudge_after_seconds: int = 900
    nudge_digest_min_interval_seconds: int = 120
    nudge_max_repeats_per_obligation: int = 3
    nudge_escalate_after_repeats: int = 2
    auto_mark_on_delivery: bool = False
    actor_idle_timeout_seconds: int = 600
    keepalive_delay_seconds: int = 120
    keepalive_max_per_actor: int = 3
    silence_timeout_seconds: int = 600
    help_nudge_interval_seconds: int = 600
    help_nudge_min_messages: int = 10
    min_interval_seconds: int = 0

    terminal_transcript_visibility: TerminalTranscriptVisibility = "foreman"
    terminal_transcript_notify_tail: bool = True
    terminal_transcript_notify_lines: int = 20

    model_config = ConfigDict(extra="ignore")


class GroupTemplatePrompts(BaseModel):
    """Optional prompt bodies embedded directly in the template file.

    When a prompt field is omitted / null, the template uses the built-in
    default and import-replace will reset (delete) the corresponding group
    prompt override file under CCCC_HOME.
    """

    preamble: Optional[str] = None
    help: Optional[str] = None

    model_config = ConfigDict(extra="ignore")


class GroupTemplate(BaseModel):
    """A single-file, portable group configuration template (v1)."""

    kind: Literal["cccc.group_template"] = "cccc.group_template"
    v: int = 1

    # Informational only (not applied on import-replace).
    title: str = ""
    topic: str = ""
    exported_at: str = ""
    cccc_version: str = ""

    actors: List[GroupTemplateActor] = Field(default_factory=list)
    settings: GroupTemplateSettings = Field(default_factory=GroupTemplateSettings)
    prompts: GroupTemplatePrompts = Field(default_factory=GroupTemplatePrompts)
    automation: AutomationRuleSet = Field(default_factory=AutomationRuleSet)

    model_config = ConfigDict(extra="ignore")
