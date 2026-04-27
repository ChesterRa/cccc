from __future__ import annotations

import os
import shlex
from dataclasses import dataclass
from pathlib import Path
from types import SimpleNamespace
from typing import Any, Awaitable, Callable, Dict, Literal, Optional, Union

from fastapi import Depends, HTTPException, Path as FastApiPath, Request, WebSocket
from pydantic import BaseModel, ConfigDict, Field

from ...contracts.v1.actor import ActorSubmit, AgentRuntime, RunnerKind
from ...contracts.v1.automation import AutomationRule
from ...kernel.access_tokens import list_access_tokens, lookup_access_token


def _default_runner_kind() -> str:
    return "pty"


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
    quote_text: str = Field(default="")
    priority: Literal["normal", "attention"] = "normal"
    reply_required: bool = False
    src_group_id: str = Field(default="")
    src_event_id: str = Field(default="")
    client_id: str = Field(default="")
    refs: list[dict[str, Any]] = Field(default_factory=list)


class SendCrossGroupRequest(BaseModel):
    text: str
    by: str = Field(default="user")
    dst_group_id: str
    to: list[str] = Field(default_factory=list)
    priority: Literal["normal", "attention"] = "normal"
    reply_required: bool = False


class TrackedSendRequest(BaseModel):
    title: str
    text: str
    by: str = Field(default="user")
    to: list[str] = Field(default_factory=list)
    outcome: str = Field(default="")
    checklist: list[dict[str, Any]] = Field(default_factory=list)
    assignee: str = Field(default="")
    waiting_on: Literal["none", "user", "actor", "external"] | str = "actor"
    handoff_to: str = Field(default="")
    notes: str = Field(default="")
    priority: Literal["normal", "attention"] = "normal"
    reply_required: bool = True
    idempotency_key: str = Field(default="")
    refs: list[dict[str, Any]] = Field(default_factory=list)


class ReplyRequest(BaseModel):
    text: str
    by: str = Field(default="user")
    to: list[str] = Field(default_factory=list)
    reply_to: str
    priority: Literal["normal", "attention"] = "normal"
    reply_required: bool = False
    client_id: str = Field(default="")
    refs: list[dict[str, Any]] = Field(default_factory=list)


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
    # Note: role is auto-determined by stable position (first visible actor = foreman)
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
    profile_scope: Optional[Literal["global", "user"]] = None
    profile_owner: Optional[str] = None
    default_scope_key: str = Field(default="")
    submit: ActorSubmit = Field(default="enter")
    by: str = Field(default="user")


class ActorUpdateRequest(BaseModel):
    by: str = Field(default="user")
    # Note: role is ignored - auto-determined by position
    title: Optional[str] = None
    avatar_asset_path: Optional[str] = None
    command: Optional[Union[str, list[str]]] = None
    env: Optional[Dict[str, str]] = None
    capability_autoload: Optional[list[str]] = None
    default_scope_key: Optional[str] = None
    submit: Optional[ActorSubmit] = None
    runner: Optional[RunnerKind] = None
    runtime: Optional[AgentRuntime] = None
    enabled: Optional[bool] = None
    profile_id: Optional[str] = None
    profile_scope: Optional[Literal["global", "user"]] = None
    profile_owner: Optional[str] = None
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
    editor_mode: Optional[Literal["structured", "raw"]] = None
    changed_blocks: list[str] = Field(default_factory=list)


class GroupUpdateRequest(BaseModel):
    title: Optional[str] = None
    topic: Optional[str] = None
    by: str = Field(default="user")


class GroupPresentationPublishRequest(BaseModel):
    slot: str = Field(default="auto")
    url: str = Field(default="")
    title: str = Field(default="")
    summary: str = Field(default="")
    by: str = Field(default="user")


class GroupPresentationPublishWorkspaceRequest(BaseModel):
    slot: str = Field(default="auto")
    path: str = Field(default="")
    title: str = Field(default="")
    summary: str = Field(default="")
    by: str = Field(default="user")


class GroupPresentationClearRequest(BaseModel):
    slot: str = Field(default="")
    all: bool = False
    by: str = Field(default="user")


class GroupPresentationBrowserSessionRequest(BaseModel):
    slot: str = Field(default="")
    url: str = Field(default="")
    width: int = Field(default=1280)
    height: int = Field(default=800)
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

    # Features
    panorama_enabled: Optional[bool] = None
    desktop_pet_enabled: Optional[bool] = None

    by: str = Field(default="user")


class AssistantSettingsUpdateRequest(BaseModel):
    enabled: Optional[bool] = None
    config: Optional[Dict[str, Any]] = None
    by: str = Field(default="user")


class AssistantStatusUpdateRequest(BaseModel):
    assistant_id: Optional[str] = None
    lifecycle: Literal["disabled", "idle", "running", "working", "waiting", "failed"]
    health: Dict[str, Any] = Field(default_factory=dict)
    by: str = Field(default="user")


class AssistantVoiceTranscriptionRequest(BaseModel):
    audio_base64: str = Field(default="")
    mime_type: str = Field(default="application/octet-stream")
    language: str = Field(default="")
    by: str = Field(default="user")


