from __future__ import annotations

import uuid
from typing import Any, Dict, List, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field

from ...util.time import utc_now_iso
from .actor import Actor, ActorRole, ActorSubmit, AgentRuntime, RunnerKind, RuntimeStateSource
from .assistant import (
    AssistantSettingsUpdateData,
    AssistantStatusUpdateData,
    AssistantVoiceDocumentData,
    AssistantVoiceInputData,
    AssistantVoicePromptDraftData,
    AssistantVoiceRequestData,
    AssistantVoiceSessionData,
)
from .message import ChatMessageData, ChatReactionData, ChatStreamData
from .notify import SystemNotifyData
from .presentation import PresentationCardType


EventKind = Literal[
    "group.create",
    "group.update",
    "group.attach",
    "group.detach_scope",
    "group.set_active_scope",
    "group.start",
    "group.stop",
    "group.set_state",
    "group.settings_update",
    "group.automation_update",
    "actor.add",
    "actor.update",
    "actor.set_role",
    "actor.start",
    "actor.stop",
    "actor.restart",
    "actor.new_session",
    "actor.remove",
    "actor.activity",
    "context.sync",
    "chat.message",
    "chat.stream",
    "mail.read",
    "chat.reaction",
    "chat.reply_request.cancelled",
    "chat.cross_group_receipt",
    "runtime.delivery",
    "system.notify",
    "assistant.settings_update",
    "assistant.status_update",
    "assistant.voice.document",
    "assistant.voice.input",
    "assistant.voice.prompt_draft",
    "assistant.voice.request",
    "assistant.voice.session",
    "presentation.publish",
    "presentation.clear",
]


class GroupCreateData(BaseModel):
    title: str
    topic: str = ""

    model_config = ConfigDict(extra="forbid")


class GroupUpdatePatch(BaseModel):
    title: Optional[str] = None
    topic: Optional[str] = None

    model_config = ConfigDict(extra="forbid")


class GroupUpdateData(BaseModel):
    patch: GroupUpdatePatch

    model_config = ConfigDict(extra="forbid")


class GroupAttachData(BaseModel):
    url: str
    label: str = ""
    git_remote: str = ""

    model_config = ConfigDict(extra="forbid")


class GroupDetachScopeData(BaseModel):
    scope_key: str

    model_config = ConfigDict(extra="forbid")


class GroupSetActiveScopeData(BaseModel):
    path: str

    model_config = ConfigDict(extra="forbid")


class GroupStartData(BaseModel):
    started: List[str] = Field(default_factory=list)

    model_config = ConfigDict(extra="forbid")


class GroupStopData(BaseModel):
    stopped: List[str] = Field(default_factory=list)

    model_config = ConfigDict(extra="forbid")


class GroupSetStateData(BaseModel):
    old_state: str = ""
    new_state: str = ""

    model_config = ConfigDict(extra="forbid")


class GroupSettingsUpdateData(BaseModel):
    patch: Dict[str, Any] = Field(default_factory=dict)

    model_config = ConfigDict(extra="forbid")


class GroupAutomationUpdateData(BaseModel):
    rules: List[str] = Field(default_factory=list)
    snippets: List[str] = Field(default_factory=list)
    version: Optional[int] = None
    actions: List[Dict[str, Any]] = Field(default_factory=list)
    source: str = ""

    model_config = ConfigDict(extra="forbid")


class ActorAddData(BaseModel):
    actor: Actor

    model_config = ConfigDict(extra="forbid")


class ActorUpdatePatch(BaseModel):
    role: Optional[ActorRole] = None
    title: Optional[str] = None
    avatar_asset_path: Optional[str] = None
    command: Optional[List[str]] = None
    env: Optional[Dict[str, str]] = None
    default_scope_key: Optional[str] = None
    submit: Optional[ActorSubmit] = None
    capability_autoload: Optional[List[str]] = None
    capability_hidden: Optional[List[str]] = None
    enabled: Optional[bool] = None
    runner: Optional[RunnerKind] = None
    runtime: Optional[AgentRuntime] = None
    runtime_state_source: Optional[RuntimeStateSource] = None

    model_config = ConfigDict(extra="forbid")


class ActorUpdateData(BaseModel):
    actor_id: str
    patch: ActorUpdatePatch
    profile_id: Optional[str] = None
    profile_scope: Optional[str] = None
    profile_owner: Optional[str] = None
    profile_action: Optional[Literal["convert_to_custom"]] = None

    model_config = ConfigDict(extra="forbid")


class ActorSetRoleData(BaseModel):
    actor_id: str
    role: ActorRole

    model_config = ConfigDict(extra="forbid")


class ActorLifecycleData(BaseModel):
    actor_id: str
    runner: Optional[str] = None  # pty or headless
    # Effective runner used at runtime (e.g., PTY → headless fallback).
    runner_effective: Optional[str] = None

    model_config = ConfigDict(extra="forbid")


class ActorNewSessionData(ActorLifecycleData):
    runtime: Optional[AgentRuntime] = None
    rotation: Optional[Literal["in_place"]] = None


class ActorActivityData(BaseModel):
    """Periodic runtime status snapshot for running actors."""
    actors: List[Dict[str, Any]] = Field(default_factory=list)

    model_config = ConfigDict(extra="allow")


