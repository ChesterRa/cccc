"""
Memory storage for CCCC groups (v2).

Structured recall system with time/task/actor/relation indexing.
Zero model dependency, pure agent-driven, SQLite FTS5 + structured indexes.

Storage: ~/.cccc/groups/<group_id>/memory.db
"""

from __future__ import annotations

import hashlib
import re
import sqlite3
import uuid
from datetime import datetime, timedelta, timezone
from typing import Any, Dict, List, Optional

from ..util.time import parse_utc_iso, utc_now_iso


# =============================================================================
# Constants
# =============================================================================

MEMORY_KINDS = ("observation", "decision", "preference", "fact", "instruction", "context", "relation")
MEMORY_STATUSES = ("draft", "solid")
MEMORY_SOURCE_TYPES = ("manual", "chat_ingest", "milestone_report", "agent_extract", "reflection")
MEMORY_CONFIDENCE_LEVELS = ("low", "medium", "high")

SCHEMA_VERSION = 3

AUTO_SOLIDIFY_HIT_THRESHOLD = 3

# Strategy definitions: control store behavior and auto-solidify
MEMORY_STRATEGIES: Dict[str, Dict[str, Any]] = {
    "aggressive": {
        "auto_solidify": True,       # store() creates as 'solid' directly
        "default_confidence": "medium",
    },
    "conservative": {
        "auto_solidify": False,      # relies on hit_count auto-solidify
        "default_confidence": "high",
    },
    "milestone-only": {
        "auto_solidify": True,       # store() creates as 'solid' directly
        "default_confidence": "high",
    },
}


# =============================================================================
# Helpers
# =============================================================================


def _validate_enum(value: str, allowed: tuple, field_name: str) -> None:
    """Validate that value is in the allowed enum tuple. Raises ValueError if not."""
    if value not in allowed:
        raise ValueError(f"Invalid {field_name}: {value!r}. Must be one of {allowed}")


def content_hash(content: str) -> str:
    """SHA-256 hash of content (no strip, preserves exact input)."""
    return hashlib.sha256(content.encode("utf-8")).hexdigest()


def _new_id() -> str:
    return uuid.uuid4().hex


def _sanitize_fts_query(query: str) -> str:
    """Wrap FTS5 query in double quotes to neutralize special characters."""
    escaped = query.replace('"', '""')
    return f'"{escaped}"'


_CJK_RE = re.compile(r"[\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af]")


def _has_cjk(text: str) -> bool:
    """Check if text contains CJK characters (Chinese/Japanese/Korean)."""
    return bool(_CJK_RE.search(text))


def _sort_solid_first(results: List[Dict[str, Any]], *, use_score: bool = False) -> List[Dict[str, Any]]:
    """Sort results: solid first, then by secondary key within each status group.

    When use_score=True (FTS query), sorts by score DESC within each group.
    Otherwise sorts by created_at DESC.
    """
    solid = [m for m in results if m.get("status") == "solid"]
    non_solid = [m for m in results if m.get("status") != "solid"]
    if use_score:
        solid.sort(key=lambda m: (m.get("score", 0.0), m.get("created_at", "")), reverse=True)
        non_solid.sort(key=lambda m: (m.get("score", 0.0), m.get("created_at", "")), reverse=True)
    else:
        solid.sort(key=lambda m: m.get("created_at", ""), reverse=True)
        non_solid.sort(key=lambda m: m.get("created_at", ""), reverse=True)
    return solid + non_solid


# =============================================================================
# Schema
# =============================================================================

_SCHEMA_SQL = """
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'observation',
    source_type TEXT NOT NULL DEFAULT 'manual',
    source_ref TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'draft',
    confidence TEXT NOT NULL DEFAULT 'medium',
    group_id TEXT NOT NULL,
    scope_key TEXT NOT NULL DEFAULT '',
    actor_id TEXT NOT NULL DEFAULT '',
    task_id TEXT NOT NULL DEFAULT '',
    milestone_id TEXT NOT NULL DEFAULT '',
    event_ts TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    content_hash TEXT NOT NULL DEFAULT '',
    hit_count INTEGER NOT NULL DEFAULT 0,
    last_recalled_at TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS memory_tags (
    memory_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY (memory_id, tag),
    FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    content,
    content=memories,
    content_rowid=rowid
);

CREATE TABLE IF NOT EXISTS memory_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT ''
);

-- FTS5 sync triggers
CREATE TRIGGER IF NOT EXISTS memory_fts_insert AFTER INSERT ON memories BEGIN
    INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
END;

CREATE TRIGGER IF NOT EXISTS memory_fts_delete BEFORE DELETE ON memories BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
END;

CREATE TRIGGER IF NOT EXISTS memory_fts_update AFTER UPDATE OF content ON memories BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
    INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
END;
"""

