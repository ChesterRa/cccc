"""Memory chunk schema."""

from pydantic import BaseModel, Field

from ..enumeration import MemorySource


class MemoryChunk(BaseModel):
    """A chunk of memory content with metadata."""

    id: str = Field(..., description="Unique identifier for the chunk")
    path: str = Field(..., description="File path relative to workspace")
    source: MemorySource = Field(..., description="Source of the memory data")
    start_line: int = Field(..., description="Starting line number in the source file")
    end_line: int = Field(..., description="Ending line number in the source file")
    text: str = Field(..., description="Text content of the chunk")
    hash: str = Field(..., description="Hash of the chunk content")
    embedding: list[float] | None = Field(default=None, description="Vector embedding of the chunk")
    metadata: dict = Field(default_factory=dict, description="Additional metadata")