class ContextSyncData(BaseModel):
    version: str = ""
    changes: List[Dict[str, Any]] = Field(default_factory=list)

    model_config = ConfigDict(extra="forbid")


class MailReadData(BaseModel):
    """Mail receipt: an actor consumes Mail up to a given event."""

    actor_id: str  # Actor who consumed the Mail
    event_id: str  # The last consumed Mail event_id (inclusive)

    model_config = ConfigDict(extra="forbid")


class ChatReplyRequestCancelledData(BaseModel):
    """Cancellation of outstanding reply obligations for one source message."""

    source_event_id: str
    src_group_id: str = ""
    src_event_id: str = ""
    src_message_event_id: str = ""

    model_config = ConfigDict(extra="forbid")


class RuntimeDeliveryData(BaseModel):
    """Daemon-authored handoff evidence for one message recipient."""

    actor_id: str
    source_event_id: str
    delivery_id: str
    state: Literal["claimed", "accepted", "failed", "ambiguous"]
    transport: str
    reason: Optional[str] = None

    model_config = ConfigDict(extra="forbid")


class ChatCrossGroupReceiptData(BaseModel):
    """Append-only link from a local source event to its delivered target event."""

    source_event_id: str
    operation: Literal["remote_send", "reply_request_cancel"] = "remote_send"
    dst_group_id: str
    dst_event_id: str = ""
    remote_event_id: str = ""
    registration_id: str = ""
    idempotency_key: str = ""
    status: str = ""

    model_config = ConfigDict(extra="forbid")


class PresentationPublishData(BaseModel):
    slot_id: str
    title: str
    card_type: PresentationCardType
    source_label: str = ""
    source_ref: str = ""
    summary: str = ""

    model_config = ConfigDict(extra="forbid")


class PresentationClearData(BaseModel):
    slot_id: str = ""
    cleared_all: bool = False
    cleared_slots: List[str] = Field(default_factory=list)

    model_config = ConfigDict(extra="forbid")


class Event(BaseModel):
    v: int = 1
    id: str = Field(default_factory=lambda: uuid.uuid4().hex)
    ts: str = Field(default_factory=utc_now_iso)
    kind: str
    group_id: str
    scope_key: str = ""
    by: str = ""
    data: Dict[str, Any] = Field(default_factory=dict)

    model_config = ConfigDict(extra="forbid")


_KIND_TO_MODEL = {
    "group.create": GroupCreateData,
    "group.update": GroupUpdateData,
    "group.attach": GroupAttachData,
    "group.detach_scope": GroupDetachScopeData,
    "group.set_active_scope": GroupSetActiveScopeData,
    "group.start": GroupStartData,
    "group.stop": GroupStopData,
    "group.set_state": GroupSetStateData,
    "group.settings_update": GroupSettingsUpdateData,
    "group.automation_update": GroupAutomationUpdateData,
    "actor.add": ActorAddData,
    "actor.update": ActorUpdateData,
    "actor.set_role": ActorSetRoleData,
    "actor.start": ActorLifecycleData,
    "actor.stop": ActorLifecycleData,
    "actor.restart": ActorLifecycleData,
    "actor.new_session": ActorNewSessionData,
    "actor.remove": ActorLifecycleData,
    "actor.activity": ActorActivityData,
    "context.sync": ContextSyncData,
    "chat.message": ChatMessageData,
    "chat.stream": ChatStreamData,
    "mail.read": MailReadData,
    "chat.reaction": ChatReactionData,
    "chat.reply_request.cancelled": ChatReplyRequestCancelledData,
    "chat.cross_group_receipt": ChatCrossGroupReceiptData,
    "runtime.delivery": RuntimeDeliveryData,
    "system.notify": SystemNotifyData,
    "assistant.settings_update": AssistantSettingsUpdateData,
    "assistant.status_update": AssistantStatusUpdateData,
    "assistant.voice.document": AssistantVoiceDocumentData,
    "assistant.voice.input": AssistantVoiceInputData,
    "assistant.voice.prompt_draft": AssistantVoicePromptDraftData,
    "assistant.voice.request": AssistantVoiceRequestData,
    "assistant.voice.session": AssistantVoiceSessionData,
    "presentation.publish": PresentationPublishData,
    "presentation.clear": PresentationClearData,
}


def normalize_event_data(kind: str, data: Any) -> Dict[str, Any]:
    if not isinstance(data, dict):
        data = {} if data is None else {"value": data}
    model = _KIND_TO_MODEL.get(str(kind))
    if model is None:
        # Unknown event kind: keep the envelope stable, keep data as a dict.
        return dict(data)
    parsed = model.model_validate(data)
    payload = parsed.model_dump()
    if kind == "group.update":
        patch = payload.get("patch") if isinstance(payload, dict) else None
        if isinstance(patch, dict) and not any(patch.get(k) is not None for k in ("title", "topic")):
            raise ValueError("group.update patch must include title and/or topic")
    if kind == "actor.update":
        patch = payload.get("patch") if isinstance(payload, dict) else None
        profile_id = str(payload.get("profile_id") or "").strip() if isinstance(payload, dict) else ""
        profile_action = str(payload.get("profile_action") or "").strip() if isinstance(payload, dict) else ""
        if isinstance(patch, dict) and not patch and not profile_id and not profile_action:
            raise ValueError("actor.update requires non-empty patch or profile action")
    return payload
