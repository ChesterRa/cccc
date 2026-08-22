"""System notification contracts.

System notifications are separated from chat messages to avoid polluting user conversations.

Kinds:
- nudge: generic explicit actor nudge (not an Inbox acknowledgement mechanism)
- keepalive: remind an actor to continue work (after detecting a "Next:" declaration)
- help_nudge: remind an actor to refresh the collaboration protocol reference (cccc_help)
- actor_idle: actor idle alert (to foreman)
- silence_check: group silence alert (to foreman)
- auto_idle: group automatically transitioned to idle after repeated silence checks
- automation: user-defined automation rule notification
- status_change: actor/group status change
- error: system error
"""

from __future__ import annotations

from typing import Any, Dict, List, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field


NotifyKind = Literal[
    "nudge",  # Generic explicit actor nudge
    "keepalive",  # Remind to continue work
    "help_nudge",  # Ask actor to refresh cccc_help
    "actor_idle",  # Actor idle alert (to foreman)
    "silence_check",  # Group silence alert (to foreman)
    "auto_idle",  # Group auto-idled after repeated silence checks
    "automation",  # User-defined automation rule notification
    "status_change",  # Status change notification
    "error",  # Error notification
    "info",  # Informational notification
    "mail_notice",  # One-shot notice that Mail is waiting in the Inbox
    "reply_notice",  # One-shot notice that an accepted request still needs a reply
]

NotifyPriority = Literal["low", "normal", "high", "urgent"]
NotifyImVisibility = Literal["internal", "public"]


class SystemNotifyData(BaseModel):
    """System notification payload."""

    # Type
    kind: NotifyKind
    priority: NotifyPriority = "normal"

    # Content
    title: str = ""
    message: str = ""

    # Target
    target_actor_id: Optional[str] = None  # Target actor (None = broadcast)

    # External delivery. Internal is fail-closed for old and newly added producers.
    im_visibility: NotifyImVisibility = "internal"

    # Context
    context: Dict[str, Any] = Field(default_factory=dict)

    # Related event
    related_event_id: Optional[str] = None

    model_config = ConfigDict(extra="forbid")
