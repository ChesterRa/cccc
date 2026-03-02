from __future__ import annotations

import os
import shlex
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Awaitable, Callable, Dict, Literal, Optional, Union

from fastapi import HTTPException
from pydantic import BaseModel, ConfigDict, Field

from ...contracts.v1.actor import ActorSubmit, AgentRuntime, RunnerKind
from ...contracts.v1.automation import AutomationRule


def _default_runner_kind() -> str:
    try:
        from ...runners import pty as pty_runner

        return "pty" if bool(getattr(pty_runner, "PTY_SUPPORTED", True)) else "headless"
    except Exception:
        return "headless"


class CreateGroupRequest(BaseModel):
    title: str = Field(default="working-group")
    topic: str = Field(default="")
    by: str = Field(default="user")


class AttachRequest(BaseModel):
    path: str
    by: str = Field(default="user")


class SendRequest(BaseModel):
    text: str
    by: str = Field(default="user")
    to: list[str] = Field(default_factory=list)
    path: str = Field(default="")
    priority: Literal["normal", "attention"] = "normal"
    reply_required: bool = False
    src_group_id: str = Field(default="")
    src_event_id: str = Field(default="")


class SendCrossGroupRequest(BaseModel):
    text: str
    by: str = Field(default="user")
    dst_group_id: str
    to: list[str] = Field(default_factory=list)
    priority: Literal["normal", "attention"] = "normal"
    reply_required: bool = False


class ReplyRequest(BaseModel):
    text: str
    by: str = Field(default="user")
    to: list[str] = Field(default_factory=list)
    reply_to: str
    priority: Literal["normal", "attention"] = "normal"
    reply_required: bool = False


class DebugClearLogsRequest(BaseModel):
    component: str
    group_id: str = Field(default="")
    by: str = Field(default="user")


class GroupTemplatePreviewRequest(BaseModel):
    template: str = Field(default="")
    by: str = Field(default="user")


WEB_MAX_FILE_MB = 20
WEB_MAX_FILE_BYTES = WEB_MAX_FILE_MB * 1024 * 1024
WEB_MAX_TEMPLATE_BYTES = 2 * 1024 * 1024  # safety bound for template uploads


class ActorCreateRequest(BaseModel):
    actor_id: str
    # Note: role is auto-determined by position (first enabled = foreman)
    runner: RunnerKind = Field(default_factory=_default_runner_kind)
    runtime: AgentRuntime = Field(default="codex")
    title: str = Field(default="")
    command: Union[str, list[str]] = Field(default="")
    env: Dict[str, str] = Field(default_factory=dict)
    capability_autoload: list[str] = Field(default_factory=list)
    # Write-only runtime-only secrets (stored under CCCC_HOME/state; never persisted into ledger).
    # Values are never returned by the daemon; only keys can be listed via the dedicated endpoints.
    env_private: Optional[Dict[str, str]] = None
    profile_id: Optional[str] = None
    default_scope_key: str = Field(default="")
    submit: ActorSubmit = Field(default="enter")
    by: str = Field(default="user")


class ActorUpdateRequest(BaseModel):
    by: str = Field(default="user")
    # Note: role is ignored - auto-determined by position
    title: Optional[str] = None
    command: Optional[Union[str, list[str]]] = None
    env: Optional[Dict[str, str]] = None
    capability_autoload: Optional[list[str]] = None
    default_scope_key: Optional[str] = None
    submit: Optional[ActorSubmit] = None
    runner: Optional[RunnerKind] = None
    runtime: Optional[AgentRuntime] = None
    enabled: Optional[bool] = None
    profile_id: Optional[str] = None
    profile_action: Optional[Literal["convert_to_custom"]] = None


class ActorProfileUpsertRequest(BaseModel):
    profile: Dict[str, Any] = Field(default_factory=dict)
    expected_revision: Optional[int] = None
    by: str = Field(default="user")


class InboxReadRequest(BaseModel):
    event_id: str
    by: str = Field(default="user")


class UserAckRequest(BaseModel):
    by: str = Field(default="user")


class ProjectMdUpdateRequest(BaseModel):
    content: str = Field(default="")
    by: str = Field(default="user")


class RepoPromptUpdateRequest(BaseModel):
    content: str = Field(default="")
    by: str = Field(default="user")


class GroupUpdateRequest(BaseModel):
    title: Optional[str] = None
    topic: Optional[str] = None
    by: str = Field(default="user")


class GroupSettingsRequest(BaseModel):
    default_send_to: Optional[Literal["foreman", "broadcast"]] = None
    nudge_after_seconds: Optional[int] = None
    reply_required_nudge_after_seconds: Optional[int] = None
    attention_ack_nudge_after_seconds: Optional[int] = None
    unread_nudge_after_seconds: Optional[int] = None
    nudge_digest_min_interval_seconds: Optional[int] = None
    nudge_max_repeats_per_obligation: Optional[int] = None
    nudge_escalate_after_repeats: Optional[int] = None
    actor_idle_timeout_seconds: Optional[int] = None
    keepalive_delay_seconds: Optional[int] = None
    keepalive_max_per_actor: Optional[int] = None
    silence_timeout_seconds: Optional[int] = None
    help_nudge_interval_seconds: Optional[int] = None
    help_nudge_min_messages: Optional[int] = None
    min_interval_seconds: Optional[int] = None  # delivery throttle
    auto_mark_on_delivery: Optional[bool] = None  # auto-mark messages as read after delivery

    # Terminal transcript (group-scoped policy)
    terminal_transcript_visibility: Optional[Literal["off", "foreman", "all"]] = None
    terminal_transcript_notify_tail: Optional[bool] = None
    terminal_transcript_notify_lines: Optional[int] = None
    by: str = Field(default="user")


