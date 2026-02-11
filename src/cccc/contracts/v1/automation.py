"""Automation rule contracts (group-scoped).

Rules are evaluated only while group state == active and do not execute arbitrary code.
Action kinds are declarative and validated by contract:
- notify: send system.notify to selected recipients
- group_state: set group state (or stop group runtime)
- actor_control: start/stop/restart selected actors
"""

from __future__ import annotations

from typing import Annotated, Dict, List, Literal, Optional, Union

from pydantic import BaseModel, ConfigDict, Field

from .notify import NotifyPriority


class AutomationTriggerInterval(BaseModel):
    kind: Literal["interval"] = "interval"
    every_seconds: int = Field(ge=1, description="Minimum seconds between firings (per group).")

    model_config = ConfigDict(extra="forbid")


class AutomationTriggerCron(BaseModel):
    kind: Literal["cron"] = "cron"
    cron: str = Field(description="Cron expression (5-field format: min hour dom month dow).")
    timezone: str = Field(default="UTC", description="IANA timezone name (e.g. Asia/Shanghai).")

    model_config = ConfigDict(extra="forbid")


class AutomationTriggerAt(BaseModel):
    kind: Literal["at"] = "at"
    at: str = Field(description="One-shot RFC3339 timestamp.")

    model_config = ConfigDict(extra="forbid")


AutomationTrigger = Annotated[
    Union[AutomationTriggerInterval, AutomationTriggerCron, AutomationTriggerAt],
    Field(discriminator="kind"),
]


class AutomationActionNotify(BaseModel):
    kind: Literal["notify"] = "notify"

    # Content: prefer snippet_ref for reuse; message is a fallback/override.
    title: str = ""
    snippet_ref: Optional[str] = None
    message: str = ""
    priority: NotifyPriority = "normal"
    requires_ack: bool = False

    model_config = ConfigDict(extra="forbid")


class AutomationActionGroupState(BaseModel):
    kind: Literal["group_state"] = "group_state"
    state: Literal["active", "idle", "paused", "stopped"] = "paused"

    model_config = ConfigDict(extra="forbid")


class AutomationActionActorControl(BaseModel):
    kind: Literal["actor_control"] = "actor_control"
    operation: Literal["start", "stop", "restart"] = "restart"
    targets: List[str] = Field(default_factory=lambda: ["@all"])

    model_config = ConfigDict(extra="forbid")


AutomationAction = Annotated[
    Union[AutomationActionNotify, AutomationActionGroupState, AutomationActionActorControl],
    Field(discriminator="kind"),
]


class AutomationRule(BaseModel):
    id: str
    enabled: bool = True
    scope: Literal["group", "personal"] = "group"
    owner_actor_id: Optional[str] = None
    to: List[str] = Field(default_factory=lambda: ["@foreman"])
    trigger: AutomationTrigger = Field(default_factory=AutomationTriggerInterval)
    action: AutomationAction = Field(default_factory=AutomationActionNotify)

    model_config = ConfigDict(extra="forbid")


class AutomationRuleSet(BaseModel):
    """Automation rules + snippet library for a group."""

    rules: List[AutomationRule] = Field(default_factory=list)
    snippets: Dict[str, str] = Field(default_factory=dict)

    model_config = ConfigDict(extra="forbid")
