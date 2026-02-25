from __future__ import annotations

from typing import Any, Dict, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field

from ...util.time import utc_now_iso

SpaceProviderId = Literal["notebooklm"]
SpaceProviderMode = Literal["disabled", "active", "degraded"]
SpaceBindingStatus = Literal["bound", "unbound", "error"]
SpaceJobState = Literal["pending", "running", "succeeded", "failed", "canceled"]
SpaceJobKind = Literal["context_sync", "resource_ingest"]
SpaceJobAction = Literal["list", "retry", "cancel"]
SpaceCredentialSource = Literal["none", "store", "env"]


class SpaceProviderState(BaseModel):
    provider: SpaceProviderId = "notebooklm"
    enabled: bool = False
    real_enabled: bool = False
    mode: SpaceProviderMode = "disabled"
    last_health_at: Optional[str] = None
    last_error: Optional[str] = None

    model_config = ConfigDict(extra="forbid")


class SpaceProviderCredentialState(BaseModel):
    provider: SpaceProviderId = "notebooklm"
    key: str = ""
    configured: bool = False
    source: SpaceCredentialSource = "none"
    env_configured: bool = False
    store_configured: bool = False
    updated_at: Optional[str] = None
    masked_value: Optional[str] = None

    model_config = ConfigDict(extra="forbid")


class SpaceBinding(BaseModel):
    group_id: str
    provider: SpaceProviderId = "notebooklm"
    remote_space_id: str = ""
    bound_by: str = ""
    bound_at: str = Field(default_factory=utc_now_iso)
    status: SpaceBindingStatus = "bound"

    model_config = ConfigDict(extra="forbid")


class SpaceJobError(BaseModel):
    code: str = ""
    message: str = ""

    model_config = ConfigDict(extra="forbid")


class SpaceJob(BaseModel):
    job_id: str
    group_id: str
    provider: SpaceProviderId = "notebooklm"
    remote_space_id: str = ""
    kind: SpaceJobKind = "context_sync"
    payload: Dict[str, Any] = Field(default_factory=dict)
    payload_digest: str = ""
    idempotency_key: str = ""
    state: SpaceJobState = "pending"
    attempt: int = 0
    max_attempts: int = 3
    next_run_at: Optional[str] = None
    created_at: str = Field(default_factory=utc_now_iso)
    updated_at: str = Field(default_factory=utc_now_iso)
    last_error: SpaceJobError = Field(default_factory=SpaceJobError)

    model_config = ConfigDict(extra="forbid")


class SpaceQueueSummary(BaseModel):
    pending: int = 0
    running: int = 0
    failed: int = 0

    model_config = ConfigDict(extra="forbid")