class GroupAutomationRequest(BaseModel):
    rules: list[AutomationRule] = Field(default_factory=list)
    snippets: Dict[str, str] = Field(default_factory=dict)
    expected_version: Optional[int] = None
    by: str = Field(default="user")


class GroupAutomationManageRequest(BaseModel):
    actions: list[Dict[str, Any]] = Field(default_factory=list)
    expected_version: Optional[int] = None
    by: str = Field(default="user")


class GroupAutomationResetBaselineRequest(BaseModel):
    expected_version: Optional[int] = None
    by: str = Field(default="user")


def _normalize_reply_required(v: Any) -> bool:
    """Normalize reply_required values from JSON/form payloads.

    Accepts bool/int/string values; defaults to False for unknown values.
    """
    if isinstance(v, bool):
        return v
    if isinstance(v, (int, float)):
        return bool(v)
    s = str(v or "").strip().lower()
    if s in ("1", "true", "yes", "on"):
        return True
    if s in ("0", "false", "no", "off", ""):
        return False
    return False


def _safe_int(value: Any, *, default: int, min_value: Optional[int] = None, max_value: Optional[int] = None) -> int:
    try:
        n = int(value)
    except Exception:
        n = default
    if min_value is not None and n < min_value:
        n = min_value
    if max_value is not None and n > max_value:
        n = max_value
    return n


class ObservabilityUpdateRequest(BaseModel):
    by: str = Field(default="user")
    developer_mode: Optional[bool] = None
    log_level: Optional[str] = None
    terminal_transcript_per_actor_bytes: Optional[int] = None
    terminal_ui_scrollback_lines: Optional[int] = None


class RegistryReconcileRequest(BaseModel):
    by: str = Field(default="user")
    remove_missing: bool = False


class RemoteAccessConfigureRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[Literal["off", "manual", "tailscale"]] = None
    mode: Optional[str] = None
    enforce_web_token: Optional[bool] = None
    web_host: Optional[str] = None
    web_port: Optional[int] = None
    web_public_url: Optional[str] = None
    web_token: Optional[str] = None
    clear_web_token: bool = False


class GroupSpaceBindRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    action: Literal["bind", "unbind"] = "bind"
    remote_space_id: str = Field(default="")


class GroupSpaceIngestRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    kind: Literal["context_sync", "resource_ingest"] = "context_sync"
    payload: Dict[str, Any] = Field(default_factory=dict)
    idempotency_key: str = Field(default="")


class GroupSpaceQueryRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")
    provider: Optional[str] = Field(default="notebooklm")
    query: str = Field(default="")
    options: Dict[str, Any] = Field(default_factory=dict)


class GroupSpaceSourceActionRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    action: Literal["delete", "rename", "refresh"]
    source_id: str = Field(default="")
    new_title: str = Field(default="")


class GroupSpaceArtifactActionRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    action: Literal["generate", "download"] = "generate"
    kind: str = Field(default="")
    options: Dict[str, Any] = Field(default_factory=dict)
    wait: bool = True
    save_to_space: bool = True
    output_path: str = Field(default="")
    output_format: str = Field(default="")
    artifact_id: str = Field(default="")
    timeout_seconds: float = 600.0
    initial_interval: float = 2.0
    max_interval: float = 10.0


class GroupSpaceJobActionRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    action: Literal["retry", "cancel"]
    job_id: str = Field(default="")


class GroupSpaceSyncRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    action: Literal["status", "run"] = "run"
    force: bool = False


class GroupSpaceProviderCredentialUpdateRequest(BaseModel):
    by: str = Field(default="user")
    auth_json: str = Field(default="")
    clear: bool = False


class GroupSpaceProviderAuthRequest(BaseModel):
    by: str = Field(default="user")
    action: Literal["status", "start", "cancel"] = "status"
    timeout_seconds: int = 900


class IMSetRequest(BaseModel):
    group_id: str
    platform: Literal["telegram", "slack", "discord", "feishu", "dingtalk"]
    # Legacy single token field (backward compat for telegram/discord)
    token_env: str = ""
    token: str = ""
    # Dual token fields for Slack
    bot_token_env: str = ""  # xoxb- for outbound (Web API)
    app_token_env: str = ""  # xapp- for inbound (Socket Mode)
    # Feishu fields
    feishu_domain: str = ""
    feishu_app_id: str = ""
    feishu_app_secret: str = ""
    # DingTalk fields
    dingtalk_app_key: str = ""
    dingtalk_app_secret: str = ""
    dingtalk_robot_code: str = ""


class IMActionRequest(BaseModel):
    group_id: str


class IMBindRequest(BaseModel):
    group_id: str
    key: str


class IMPendingRejectRequest(BaseModel):
    group_id: str
    key: str


def _normalize_command(cmd: Union[str, list[str], None]) -> Optional[list[str]]:
    if cmd is None:
        return None
    if isinstance(cmd, str):
        s = cmd.strip()
        return shlex.split(s, posix=(os.name != "nt")) if s else []
    if isinstance(cmd, list) and all(isinstance(x, str) for x in cmd):
        return [str(x).strip() for x in cmd if str(x).strip()]
    raise HTTPException(status_code=400, detail={"code": "invalid_command", "message": "invalid command"})


@dataclass
class RouteContext:
    """Typed context passed from create_app() to route registration functions."""

    home: Path
    version: str
    web_mode: str
    read_only: bool
    exhibit_cache_ttl_s: float
    exhibit_allow_terminal: bool
    dist_dir: Optional[Path]
    daemon: Callable[..., Awaitable[Dict[str, Any]]]
    cached_json: Callable[..., Awaitable[Dict[str, Any]]]
    apply_web_logging: Callable[..., None]
    configured_web_token: Callable[[], str]