class AssistantVoiceModelInstallRequest(BaseModel):
    model_id: str = Field(default="")
    by: str = Field(default="user")
    background: bool = Field(default=False)


class AssistantVoiceRuntimeInstallRequest(BaseModel):
    runtime_id: str = Field(default="")
    by: str = Field(default="user")
    background: bool = Field(default=True)


class AssistantVoiceTranscriptSegmentRequest(BaseModel):
    session_id: str = Field(default="")
    segment_id: str = Field(default="")
    document_path: str = Field(default="")
    text: str = Field(default="")
    language: str = Field(default="")
    is_final: bool = Field(default=True)
    flush: bool = Field(default=False)
    trigger: Dict[str, Any] = Field(default_factory=dict)
    by: str = Field(default="user")


class AssistantVoiceDocumentSaveRequest(BaseModel):
    document_path: str = Field(default="")
    workspace_path: str = Field(default="")
    title: str = Field(default="")
    content: Optional[str] = None
    status: str = Field(default="")
    create_new: bool = Field(default=False)
    by: str = Field(default="user")


class AssistantVoiceDocumentInstructionRequest(BaseModel):
    document_path: str = Field(default="")
    instruction: str = Field(default="")
    source_text: str = Field(default="")
    trigger: Dict[str, Any] = Field(default_factory=dict)
    by: str = Field(default="user")


class AssistantVoiceInputRequest(BaseModel):
    kind: str = Field(default="")
    text: str = Field(default="")
    instruction: str = Field(default="")
    source_text: str = Field(default="")
    document_path: str = Field(default="")
    voice_transcript: str = Field(default="")
    composer_text: str = Field(default="")
    request_id: str = Field(default="")
    operation: str = Field(default="")
    composer_context: Dict[str, Any] = Field(default_factory=dict)
    composer_snapshot_hash: str = Field(default="")
    language: str = Field(default="")
    trigger: Dict[str, Any] = Field(default_factory=dict)
    by: str = Field(default="user")


class AssistantVoicePromptDraftAckRequest(BaseModel):
    request_id: str = Field(default="")
    status: Literal["applied", "dismissed", "stale"]
    by: str = Field(default="user")


class AssistantVoiceAskRequestsClearRequest(BaseModel):
    keep_active: bool = Field(default=False)
    by: str = Field(default="user")


class PetDecisionOutcomeRequest(BaseModel):
    fingerprint: str
    outcome: Literal["executed", "dismissed"]
    decision_id: str = Field(default="")
    action_type: str = Field(default="")
    cooldown_ms: int = Field(default=0)
    source_event_id: str = Field(default="")
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
    logger_levels: Optional[Dict[str, str]] = None
    terminal_transcript_per_actor_bytes: Optional[int] = None
    terminal_ui_scrollback_lines: Optional[int] = None
    peer_runtime_visibility: Optional[Literal["hidden", "visible"]] = None
    pet_runtime_visibility: Optional[Literal["hidden", "visible"]] = None


class RegistryReconcileRequest(BaseModel):
    by: str = Field(default="user")
    remove_missing: bool = False


class RemoteAccessConfigureRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[Literal["off", "manual", "tailscale"]] = None
    mode: Optional[str] = None
    enabled: Optional[bool] = None
    require_access_token: Optional[bool] = None
    web_host: Optional[str] = None
    web_port: Optional[int] = None
    web_public_url: Optional[str] = None


class BrandingUpdateRequest(BaseModel):
    by: str = Field(default="user")
    product_name: Optional[str] = None
    clear_logo_icon: bool = False
    clear_favicon: bool = False


class GroupSpaceBindRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    lane: Literal["work", "memory"]
    action: Literal["bind", "unbind"] = "bind"
    remote_space_id: str = Field(default="")


class GroupSpaceIngestRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    lane: Literal["work", "memory"]
    kind: Literal["context_sync", "resource_ingest"] = "context_sync"
    payload: Dict[str, Any] = Field(default_factory=dict)
    idempotency_key: str = Field(default="")


class GroupSpaceQueryRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")
    provider: Optional[str] = Field(default="notebooklm")
    lane: Literal["work", "memory"]
    query: str = Field(default="")
    options: Dict[str, Any] = Field(default_factory=dict)


class GroupSpaceSourceActionRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    lane: Literal["work", "memory"]
    action: Literal["delete", "rename", "refresh"]
    source_id: str = Field(default="")
    new_title: str = Field(default="")


class GroupSpaceArtifactActionRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    lane: Literal["work", "memory"]
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
    lane: Literal["work", "memory"]
    action: Literal["retry", "cancel"]
    job_id: str = Field(default="")


class GroupSpaceSyncRequest(BaseModel):
    by: str = Field(default="user")
    provider: Optional[str] = Field(default="notebooklm")
    lane: Literal["work", "memory"]
    action: Literal["status", "run"] = "run"
    force: bool = False


class GroupSpaceProviderCredentialUpdateRequest(BaseModel):
    by: str = Field(default="user")
    auth_json: str = Field(default="")
    clear: bool = False


