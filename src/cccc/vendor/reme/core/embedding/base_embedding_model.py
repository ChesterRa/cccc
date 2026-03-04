"""Minimal embedding model interface for vendored ReMe file-store usage."""

from __future__ import annotations

from abc import ABC
from typing import Any


class BaseEmbeddingModel(ABC):
    def __init__(self, dimensions: int | None = 1024, **kwargs: Any) -> None:
        self.dimensions = int(dimensions or 1024)
        self.kwargs = kwargs

    async def get_embedding(self, query: str, **kwargs: Any) -> list[float]:
        raise NotImplementedError

    async def get_embeddings(self, queries: list[str], **kwargs: Any) -> list[list[float]]:
        raise NotImplementedError

    async def get_chunk_embedding(self, chunk: Any, **kwargs: Any) -> Any:
        raise NotImplementedError

    async def get_chunk_embeddings(self, chunks: list[Any], **kwargs: Any) -> list[Any]:
        raise NotImplementedError
