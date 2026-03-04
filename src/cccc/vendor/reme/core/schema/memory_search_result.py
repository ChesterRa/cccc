"""Memory search result schema."""

from pydantic import BaseModel, Field

from ..enumeration import MemorySource


class MemorySearchResult(BaseModel):
    """Search result from memory index."""

    path: str = Field(..., description="File path relative to workspace")
    start_line: int = Field(..., description="Starting line number of the match")
    end_line: int = Field(..., description="Ending line number of the match")
    score: float = Field(..., description="Relevance score of the search result")
    snippet: str = Field(..., description="Text snippet from the matched content")
    source: MemorySource = Field(..., description="Source of the memory data")
    raw_metric: float | None = Field(None, description="Raw metric value from search (e.g., distance, rank)")
    metadata: dict = Field(default_factory=dict, description="Additional metadata")

    @property
    def merge_key(self) -> str:
        """Merge key for the search result."""
        return self.path + f":{self.start_line}:{self.end_line}"
