from __future__ import annotations

from typing import Any, Dict, Optional

from pydantic import BaseModel, ConfigDict, Field


class DaemonRequest(BaseModel):
    v: int = 1
    op: str
    args: Dict[str, Any] = Field(default_factory=dict)

    model_config = ConfigDict(extra="forbid")


class DaemonError(BaseModel):
    code: str
    message: str
    details: Dict[str, Any] = Field(default_factory=dict)

    model_config = ConfigDict(extra="forbid")


class DaemonResponse(BaseModel):
    v: int = 1
    ok: bool
    result: Dict[str, Any] = Field(default_factory=dict)
    error: Optional[DaemonError] = None

    model_config = ConfigDict(extra="forbid")

