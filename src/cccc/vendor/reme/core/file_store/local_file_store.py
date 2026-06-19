"""Pure-Python in-memory storage backend for file store, with JSON file persistence."""

import json
import logging
import math
import re
from collections import Counter
from pathlib import Path

from .base_file_store import BaseFileStore
from ..enumeration import MemorySource
from ..schema import FileMetadata, MemoryChunk, MemorySearchResult
from ..utils.common_utils import batch_cosine_similarity

logger = logging.getLogger(__name__)


class LocalFileStore(BaseFileStore):
    """Pure-Python in-memory file storage with JSONL file persistence.

    No external dependencies required. All data lives in Python dicts;
    writes are persisted to JSONL files on disk so state survives restarts.

    Inherits embedding methods from BaseFileStore:
    - get_chunk_embedding / get_chunk_embeddings (async)
    - get_embedding / get_embeddings (async)

    Provides:
    - Vector similarity search (cosine similarity, pure Python)
    - Full-text / keyword search (Python substring matching)
    - Efficient chunk and file metadata management
    """

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self._started: bool = False
        # In-memory indexes
        self._chunks: dict[str, MemoryChunk] = {}
        self._files: dict[str, dict[str, FileMetadata]] = {}  # source -> path -> meta
        # Persistence paths (mirror ChromaFileStore convention)
        self._chunks_file: Path = self.db_path / f"{self.store_name}_chunks.jsonl"
        self._metadata_file: Path = self.db_path / f"{self.store_name}_file_metadata.json"

    _CJK_RUN_RE = re.compile(r"[\u4e00-\u9fff]+")
    _WORD_RE = re.compile(r"(?u)[a-zA-Z0-9_]+(?:[._:/-][a-zA-Z0-9_]+)*")
    _WORD_SPLIT_RE = re.compile(r"[_./:\-]+")

    @classmethod
    def _keyword_tokens(cls, text: str) -> list[str]:
        """Tokenize text for local keyword ranking without optional dependencies.

        This follows the same dependency-free direction as ReMe4's regex tokenizer:
        CJK text is indexed by characters and bigrams, while latin/code-like tokens
        keep both the full identifier and separator-delimited parts.
        """
        raw = str(text or "").lower()
        tokens: list[str] = []
        for run in cls._CJK_RUN_RE.findall(raw):
            tokens.extend(run)
            tokens.extend(run[i : i + 2] for i in range(max(0, len(run) - 1)))

        latin_text = cls._CJK_RUN_RE.sub(" ", raw)
        for word in cls._WORD_RE.findall(latin_text):
            if len(word) >= 2:
                tokens.append(word)
            for part in cls._WORD_SPLIT_RE.split(word):
                if len(part) >= 2 and part != word:
                    tokens.append(part)
        return tokens

    # ------------------------------------------------------------------
    # Persistence helpers
    # ------------------------------------------------------------------

    async def _load_chunks(self) -> None:
        """Load chunks from JSONL file into memory."""
        if not self._chunks_file.exists():
            return
        try:
            data = self._chunks_file.read_text(encoding="utf-8")
            self._chunks = {}
            for line in data.strip().split("\n"):
                if not line:
                    continue
                rec = json.loads(line)
                chunk = MemoryChunk.model_validate(rec)
                self._chunks[chunk.id] = chunk
            logger.debug(f"Loaded {len(self._chunks)} chunks from {self._chunks_file}")
        except Exception as e:
            logger.warning(f"Failed to load chunks from {self._chunks_file}: {e}")

    async def _save_chunks(self) -> None:
        """Persist chunks to JSONL file."""
        try:
            lines = []
            for chunk in self._chunks.values():
                chunk_dict = chunk.model_dump(mode="json")
                lines.append(json.dumps(chunk_dict, ensure_ascii=False))
            data = "\n".join(lines)
            self._chunks_file.write_text(data, encoding="utf-8")
            logger.debug(f"Saved {len(self._chunks)} chunks to {self._chunks_file}")
        except Exception as e:
            logger.error(f"Failed to save chunks to {self._chunks_file}: {e}")

    async def _load_metadata(self) -> None:
        """Load file metadata from JSON file into memory."""
        if not self._metadata_file.exists():
            return
        try:
            data = self._metadata_file.read_text(encoding="utf-8")
            raw: dict = json.loads(data)
            self._files = {
                source: {path: FileMetadata(**meta) for path, meta in files.items()} for source, files in raw.items()
            }
            logger.debug(f"Loaded file metadata from {self._metadata_file}")
        except Exception as e:
            logger.warning(f"Failed to load file metadata from {self._metadata_file}: {e}")

    async def _save_metadata(self) -> None:
        """Persist file metadata to JSON file."""
        try:
            raw: dict = {}
            for source, files in self._files.items():
                raw[source] = {
                    path: {
                        "path": meta.path,
                        "hash": meta.hash,
                        "mtime_ms": meta.mtime_ms,
                        "size": meta.size,
                        "chunk_count": meta.chunk_count,
                    }
                    for path, meta in files.items()
                }
            data = json.dumps(raw, indent=2, ensure_ascii=False)
            self._metadata_file.write_text(data, encoding="utf-8")
            logger.debug(f"Saved file metadata to {self._metadata_file}")
        except Exception as e:
            logger.error(f"Failed to save file metadata to {self._metadata_file}: {e}")

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    async def start(self) -> None:
        """Load persisted data into memory."""
        if self._started:
            return
        self._started = True
        await self._load_metadata()
        await self._load_chunks()
        logger.info(
            f"LocalFileStore '{self.store_name}' ready: "
            f"{len(self._chunks)} chunks, metadata at {self._metadata_file}",
        )

    async def close(self) -> None:
        """Flush state to disk and release memory."""
        await self._save_metadata()
        await self._save_chunks()
        self._chunks.clear()
        self._files.clear()
        self._started = False

    # ------------------------------------------------------------------
    # Write operations
    # ------------------------------------------------------------------

    async def upsert_file(
        self,
        file_meta: FileMetadata,
        source: MemorySource,
        chunks: list[MemoryChunk],
    ) -> None:
        """Insert or update file and its chunks."""
        if not chunks:
            return

        # Remove existing chunks for this file/source first
        await self.delete_file(file_meta.path, source)

        # Batch generate embeddings (base class returns mock embeddings when vector_enabled=False)
        chunks = await self.get_chunk_embeddings(chunks)

        for chunk in chunks:
            self._chunks[chunk.id] = chunk

        if source.value not in self._files:
            self._files[source.value] = {}
        self._files[source.value][file_meta.path] = FileMetadata(
            hash=file_meta.hash,
            mtime_ms=file_meta.mtime_ms,
            size=file_meta.size,
            path=file_meta.path,
            chunk_count=len(chunks),
        )

    async def delete_file(self, path: str, source: MemorySource) -> None:
        """Delete file and all its chunks."""
        to_delete = [cid for cid, chunk in self._chunks.items() if chunk.path == path and chunk.source == source]
        for cid in to_delete:
            del self._chunks[cid]

        if source.value in self._files:
            self._files[source.value].pop(path, None)

    async def delete_file_chunks(self, path: str, chunk_ids: list[str]) -> None:
        """Delete specific chunks for a file."""
        if not chunk_ids:
            return

        for cid in chunk_ids:
            self._chunks.pop(cid, None)

        # Recalculate chunk_count in file metadata (per source)
        for source_key, source_meta in self._files.items():
            if path in source_meta:
                source_meta[path].chunk_count = sum(
                    1 for chunk in self._chunks.values() if chunk.path == path and chunk.source.value == source_key
                )

    async def upsert_chunks(
        self,
        chunks: list[MemoryChunk],
        source: MemorySource,
    ) -> None:
        """Insert or update specific chunks without affecting other chunks."""
        if not chunks:
            return

        chunks = await self.get_chunk_embeddings(chunks)

        for chunk in chunks:
            self._chunks[chunk.id] = chunk

    # ------------------------------------------------------------------
    # Read operations
    # ------------------------------------------------------------------

    async def list_files(self, source: MemorySource) -> list[str]:
        """List all indexed files for a source."""
        return list(self._files.get(source.value, {}).keys())

    async def get_file_metadata(
        self,
        path: str,
        source: MemorySource,
    ) -> FileMetadata | None:
        """Get file metadata."""
        return self._files.get(source.value, {}).get(path)

    async def update_file_metadata(self, file_meta: FileMetadata, source: MemorySource) -> None:
        """Update file metadata without affecting chunks."""
        if source.value not in self._files:
            self._files[source.value] = {}

        self._files[source.value][file_meta.path] = FileMetadata(
            hash=file_meta.hash,
            mtime_ms=file_meta.mtime_ms,
            size=file_meta.size,
            path=file_meta.path,
            chunk_count=file_meta.chunk_count,
        )

    async def get_file_chunks(
        self,
        path: str,
        source: MemorySource,
    ) -> list[MemoryChunk]:
        """Get all chunks for a file, sorted by start_line."""
        chunks = [chunk for chunk in self._chunks.values() if chunk.path == path and chunk.source == source]
        chunks.sort(key=lambda c: c.start_line)
        return chunks

    # ------------------------------------------------------------------
    # Search
    # ------------------------------------------------------------------

    async def vector_search(
        self,
        query: str,
        limit: int,
        sources: list[MemorySource] | None = None,
    ) -> list[MemorySearchResult]:
        """Perform cosine-similarity vector search over in-memory embeddings."""
        if not self.vector_enabled or not query:
            return []

        query_embedding = await self.get_embedding(query)
        if not query_embedding:
            return []

        # Collect candidate chunks with embeddings
        candidates = [
            chunk for chunk in self._chunks.values() if (not sources or chunk.source in sources) and chunk.embedding
        ]

        if not candidates:
            return []

        # Build embedding matrix and compute similarities in batch (pure python helper).
        query_array = [query_embedding]
        chunk_embeddings = [list(chunk.embedding or []) for chunk in candidates]
        similarities = batch_cosine_similarity(query_array, chunk_embeddings)[0]

        # Build results
        results = [
            MemorySearchResult(
                path=chunk.path,
                start_line=chunk.start_line,
                end_line=chunk.end_line,
                score=float(similarity),
                snippet=chunk.text,
                source=chunk.source,
                raw_metric=1.0 - float(similarity),
            )
            for chunk, similarity in zip(candidates, similarities)
        ]

        results.sort(key=lambda r: r.score, reverse=True)
        return results[:limit]

    async def keyword_search(
        self,
        query: str,
        limit: int,
        sources: list[MemorySource] | None = None,
    ) -> list[MemorySearchResult]:
        """Perform keyword/full-text search with a lightweight BM25-style ranker."""
        if not self.fts_enabled or not query:
            return []

        query_tokens = self._keyword_tokens(query)
        if not query_tokens:
            return []

        query_lower = query.lower()
        query_counts = Counter(query_tokens)
        query_terms = set(query_counts)

        documents = []
        for chunk in self._chunks.values():
            if sources and chunk.source not in sources:
                continue
            doc_tokens = self._keyword_tokens(chunk.text)
            if not doc_tokens:
                continue
            token_counts = Counter(doc_tokens)
            matched_terms = query_terms.intersection(token_counts)
            if not matched_terms:
                continue
            documents.append((chunk, token_counts, len(doc_tokens), matched_terms))

        if not documents:
            return []

        doc_count = len(documents)
        avg_len = sum(doc_len for _, _, doc_len, _ in documents) / doc_count
        doc_freq: Counter[str] = Counter()
        for _, _, _, matched_terms in documents:
            doc_freq.update(matched_terms)

        k1 = 1.5
        b = 0.75
        scored = []
        for chunk, token_counts, doc_len, matched_terms in documents:
            raw_score = 0.0
            for term, query_tf in query_counts.items():
                tf = token_counts.get(term, 0)
                if tf <= 0:
                    continue
                df = max(1, doc_freq.get(term, 0))
                idf = math.log(1.0 + (doc_count - df + 0.5) / (df + 0.5))
                denom = tf + k1 * (1.0 - b + b * (doc_len / max(avg_len, 1.0)))
                raw_score += idf * ((tf * (k1 + 1.0)) / denom) * min(query_tf, 2)
            if raw_score <= 0:
                continue
            scored.append((chunk, raw_score, len(matched_terms) / max(1, len(query_terms)), matched_terms))

        if not scored:
            return []

        max_raw_score = max(raw_score for _, raw_score, _, _ in scored) or 1.0
        results = []
        for chunk, raw_score, coverage, matched_terms in scored:
            phrase_bonus = 0.15 if query_lower in chunk.text.lower() else 0.0
            raw_rank = raw_score / max_raw_score
            # Coverage is absolute relevance: a one-token match in a multi-token
            # query must not be normalized into a high-confidence hit.
            score = min(1.0, coverage * (0.85 + raw_rank * 0.1) + phrase_bonus)

            results.append(
                MemorySearchResult(
                    path=chunk.path,
                    start_line=chunk.start_line,
                    end_line=chunk.end_line,
                    score=score,
                    snippet=chunk.text,
                    source=chunk.source,
                    raw_metric=raw_score,
                ),
            )

        results.sort(key=lambda r: r.score, reverse=True)
        return results[:limit]

    async def hybrid_search(
        self,
        query: str,
        limit: int,
        sources: list[MemorySource] | None = None,
        vector_weight: float = 0.7,
        candidate_multiplier: float = 3.0,
    ) -> list[MemorySearchResult]:
        """Perform hybrid search combining vector and keyword search.

        Args:
            query: Search query text
            limit: Maximum number of results
            sources: Optional list of sources to filter
            vector_weight: Weight for vector search results (0.0-1.0).
                          Keyword weight = 1.0 - vector_weight.
            candidate_multiplier: Multiplier for candidate pool size.

        Returns:
            List of search results sorted by combined relevance score
        """
        assert 0.0 <= vector_weight <= 1.0, f"vector_weight must be between 0 and 1, got {vector_weight}"

        candidates = min(200, max(1, int(limit * candidate_multiplier)))
        text_weight = 1.0 - vector_weight

        if self.vector_enabled and self.fts_enabled:
            keyword_results = await self.keyword_search(query, candidates, sources)
            vector_results = await self.vector_search(query, candidates, sources)

            logger.info("\n=== Vector Search Results ===")
            for i, r in enumerate(vector_results[:10], 1):
                snippet_preview = (r.snippet[:100] + "...") if len(r.snippet) > 100 else r.snippet
                logger.info(f"{i}. Score: {r.score:.4f} | Snippet: {snippet_preview}")

            logger.info("\n=== Keyword Search Results ===")
            for i, r in enumerate(keyword_results[:10], 1):
                snippet_preview = (r.snippet[:100] + "...") if len(r.snippet) > 100 else r.snippet
                logger.info(f"{i}. Score: {r.score:.4f} | Snippet: {snippet_preview}")

            if not keyword_results:
                return vector_results[:limit]
            elif not vector_results:
                return keyword_results[:limit]
            else:
                merged = self._merge_hybrid_results(
                    vector=vector_results,
                    keyword=keyword_results,
                    vector_weight=vector_weight,
                    text_weight=text_weight,
                )

                logger.info("\n=== Merged Hybrid Results ===")
                for i, r in enumerate(merged[:10], 1):
                    snippet_preview = (r.snippet[:100] + "...") if len(r.snippet) > 100 else r.snippet
                    logger.info(f"{i}. Score: {r.score:.4f} | Snippet: {snippet_preview}")

                return merged[:limit]
        elif self.vector_enabled:
            return await self.vector_search(query, limit, sources)
        elif self.fts_enabled:
            return await self.keyword_search(query, limit, sources)
        else:
            return []

    @staticmethod
    def _merge_hybrid_results(
        vector: list[MemorySearchResult],
        keyword: list[MemorySearchResult],
        vector_weight: float,
        text_weight: float,
    ) -> list[MemorySearchResult]:
        """Merge vector and keyword search results with weighted scoring."""
        merged: dict[str, MemorySearchResult] = {}

        for result in vector:
            result.metadata["_weighted_score"] = result.score * vector_weight
            merged[result.merge_key] = result

        for result in keyword:
            key = result.merge_key
            if key in merged:
                merged[key].metadata["_weighted_score"] += result.score * text_weight
            else:
                result.metadata["_weighted_score"] = result.score * text_weight
                merged[key] = result

        results = list(merged.values())
        for r in results:
            r.score = r.metadata.pop("_weighted_score")

        results.sort(key=lambda r: r.score, reverse=True)
        return results

    async def clear_all(self) -> None:
        """Clear all indexed data from memory and disk."""
        self._chunks.clear()
        self._files.clear()
        await self._save_chunks()
        await self._save_metadata()
        logger.info(f"Cleared all data from LocalFileStore '{self.store_name}'")