_INDEX_SQL = """
CREATE INDEX IF NOT EXISTS idx_memories_group_id ON memories(group_id);
CREATE INDEX IF NOT EXISTS idx_memories_status ON memories(status);
CREATE INDEX IF NOT EXISTS idx_memories_actor_id ON memories(actor_id);
CREATE INDEX IF NOT EXISTS idx_memories_task_id ON memories(task_id);
CREATE INDEX IF NOT EXISTS idx_memories_milestone_id ON memories(milestone_id);
CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(kind);
CREATE INDEX IF NOT EXISTS idx_memories_source_type ON memories(source_type);
CREATE INDEX IF NOT EXISTS idx_memories_event_ts ON memories(event_ts);
CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at);
CREATE INDEX IF NOT EXISTS idx_memories_content_hash ON memories(content_hash);
CREATE INDEX IF NOT EXISTS idx_memories_confidence ON memories(confidence);
CREATE INDEX IF NOT EXISTS idx_memories_last_recalled_at ON memories(last_recalled_at);
CREATE INDEX IF NOT EXISTS idx_memory_tags_memory_id ON memory_tags(memory_id);
"""


# =============================================================================
# MemoryStore
# =============================================================================


class MemoryStore:
    """SQLite-backed memory store for a single group."""

    def __init__(self, db_path: str, *, group_id: str):
        self.db_path = db_path
        self.group_id = group_id
        self._conn: Optional[sqlite3.Connection] = None
        self._connect()
        self._init_schema()

    def _connect(self) -> None:
        self._conn = sqlite3.connect(self.db_path)
        self._conn.row_factory = sqlite3.Row
        self._conn.execute("PRAGMA journal_mode=WAL")
        self._conn.execute("PRAGMA foreign_keys=ON")

    def _init_schema(self) -> None:
        assert self._conn is not None
        v = self._conn.execute("PRAGMA user_version").fetchone()[0]
        if v == 0:
            # Fresh DB — create all tables at latest schema version
            self._conn.executescript(_SCHEMA_SQL)
            self._conn.executescript(_INDEX_SQL)
            # PRAGMA user_version does not support parameter binding (?);
            # SCHEMA_VERSION is a code constant, not user input.
            self._conn.execute(f"PRAGMA user_version = {SCHEMA_VERSION}")
            self._conn.commit()
        elif v < SCHEMA_VERSION:
            self._migrate(v)
        # YAGNI cleanup: remove legacy relation artifacts kept from old versions.
        self._drop_legacy_relation_artifacts()

    def _drop_legacy_relation_artifacts(self) -> None:
        """Remove deprecated relation table/indexes if present."""
        assert self._conn is not None
        self._conn.execute("DROP INDEX IF EXISTS idx_memory_relations_from")
        self._conn.execute("DROP INDEX IF EXISTS idx_memory_relations_to")
        self._conn.execute("DROP TABLE IF EXISTS memory_relations")
        self._conn.commit()

    def _migrate(self, from_version: int) -> None:
        """Run incremental migrations from from_version to SCHEMA_VERSION."""
        assert self._conn is not None
        if from_version < 2:
            # v1 → v2: add memory_meta table + last_recalled_at column
            self._conn.execute(
                "CREATE TABLE IF NOT EXISTS memory_meta ("
                "key TEXT PRIMARY KEY, "
                "value TEXT NOT NULL DEFAULT '', "
                "updated_at TEXT NOT NULL DEFAULT ''"
                ")"
            )
            self._conn.execute(
                "ALTER TABLE memories ADD COLUMN last_recalled_at TEXT NOT NULL DEFAULT ''"
            )
            self._conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_memories_last_recalled_at "
                "ON memories(last_recalled_at)"
            )
        if from_version < 3:
            # v2 → v3: add summary column
            self._conn.execute(
                "ALTER TABLE memories ADD COLUMN summary TEXT NOT NULL DEFAULT ''"
            )
        # Bump version
        self._conn.execute(f"PRAGMA user_version = {SCHEMA_VERSION}")
        self._conn.commit()

    # -- Context manager --

    def __enter__(self) -> "MemoryStore":
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        self.close()

    def close(self) -> None:
        if self._conn is not None:
            self._conn.close()
            self._conn = None

    # -- Meta KV --

    def set_meta(self, key: str, value: str) -> None:
        """Set a metadata key-value pair (upsert)."""
        assert self._conn is not None
        now = utc_now_iso()
        self._conn.execute(
            "INSERT INTO memory_meta (key, value, updated_at) VALUES (?, ?, ?) "
            "ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            (key, value, now),
        )
        self._conn.commit()

    def get_meta(self, key: str, default: Optional[str] = None) -> Optional[str]:
        """Get a metadata value by key. Returns default if not found."""
        assert self._conn is not None
        row = self._conn.execute(
            "SELECT value FROM memory_meta WHERE key = ?", (key,)
        ).fetchone()
        if row is None:
            return default
        return row[0]

    def delete_meta(self, key: str) -> bool:
        """Delete a metadata key. Returns True if deleted."""
        assert self._conn is not None
        cur = self._conn.execute("DELETE FROM memory_meta WHERE key = ?", (key,))
        self._conn.commit()
        return cur.rowcount > 0

    # -- CRUD: store --

    def store(
        self,
        content: str,
        *,
        kind: str = "observation",
        source_type: str = "manual",
        source_ref: str = "",
        status: str = "draft",
        confidence: str = "medium",
        scope_key: str = "",
        actor_id: str = "",
        task_id: str = "",
        milestone_id: str = "",
        event_ts: str = "",
        tags: Optional[List[str]] = None,
        memory_id: Optional[str] = None,
        strategy: str = "",
        summary: str = "",
    ) -> Dict[str, Any]:
        """Store a new memory. Returns the created memory dict.

        Deduplicates by (content_hash + source_ref) within the same group.
        When strategy is set, overrides status/confidence based on MEMORY_STRATEGIES.
        """
        assert self._conn is not None

        # Enum validation
        _validate_enum(kind, MEMORY_KINDS, "kind")
        _validate_enum(source_type, MEMORY_SOURCE_TYPES, "source_type")
        _validate_enum(status, MEMORY_STATUSES, "status")
        _validate_enum(confidence, MEMORY_CONFIDENCE_LEVELS, "confidence")
        if strategy and strategy not in MEMORY_STRATEGIES:
            raise ValueError(
                f"Invalid strategy: {strategy!r}. Must be one of {tuple(MEMORY_STRATEGIES.keys())}"
            )

        # Apply strategy overrides
        if strategy and strategy in MEMORY_STRATEGIES:
            strat = MEMORY_STRATEGIES[strategy]
            if strat["auto_solidify"]:
                status = "solid"
            confidence = strat["default_confidence"]

        c_hash = content_hash(content)

        # Dedup check: same source_ref + same content = idempotent retry.
        # Different source_ref (or empty) + same content = separate records.
        if source_ref:
            row = self._conn.execute(
                "SELECT id FROM memories WHERE group_id = ? AND content_hash = ? AND source_ref = ?",
                (self.group_id, c_hash, source_ref),
            ).fetchone()
        else:
            row = self._conn.execute(
                "SELECT id FROM memories WHERE group_id = ? AND content_hash = ? AND source_ref = ''",
                (self.group_id, c_hash),
            ).fetchone()
        if row is not None:
            return {"id": row["id"], "deduplicated": True}

        now = utc_now_iso()
        mid = memory_id or _new_id()

        self._conn.execute(
            """INSERT INTO memories
               (id, content, kind, source_type, source_ref, status, confidence,
                group_id, scope_key, actor_id, task_id, milestone_id,
                event_ts, created_at, updated_at, content_hash, summary)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (
                mid, content, kind, source_type, source_ref, status, confidence,
                self.group_id, scope_key, actor_id, task_id, milestone_id,
                event_ts, now, now, c_hash, summary,
            ),
        )

        if tags:
            for tag in tags:
                tag = tag.strip()
                if tag:
                    self._conn.execute(
                        "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?, ?)",
                        (mid, tag),
                    )

        self._conn.commit()
        return {"id": mid, "deduplicated": False, "content_hash": c_hash, "created_at": now, "status": status}

    # -- Solidify --

    def solidify(self, memory_id: str) -> Optional[Dict[str, Any]]:
        """Manually solidify a memory: draft → solid.

        Returns updated memory dict, or None if not found.
        No-op if already solid.
        """
        assert self._conn is not None
        row = self._conn.execute(
            "SELECT id, status FROM memories WHERE id = ? AND group_id = ?",
            (memory_id, self.group_id),
        ).fetchone()
        if row is None:
            return None
        if row["status"] == "solid":
            return self.get(memory_id)

        now = utc_now_iso()
        self._conn.execute(
            "UPDATE memories SET status = 'solid', updated_at = ? WHERE id = ? AND group_id = ?",
            (now, memory_id, self.group_id),
        )
        self._conn.commit()
        return self.get(memory_id)

    def solidify_batch(
        self,
        *,
        milestone_id: str = "",
        kind: str = "",
    ) -> Dict[str, Any]:
        """Batch solidify: draft → solid for matching memories.

        Returns {"solidified": count, "ids": [solidified_ids]}.
        """
        assert self._conn is not None
        clauses = ["group_id = ?", "status = 'draft'"]
        params: List[Any] = [self.group_id]

        if milestone_id:
            clauses.append("milestone_id = ?")
            params.append(milestone_id)
        if kind:
            _validate_enum(kind, MEMORY_KINDS, "kind")
            clauses.append("kind = ?")
            params.append(kind)

        where = " AND ".join(clauses)

        # Get IDs first for return value
        rows = self._conn.execute(
            f"SELECT id FROM memories WHERE {where}", params
        ).fetchall()
        ids = [r["id"] for r in rows]

        if ids:
            now = utc_now_iso()
            placeholders = ", ".join("?" for _ in ids)
            self._conn.execute(
                f"UPDATE memories SET status = 'solid', updated_at = ? "
                f"WHERE id IN ({placeholders})",
                [now] + ids,
            )
            self._conn.commit()

            # Record in meta
            import json
            self.set_meta("last_solidify_batch", json.dumps({
                "count": len(ids),
                "milestone_id": milestone_id,
                "kind": kind,
                "at": now,
            }))

        return {"solidified": len(ids), "ids": ids}

    # -- CRUD: get --

    def get(self, memory_id: str) -> Optional[Dict[str, Any]]:
        """Get a single memory by ID (scoped to this group)."""
        assert self._conn is not None
        row = self._conn.execute(
            "SELECT * FROM memories WHERE id = ? AND group_id = ?",
            (memory_id, self.group_id),
        ).fetchone()
        if row is None:
            return None
        mem = dict(row)
        mem["tags"] = self._get_tags(memory_id)
        return mem

    def _get_tags(self, memory_id: str) -> List[str]:
        assert self._conn is not None
        rows = self._conn.execute(
            "SELECT tag FROM memory_tags WHERE memory_id = ? ORDER BY tag",
            (memory_id,),
        ).fetchall()
        return [r["tag"] for r in rows]

    # -- CRUD: update --

    def update(
        self,
        memory_id: str,
        *,
        content: Optional[str] = None,
        kind: Optional[str] = None,
        status: Optional[str] = None,
        confidence: Optional[str] = None,
        source_type: Optional[str] = None,
        source_ref: Optional[str] = None,
        actor_id: Optional[str] = None,
        task_id: Optional[str] = None,
        milestone_id: Optional[str] = None,
        event_ts: Optional[str] = None,
        tags: Optional[List[str]] = None,
        summary: Optional[str] = None,
    ) -> Optional[Dict[str, Any]]:
        """Update a memory. Returns updated memory dict or None if not found."""
        assert self._conn is not None

        # Enum validation
        if kind is not None:
            _validate_enum(kind, MEMORY_KINDS, "kind")
        if status is not None:
            _validate_enum(status, MEMORY_STATUSES, "status")
        if confidence is not None:
            _validate_enum(confidence, MEMORY_CONFIDENCE_LEVELS, "confidence")
        if source_type is not None:
            _validate_enum(source_type, MEMORY_SOURCE_TYPES, "source_type")

        existing = self.get(memory_id)
        if existing is None:
            return None

        sets: List[str] = []
        vals: List[Any] = []

        if content is not None:
            sets.append("content = ?")
            vals.append(content)
            sets.append("content_hash = ?")
            vals.append(content_hash(content))
        if kind is not None:
            sets.append("kind = ?")
            vals.append(kind)
        if status is not None:
            sets.append("status = ?")
            vals.append(status)
        if confidence is not None:
            sets.append("confidence = ?")
            vals.append(confidence)
        if source_type is not None:
            sets.append("source_type = ?")
            vals.append(source_type)
        if source_ref is not None:
            sets.append("source_ref = ?")
            vals.append(source_ref)
        if actor_id is not None:
            sets.append("actor_id = ?")
            vals.append(actor_id)
        if task_id is not None:
            sets.append("task_id = ?")
            vals.append(task_id)
        if milestone_id is not None:
            sets.append("milestone_id = ?")
            vals.append(milestone_id)
        if event_ts is not None:
            sets.append("event_ts = ?")
            vals.append(event_ts)
        if summary is not None:
            sets.append("summary = ?")
            vals.append(summary)

        if sets:
            now = utc_now_iso()
            sets.append("updated_at = ?")
            vals.append(now)
            vals.extend([memory_id, self.group_id])
            self._conn.execute(
                f"UPDATE memories SET {', '.join(sets)} WHERE id = ? AND group_id = ?",
                vals,
            )

        if tags is not None:
            self._conn.execute("DELETE FROM memory_tags WHERE memory_id = ?", (memory_id,))
            for tag in tags:
                tag = tag.strip()
                if tag:
                    self._conn.execute(
                        "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?, ?)",
                        (memory_id, tag),
                    )

        self._conn.commit()
        return self.get(memory_id)

    # -- CRUD: delete --

    def delete(self, memory_id: str) -> bool:
        """Delete a memory by ID (scoped to this group). Returns True if deleted."""
        assert self._conn is not None
        # Tags and relations deleted by CASCADE
        cur = self._conn.execute(
            "DELETE FROM memories WHERE id = ? AND group_id = ?",
            (memory_id, self.group_id),
        )
        self._conn.commit()
        return cur.rowcount > 0

    def delete_many(self, memory_ids: List[str]) -> Dict[str, Any]:
        """Batch delete by IDs (scoped to this group).

        Returns {"deleted": <count>, "ids": [deleted_ids]}.
        """
        assert self._conn is not None

        # Normalize + de-duplicate while preserving caller order.
        seen: set[str] = set()
        ids: List[str] = []
        for raw_id in memory_ids:
            mid = str(raw_id or "").strip()
            if not mid or mid in seen:
                continue
            seen.add(mid)
            ids.append(mid)

        if not ids:
            return {"deleted": 0, "ids": []}

        placeholders = ", ".join("?" for _ in ids)
        rows = self._conn.execute(
            f"SELECT id FROM memories WHERE group_id = ? AND id IN ({placeholders})",
            [self.group_id] + ids,
        ).fetchall()
        found = {str(r["id"]) for r in rows}
        deleted_ids = [mid for mid in ids if mid in found]

        if deleted_ids:
            delete_placeholders = ", ".join("?" for _ in deleted_ids)
            self._conn.execute(
                f"DELETE FROM memories WHERE group_id = ? AND id IN ({delete_placeholders})",
                [self.group_id] + deleted_ids,
            )
            self._conn.commit()

        return {"deleted": len(deleted_ids), "ids": deleted_ids}

    # -- CRUD: list --

    def list_memories(
        self,
        *,
        status: Optional[str] = None,
        kind: Optional[str] = None,
        limit: int = 50,
        offset: int = 0,
    ) -> List[Dict[str, Any]]:
        """List memories for this group with optional filters."""
        assert self._conn is not None
        clauses = ["group_id = ?"]
        params: List[Any] = [self.group_id]

        if status is not None:
            clauses.append("status = ?")
            params.append(status)
        if kind is not None:
            clauses.append("kind = ?")
            params.append(kind)

        where = " AND ".join(clauses)
        params.extend([limit, offset])
        rows = self._conn.execute(
            f"SELECT * FROM memories WHERE {where} ORDER BY created_at DESC LIMIT ? OFFSET ?",
            params,
        ).fetchall()

        results = []
        for row in rows:
            mem = dict(row)
            mem["tags"] = self._get_tags(mem["id"])
            results.append(mem)
        return results

    # -- Lookup by hash --

    def find_by_hash(self, c_hash: str) -> Optional[Dict[str, Any]]:
        """Find a memory by content hash within this group."""
        assert self._conn is not None
        row = self._conn.execute(
            "SELECT * FROM memories WHERE group_id = ? AND content_hash = ?",
            (self.group_id, c_hash),
        ).fetchone()
        if row is None:
            return None
        mem = dict(row)
        mem["tags"] = self._get_tags(mem["id"])
        return mem

    # -- Stats --

    def stats(self) -> Dict[str, Any]:
        """Return memory statistics for this group."""
        assert self._conn is not None
        total = self._conn.execute(
            "SELECT COUNT(*) as c FROM memories WHERE group_id = ?",
            (self.group_id,),
        ).fetchone()["c"]

        by_status = {}
        for row in self._conn.execute(
            "SELECT status, COUNT(*) as c FROM memories WHERE group_id = ? GROUP BY status",
            (self.group_id,),
        ).fetchall():
            by_status[row["status"]] = row["c"]

        by_kind = {}
        for row in self._conn.execute(
            "SELECT kind, COUNT(*) as c FROM memories WHERE group_id = ? GROUP BY kind",
            (self.group_id,),
        ).fetchall():
            by_kind[row["kind"]] = row["c"]

        by_source = {}
        for row in self._conn.execute(
            "SELECT source_type, COUNT(*) as c FROM memories WHERE group_id = ? GROUP BY source_type",
            (self.group_id,),
        ).fetchall():
            by_source[row["source_type"]] = row["c"]

        tag_count = self._conn.execute(
            "SELECT COUNT(DISTINCT tag) as c FROM memory_tags mt "
            "JOIN memories m ON mt.memory_id = m.id WHERE m.group_id = ?",
            (self.group_id,),
        ).fetchone()["c"]

        return {
            "total": total,
            "by_status": by_status,
            "by_kind": by_kind,
            "by_source_type": by_source,
            "tag_count": tag_count,
        }

    # -- Decay / stale candidate discovery --

    def find_stale(
        self,
        *,
        draft_days: int = 30,
        zero_hit_days: int = 14,
        solid_review_days: int = 120,
        solid_max_hit: int = 1,
        limit: int = 100,
    ) -> Dict[str, Any]:
        """Identify stale memory candidates for cleanup/decay (safe, read-only).

        Rules:
        - Draft memories older than ``draft_days`` (by last_recalled_at fallback created_at)
          are delete candidates.
        - Draft with hit_count=0 and older than ``zero_hit_days`` are high-priority delete candidates.
        - Solid memories are never auto-delete candidates; only review candidates when old and low-hit.
        """
        assert self._conn is not None

        if draft_days < 1:
            raise ValueError("draft_days must be >= 1")
        if zero_hit_days < 1:
            raise ValueError("zero_hit_days must be >= 1")
        if solid_review_days < 1:
            raise ValueError("solid_review_days must be >= 1")
        if solid_max_hit < 0:
            raise ValueError("solid_max_hit must be >= 0")
        if limit < 1:
            raise ValueError("limit must be >= 1")

        now = datetime.now(timezone.utc)
        draft_cutoff = (now - timedelta(days=draft_days)).isoformat().replace("+00:00", "Z")
        zero_hit_cutoff = (now - timedelta(days=zero_hit_days)).isoformat().replace("+00:00", "Z")
        solid_cutoff = (now - timedelta(days=solid_review_days)).isoformat().replace("+00:00", "Z")
        anchor_expr = "COALESCE(NULLIF(last_recalled_at, ''), created_at)"

        def _run_bucket(where_sql: str, params: Dict[str, Any]) -> List[sqlite3.Row]:
            query = f"""
                SELECT
                    id,
                    content,
                    kind,
                    status,
                    hit_count,
                    created_at,
                    last_recalled_at,
                    {anchor_expr} AS anchor_ts
                FROM memories
                WHERE group_id = :group_id
                  AND {where_sql}
                ORDER BY anchor_ts ASC, created_at ASC
                LIMIT :limit
            """
            args = {"group_id": self.group_id, "limit": limit}
            args.update(params)
            return self._conn.execute(query, args).fetchall()

        # SQL coarse filter + SQL ordering + SQL limit per bucket.
        # Bucket order defines global priority: high (draft zero-hit) -> medium (draft old) -> low (solid review).
        draft_zero_hit_rows = _run_bucket(
            (
                "status = 'draft' AND hit_count = 0 AND "
                "((last_recalled_at != '' AND last_recalled_at <= :zero_hit_cutoff) "
                "OR (last_recalled_at = '' AND created_at <= :zero_hit_cutoff))"
            ),
            {"zero_hit_cutoff": zero_hit_cutoff},
        )
        draft_old_rows = _run_bucket(
            (
                "status = 'draft' AND "
                "((last_recalled_at != '' AND last_recalled_at <= :draft_cutoff) "
                "OR (last_recalled_at = '' AND created_at <= :draft_cutoff))"
            ),
            {"draft_cutoff": draft_cutoff},
        )
        solid_review_rows = _run_bucket(
            (
                "status = 'solid' AND hit_count <= :solid_max_hit AND "
                "((last_recalled_at != '' AND last_recalled_at <= :solid_cutoff) "
                "OR (last_recalled_at = '' AND created_at <= :solid_cutoff))"
            ),
            {"solid_cutoff": solid_cutoff, "solid_max_hit": solid_max_hit},
        )

        candidates: List[Dict[str, Any]] = []
        seen_ids: set[str] = set()

        def _append_row(row: sqlite3.Row, *, score: int, action: str) -> None:
            rid = str(row["id"] or "")
            if not rid or rid in seen_ids:
                return
            seen_ids.add(rid)
            created_raw = str(row["created_at"] or "")
            recalled_raw = str(row["last_recalled_at"] or "")
            anchor_raw = str(row["anchor_ts"] or "")
            anchor_dt = parse_utc_iso(anchor_raw) or parse_utc_iso(created_raw) or now
            age_days = max(0, int((now - anchor_dt).total_seconds() // 86400))

            status = str(row["status"] or "")
            hit_count = int(row["hit_count"] or 0)
            reasons: List[str] = []
            if action == "delete_candidate":
                if age_days >= draft_days:
                    reasons.append(f"draft_older_than_{draft_days}d")
                if hit_count == 0 and age_days >= zero_hit_days:
                    reasons.append(f"zero_hit_older_than_{zero_hit_days}d")
            elif action == "review_candidate":
                if age_days >= solid_review_days and hit_count <= solid_max_hit:
                    reasons.append(f"solid_review_older_than_{solid_review_days}d")

            priority = "low"
            if score >= 4:
                priority = "high"
            elif score >= 2:
                priority = "medium"

            candidates.append(
                {
                    "id": str(row["id"] or ""),
                    "content_preview": str(row["content"] or "")[:120],
                    "kind": str(row["kind"] or ""),
                    "status": status,
                    "hit_count": hit_count,
                    "created_at": created_raw,
                    "last_recalled_at": recalled_raw,
                    "age_days": age_days,
                    "reasons": reasons,
                    "recommended_action": action,
                    "priority": priority,
                    "score": score,
                }
            )

        for row in draft_zero_hit_rows:
            _append_row(row, score=5, action="delete_candidate")
        for row in draft_old_rows:
            _append_row(row, score=2, action="delete_candidate")
        for row in solid_review_rows:
            _append_row(row, score=1, action="review_candidate")

        candidates = candidates[:limit]

        delete_candidates = [c for c in candidates if c.get("recommended_action") == "delete_candidate"]
        review_candidates = [c for c in candidates if c.get("recommended_action") == "review_candidate"]

        return {
            "candidates": candidates,
            "count": len(candidates),
            "delete_candidate_count": len(delete_candidates),
            "review_candidate_count": len(review_candidates),
            "config": {
                "draft_days": draft_days,
                "zero_hit_days": zero_hit_days,
                "solid_review_days": solid_review_days,
                "solid_max_hit": solid_max_hit,
                "limit": limit,
            },
        }

    # -- Recall (structured search) --

    def recall(
        self,
        query: str = "",
        *,
        status: str = "",
        kind: str = "",
        actor_id: str = "",
        task_id: str = "",
        milestone_id: str = "",
        confidence: str = "",
        tags: Optional[List[str]] = None,
        since: str = "",
        until: str = "",
        limit: int = 20,
        track_hit: bool = False,
        depth: str = "L0",
    ) -> List[Dict[str, Any]]:
        """Structured memory recall with FTS5 search and filters.

        Returns memories sorted by solid-first, then created_at DESC.
        Each result includes a 'score' field (FTS5 rank or LIKE match indicator).

        When query contains CJK characters (len >= 2), supplements FTS5
        with LIKE matching and merges results with dedup.

        When track_hit=True, increments hit_count and updates last_recalled_at
        on all returned memories, and auto-solidifies drafts reaching the threshold.
        Default is False (no side effects on query).

        depth controls returned fields:
        - "L0" (default): returns summary instead of content (saves tokens).
          If summary is empty, falls back to content[:150]+"…".
        - "L2": returns full content + summary.
        """
        assert self._conn is not None

        # Validate depth
        if depth not in ("L0", "L2"):
            raise ValueError(f"Invalid depth: {depth!r}. Must be 'L0' or 'L2'.")

        # Enum validation for filters
        if kind:
            _validate_enum(kind, MEMORY_KINDS, "kind")
        if status:
            _validate_enum(status, MEMORY_STATUSES, "status")
        if confidence:
            _validate_enum(confidence, MEMORY_CONFIDENCE_LEVELS, "confidence")

        filter_kwargs = dict(
            status=status, kind=kind, actor_id=actor_id,
            task_id=task_id, milestone_id=milestone_id,
            confidence=confidence, tags=tags,
            since=since, until=until, limit=limit,
        )

        if not query.strip():
            results = self._recall_no_query(**filter_kwargs)
        else:
            # FTS5 search
            results = self._recall_fts(query, **filter_kwargs)

            # CJK supplement: LIKE match for CJK queries with len >= 2
            if _has_cjk(query) and len(query.strip()) >= 2:
                like_results = self._recall_like(query, **filter_kwargs)
                seen_ids = {m["id"] for m in results}
                for m in like_results:
                    if m["id"] not in seen_ids:
                        results.append(m)
                        seen_ids.add(m["id"])

        # Sort: solid first; use FTS5 score when text query is present
        has_query = bool(query.strip())
        results = _sort_solid_first(results, use_score=has_query)[:limit]

        # Increment hit_count + update last_recalled_at only when explicitly requested.
        # Default is no side effects (track_hit=False).
        if track_hit and results:
            now = utc_now_iso()
            ids = [m["id"] for m in results]
            placeholders = ", ".join("?" for _ in ids)
            self._conn.execute(
                f"UPDATE memories SET hit_count = hit_count + 1, last_recalled_at = ? "
                f"WHERE id IN ({placeholders})",
                [now] + ids,
            )
            # Auto-solidify: draft memories that reach the hit threshold.
            # Params: [updated_at, *ids, threshold] — all bound via ?.
            self._conn.execute(
                f"UPDATE memories SET status = 'solid', updated_at = ? "
                f"WHERE id IN ({placeholders}) "
                f"AND status = 'draft' AND hit_count >= ?",
                [now] + ids + [AUTO_SOLIDIFY_HIT_THRESHOLD],
            )
            self._conn.commit()
            # Update hit_count, last_recalled_at and status in returned results
            for m in results:
                new_hit = m.get("hit_count", 0) + 1
                m["hit_count"] = new_hit
                m["last_recalled_at"] = now
                if m.get("status") == "draft" and new_hit >= AUTO_SOLIDIFY_HIT_THRESHOLD:
                    m["status"] = "solid"

        # Apply depth projection
        for m in results:
            m["depth"] = depth
            if depth == "L0":
                summary = m.get("summary", "")
                if not summary:
                    full = m.get("content", "")
                    summary = (full[:150] + "\u2026") if len(full) > 150 else full
                m["summary"] = summary
                m.pop("content", None)
            # depth == "L2": keep both content and summary as-is

        return results

    def _build_filter_clauses(
        self,
        *,
        status: str,
        kind: str,
        actor_id: str,
        task_id: str,
        milestone_id: str,
        confidence: str,
        tags: Optional[List[str]],
        since: str,
        until: str,
    ) -> tuple[List[str], List[Any]]:
        """Build WHERE clauses and params for recall filters."""
        clauses = ["m.group_id = ?"]
        params: List[Any] = [self.group_id]

        if status:
            clauses.append("m.status = ?")
            params.append(status)
        if kind:
            clauses.append("m.kind = ?")
            params.append(kind)
        if actor_id:
            clauses.append("m.actor_id = ?")
            params.append(actor_id)
        if task_id:
            clauses.append("m.task_id = ?")
            params.append(task_id)
        if milestone_id:
            clauses.append("m.milestone_id = ?")
            params.append(milestone_id)
        if confidence:
            clauses.append("m.confidence = ?")
            params.append(confidence)
        if since:
            clauses.append("m.created_at >= ?")
            params.append(since)
        if until:
            clauses.append("m.created_at <= ?")
            params.append(until)
        if tags:
            for tag in tags:
                tag = tag.strip()
                if tag:
                    clauses.append(
                        "EXISTS (SELECT 1 FROM memory_tags t WHERE t.memory_id = m.id AND t.tag = ?)"
                    )
                    params.append(tag)

        return clauses, params

    def _recall_no_query(
        self, *, status: str, kind: str, actor_id: str,
        task_id: str, milestone_id: str, confidence: str,
        tags: Optional[List[str]], since: str, until: str, limit: int,
    ) -> List[Dict[str, Any]]:
        """Recall without text search - just filters."""
        assert self._conn is not None
        clauses, params = self._build_filter_clauses(
            status=status, kind=kind, actor_id=actor_id,
            task_id=task_id, milestone_id=milestone_id,
            confidence=confidence, tags=tags, since=since, until=until,
        )
        where = " AND ".join(clauses)
        params.append(limit)
        rows = self._conn.execute(
            f"SELECT m.*, 0.0 as score FROM memories m WHERE {where} "
            "ORDER BY CASE m.status WHEN 'solid' THEN 0 ELSE 1 END, m.created_at DESC "
            "LIMIT ?",
            params,
        ).fetchall()
        results = []
        for row in rows:
            mem = dict(row)
            mem["tags"] = self._get_tags(mem["id"])
            results.append(mem)
        return results

    def _recall_fts(
        self, query: str, *, status: str, kind: str, actor_id: str,
        task_id: str, milestone_id: str, confidence: str,
        tags: Optional[List[str]], since: str, until: str, limit: int,
    ) -> List[Dict[str, Any]]:
        """FTS5 full-text search with filters."""
        assert self._conn is not None
        sanitized = _sanitize_fts_query(query.strip())
        clauses, params = self._build_filter_clauses(
            status=status, kind=kind, actor_id=actor_id,
            task_id=task_id, milestone_id=milestone_id,
            confidence=confidence, tags=tags, since=since, until=until,
        )
        # FTS5 join
        clauses.append("m.rowid IN (SELECT rowid FROM memory_fts WHERE memory_fts MATCH ?)")
        params.append(sanitized)
        where = " AND ".join(clauses)
        params.append(limit)

        try:
            rows = self._conn.execute(
                f"SELECT m.*, "
                f"(SELECT rank FROM memory_fts WHERE memory_fts MATCH ? AND rowid = m.rowid) as score "
                f"FROM memories m WHERE {where} "
                f"ORDER BY CASE m.status WHEN 'solid' THEN 0 ELSE 1 END, m.created_at DESC "
                f"LIMIT ?",
                [sanitized] + params,
            ).fetchall()
        except sqlite3.OperationalError:
            # FTS5 parse error (shouldn't happen with sanitized query, but be safe)
            return []

        results = []
        for row in rows:
            mem = dict(row)
            mem["tags"] = self._get_tags(mem["id"])
            # Normalize score: FTS5 rank is negative (more negative = better match)
            mem["score"] = abs(float(mem.get("score") or 0.0))
            results.append(mem)
        return results

    def _recall_like(
        self, query: str, *, status: str, kind: str, actor_id: str,
        task_id: str, milestone_id: str, confidence: str,
        tags: Optional[List[str]], since: str, until: str, limit: int,
    ) -> List[Dict[str, Any]]:
        """LIKE-based search for CJK content (FTS5 supplement)."""
        assert self._conn is not None
        clauses, params = self._build_filter_clauses(
            status=status, kind=kind, actor_id=actor_id,
            task_id=task_id, milestone_id=milestone_id,
            confidence=confidence, tags=tags, since=since, until=until,
        )
        # Escape LIKE wildcards in user query to prevent unintended pattern matching
        escaped = query.strip().replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
        clauses.append("m.content LIKE ? ESCAPE '\\'")
        params.append(f"%{escaped}%")
        where = " AND ".join(clauses)
        params.append(limit)

        rows = self._conn.execute(
            f"SELECT m.*, 0.5 as score FROM memories m WHERE {where} "
            f"ORDER BY CASE m.status WHEN 'solid' THEN 0 ELSE 1 END, m.created_at DESC "
            f"LIMIT ?",
            params,
        ).fetchall()

        results = []
        for row in rows:
            mem = dict(row)
            mem["tags"] = self._get_tags(mem["id"])
            mem["score"] = 0.5  # LIKE match indicator
            results.append(mem)
        return results
