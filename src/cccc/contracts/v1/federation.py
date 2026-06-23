"""Federation (remote send) contracts — Stage 1 minimal schema.

These models describe the canonical envelope, receipt, error and registration
record used by the outbound remote-send feature. They are transport-agnostic:
the same payload shape is translated by each RemoteSendTransport.

Security invariant: a raw remote credential (token/secret) MUST NEVER be a
field on any of these models. Registrations carry only an opaque
``credential_ref`` that points into a separate secrets store.
"""

from __future__ import annotations

from typing import Dict, List, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field

RegistrationStatus = Literal["active", "unauthorized", "revoked", "error"]
RemoteSendStatus = Literal["queued", "sending", "retrying", "sent", "failed"]


class RemoteSendPayload(BaseModel):
    """Canonical message body for a remote send.

    Intentionally mirrors the local ``send`` arguments so transports translate
    from a single shape instead of each inventing its own.
    """

    text: str
    format: Literal["plain", "markdown"] = "plain"
    priority: Literal["normal", "attention"] = "normal"
    reply_required: bool = False
    to: List[str] = Field(default_factory=list)
    refs: List[Dict[str, object]] = Field(default_factory=list)
    attachments: List[Dict[str, object]] = Field(default_factory=list)
    source_by: str = ""  # Original sender id in the source group, e.g. user or actor_id.

    model_config = ConfigDict(extra="forbid")


class RemoteSendEnvelope(BaseModel):
    """A queued unit of remote-send work."""

    src_group_id: str
    registration_id: str
    idempotency_key: str
    payload: RemoteSendPayload

    model_config = ConfigDict(extra="forbid")


class RemoteSendError(BaseModel):
    """Structured, secret-free error describing a failed remote send."""

    code: str
    message: str = ""
    retriable: bool = False
    transport: str = ""
    http_status: Optional[int] = None

    model_config = ConfigDict(extra="forbid")


class RemoteSendReceipt(BaseModel):
    """Result of a remote-send attempt (also the queued placeholder)."""

    ok: bool = False
    status: RemoteSendStatus = "queued"
    idempotency_key: str = ""
    registration_id: str = ""
    transport: str = ""
    remote_event_id: Optional[str] = None
    attempt: int = 0
    max_attempts: int = 5
    first_queued_at: str = ""
    last_attempt_at: str = ""
    next_attempt_at: str = ""
    accepted_at: str = ""
    error: Optional[RemoteSendError] = None

    model_config = ConfigDict(extra="forbid")


class RegistrationRecord(BaseModel):
    """A persisted outbound registration of a local group to a remote target.

    No raw credential is stored here — only ``credential_ref``.
    """

    registration_id: str
    group_id: str
    url: str
    transport: str = "registry_hub"
    remote_group_id: str = ""
    remote_peer_id: str = ""
    multiaddrs: List[str] = Field(default_factory=list)
    credential_ref: str = ""
    user_id: str = ""
    status: RegistrationStatus = "active"
    created_at: str
    updated_at: str
    last_sync_at: Optional[str] = None
    last_error: Optional[str] = None

    model_config = ConfigDict(extra="forbid")
