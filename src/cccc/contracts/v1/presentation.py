from __future__ import annotations

from typing import List, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field


PresentationCardType = Literal["markdown", "table", "image", "pdf", "file", "web_preview"]
PresentationSourceMode = Literal["inline", "reference", "workspace_link"]


class PresentationTableData(BaseModel):
    columns: List[str] = Field(default_factory=list)
    rows: List[List[str]] = Field(default_factory=list)

    model_config = ConfigDict(extra="forbid")


class PresentationContent(BaseModel):
    mode: PresentationSourceMode = "inline"
    markdown: Optional[str] = None
    table: Optional[PresentationTableData] = None
    url: Optional[str] = None
    blob_rel_path: Optional[str] = None
    workspace_rel_path: Optional[str] = None
    mime_type: Optional[str] = None
    file_name: Optional[str] = None

    model_config = ConfigDict(extra="forbid")


class PresentationCard(BaseModel):
    slot_id: str
    title: str
    card_type: PresentationCardType
    published_by: str
    published_at: str
    source_label: str = ""
    source_ref: str = ""
    summary: str = ""
    content: PresentationContent

    model_config = ConfigDict(extra="forbid")


class PresentationSlot(BaseModel):
    slot_id: str
    index: int
    card: Optional[PresentationCard] = None

    model_config = ConfigDict(extra="forbid")


class PresentationSnapshot(BaseModel):
    v: int = 1
    updated_at: str = ""
    highlight_slot_id: str = ""
    slots: List[PresentationSlot] = Field(default_factory=list)

    model_config = ConfigDict(extra="forbid")
