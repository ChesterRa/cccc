from __future__ import annotations

from typing import Any, Dict, List, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field


AssistantKind = Literal["pet", "voice_secretary"]
AssistantLifecycle = Literal["disabled", "idle", "running", "working", "waiting", "failed"]


class AssistantPolicy(BaseModel):
    action_allowlist: List[str] = Field(default_factory=list)
    requires_user_confirmation: List[str] = Field(default_factory=list)

    model_config = ConfigDict(extra="forbid")


class BuiltinAssistant(BaseModel):
    assistant_id: str
    kind: str
    enabled: bool = False
    principal: str = ""
    lifecycle: str = "disabled"
    health: Dict[str, Any] = Field(default_factory=dict)
    policy: AssistantPolicy = Field(default_factory=AssistantPolicy)
    config: Dict[str, Any] = Field(default_factory=dict)
    ui: Dict[str, Any] = Field(default_factory=dict)

    model_config = ConfigDict(extra="forbid")


class AssistantSettingsUpdateData(BaseModel):
    assistant_id: str
    enabled: Optional[bool] = None
    config_keys: List[str] = Field(default_factory=list)

    model_config = ConfigDict(extra="forbid")


class AssistantStatusUpdateData(BaseModel):
    assistant_id: str
    lifecycle: AssistantLifecycle
    health_keys: List[str] = Field(default_factory=list)

    model_config = ConfigDict(extra="forbid")


class AssistantVoiceDocumentData(BaseModel):
    assistant_id: str
    document_path: str = ""
    action: str
    input_kind: str = ""
    request_id: str = ""
    status: str = "active"
    workspace_path: str = ""
    title: str = ""
    input_preview: str = ""

    model_config = ConfigDict(extra="forbid")


class AssistantVoiceRequestData(BaseModel):
    assistant_id: str
    request_id: str
    target_actor_id: str
    action: str = "handoff"
    status: str = ""
    source_request_id: str = ""
    document_path: str = ""
    artifact_paths: List[str] = Field(default_factory=list)
    source_summary: str = ""
    checked_at: str = ""
    source_urls: List[str] = Field(default_factory=list)
    source_event_id: str = ""
    request_preview: str = ""
    reply_text: str = ""
    notify_event_id: str = ""

    model_config = ConfigDict(extra="forbid")


class AssistantVoiceSessionData(BaseModel):
    assistant_id: str
    session_id: str
    action: str
    status: str = ""
    artifact_path: str = ""
    error_code: str = ""
    error_message: str = ""

    model_config = ConfigDict(extra="forbid")


class AssistantVoiceInputData(BaseModel):
    assistant_id: str
    input_kind: str
    target_kind: str = ""
    request_id: str = ""
    document_path: str = ""
    input_preview: str = ""

    model_config = ConfigDict(extra="forbid")


class AssistantVoicePromptDraftData(BaseModel):
    assistant_id: str
    request_id: str
    action: str
    status: str = ""
    draft_preview: str = ""

    model_config = ConfigDict(extra="forbid")