class GroupSpaceProviderAuthRequest(BaseModel):
    by: str = Field(default="user")
    action: Literal["status", "start", "cancel", "disconnect"] = "status"
    timeout_seconds: int = 900
    force_reauth: bool = False
    projected: bool = False


class IMSetRequest(BaseModel):
    group_id: str
    platform: Literal["telegram", "slack", "discord", "feishu", "dingtalk", "wecom", "weixin"]
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
    # WeCom fields
    wecom_bot_id: str = ""
    wecom_secret: str = ""
    # Weixin fields
    weixin_account_id: str = ""


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


def _anonymous_principal() -> Any:
    return SimpleNamespace(kind="anonymous", user_id="", allowed_groups=(), is_admin=False)


def _tokens_enabled() -> bool:
    return bool(list_access_tokens())


def _principal_kind(principal: Any) -> str:
    return str(getattr(principal, "kind", "anonymous") or "anonymous").strip() or "anonymous"


def _principal_allowed_groups(principal: Any) -> tuple[str, ...]:
    raw = getattr(principal, "allowed_groups", ()) or ()
    if not isinstance(raw, (list, tuple, set)):
        return ()
    values = [str(item or "").strip() for item in raw]
    return tuple(item for item in values if item)


def _principal_is_admin(principal: Any) -> bool:
    return bool(getattr(principal, "is_admin", False))


def _extract_group_item_id(item: Any) -> str:
    if not isinstance(item, dict):
        return ""
    return str(item.get("id") or item.get("group_id") or "").strip()


def get_principal(conn: Request | WebSocket) -> Any:
    state = getattr(conn, "state", None)
    principal = getattr(state, "principal", None)
    return principal if principal is not None else _anonymous_principal()


def check_admin(conn: Request | WebSocket) -> Any:
    principal = get_principal(conn)
    if not _tokens_enabled():
        return principal
    if _principal_kind(principal) == "user" and _principal_is_admin(principal):
        return principal
    raise HTTPException(status_code=403, detail={"code": "permission_denied", "message": "admin access required", "details": {}})


def check_group(conn: Request | WebSocket, group_id: str) -> Any:
    principal = get_principal(conn)
    if not _tokens_enabled():
        return principal
    gid = str(group_id or "").strip()
    allowed_groups = _principal_allowed_groups(principal)
    if _principal_kind(principal) != "user":
        raise HTTPException(status_code=403, detail={"code": "permission_denied", "message": "group access required", "details": {"group_id": gid}})
    if _principal_is_admin(principal):
        return principal
    if gid and gid in allowed_groups:
        return principal
    raise HTTPException(status_code=403, detail={"code": "permission_denied", "message": "group access denied", "details": {"group_id": gid}})


def require_admin(request: Request) -> Any:
    return check_admin(request)


def require_user(request: Request) -> Any:
    """Allow any authenticated user (admin or non-admin). Reject non-user principals."""
    principal = get_principal(request)
    if not _tokens_enabled():
        return principal
    if _principal_kind(principal) == "user":
        return principal
    raise HTTPException(status_code=403, detail={"code": "permission_denied", "message": "authentication required", "details": {}})


def require_group(request: Request, group_id: str = FastApiPath(...)) -> Any:
    return check_group(request, group_id)


def filter_groups_for_principal(conn: Request | WebSocket, groups: list[dict[str, Any]]) -> list[dict[str, Any]]:
    principal = get_principal(conn)
    if not _tokens_enabled() or _principal_is_admin(principal):
        return groups
    if _principal_kind(principal) != "user":
        return []
    allowed_groups = _principal_allowed_groups(principal)
    if not allowed_groups:
        return []
    allowed = set(allowed_groups)
    return [item for item in groups if _extract_group_item_id(item) in allowed]


def _extract_token_from_headers(headers: Any) -> str:
    try:
        auth = str(headers.get("authorization") or "").strip()
    except Exception:
        auth = ""
    if auth.lower().startswith("bearer "):
        return str(auth[7:] or "").strip()
    return ""


def resolve_websocket_principal(websocket: WebSocket) -> Any:
    token = _extract_token_from_headers(getattr(websocket, "headers", {}) or {})
    if not token:
        try:
            cookies = getattr(websocket, "cookies", None) or {}
            token = str(cookies.get("cccc_access_token") or "").strip()
        except Exception:
            token = ""
    if not token:
        try:
            token = str(websocket.query_params.get("token") or "").strip()
        except Exception:
            token = ""
    if not token:
        return _anonymous_principal()
    entry = lookup_access_token(token)
    if not isinstance(entry, dict):
        return _anonymous_principal()
    return SimpleNamespace(
        kind="user",
        user_id=str(entry.get("user_id") or "").strip(),
        allowed_groups=tuple(str(item or "").strip() for item in (entry.get("allowed_groups") or []) if str(item or "").strip()),
        is_admin=bool(entry.get("is_admin", False)),
    )


def websocket_tokens_active() -> bool:
    """Check if access-token auth is active for WebSocket flows."""
    return _tokens_enabled()
